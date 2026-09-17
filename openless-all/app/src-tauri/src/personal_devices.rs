//! Small, in-process device integration for the user's Bluetooth remote and DJI receiver.
use serde_json::{json, Value};

fn ensure_main(window: &tauri::Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Device controls are only available in the main window".into())
    }
}

#[tauri::command]
pub async fn get_personal_devices(window: tauri::Window) -> Result<Value, String> {
    ensure_main(&window)?;
    #[cfg(target_os = "windows")]
    return tauri::async_runtime::spawn_blocking(windows_devices::snapshot)
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "windows"))]
    Ok(json!({"supported": false}))
}

#[tauri::command]
pub async fn set_personal_microphone(
    window: tauri::Window,
    device_id: String,
    setting: String,
    value: String,
    tx: Option<usize>,
) -> Result<(), String> {
    ensure_main(&window)?;
    #[cfg(target_os = "windows")]
    return tauri::async_runtime::spawn_blocking(move || {
        windows_devices::set(device_id, setting, value, tx)
    })
    .await
    .map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "windows"))]
    Err("Device controls currently require Windows".into())
}

#[cfg(target_os = "windows")]
mod windows_devices {
    use super::*;
    use std::{
        sync::{Mutex, OnceLock},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    use windows::{
        core::{GUID, PCWSTR},
        Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothLEDevice},
        Win32::{
            Devices::{
                DeviceAndDriverInstallation::*,
                Properties::{DEVPROPKEY, DEVPROPTYPE},
            },
            Foundation::HWND,
        },
    };

    static MANAGER: OnceLock<dji_device::DeviceManager> = OnceLock::new();
    static REMOTE: OnceLock<Mutex<Option<(Instant, Value)>>> = OnceLock::new();
    const SETTINGS: &[&str] = &[
        "noise-cancel-power",
        "noise-cancel",
        "low-cut",
        "clip-limiter",
        "stereo",
        "voice-tone",
    ];
    fn manager() -> &'static dji_device::DeviceManager {
        MANAGER.get_or_init(dji_device::DeviceManager::new)
    }
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn snapshot() -> Result<Value, String> {
        let manager = manager();
        manager.refresh();
        let devices: Vec<Value> = manager.list().into_iter().map(|d| {
            let status = manager.status(&d.id).ok();
            json!({"id": d.id, "name": d.model_name, "connected": d.connected, "status": status})
        }).collect();
        let cache = REMOTE.get_or_init(|| Mutex::new(None));
        let mut cached = cache.lock().map_err(|e| e.to_string())?;
        if cached
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(30))
        {
            let value = match remote() {
                Ok(v) => v,
                Err(e) => {
                    json!({"name": "IINE_keyboard", "connected": null, "battery": null, "error": e, "checkedAt": now()})
                }
            };
            *cached = Some((Instant::now(), value));
        }
        Ok(
            json!({"supported": true, "remote": cached.as_ref().map(|(_, v)| v), "microphones": devices, "probe": manager.probe(), "checkedAt": now()}),
        )
    }

    pub fn set(
        id: String,
        setting: String,
        value: String,
        tx: Option<usize>,
    ) -> Result<(), String> {
        if !SETTINGS.contains(&setting.as_str()) {
            return Err("Unsupported microphone setting".into());
        }
        let manager = manager();
        let status = manager.status(&id).map_err(|e| e.to_string())?;
        if !status.connected {
            return Err("麦克风已断开，请重新连接后再试".into());
        }
        if setting == "stereo"
            && value == "stereo"
            && status
                .settings
                .get("safety-track")
                .is_some_and(|v| v == "on")
        {
            return Err("当前已启用安全音轨，不能同时启用立体声".into());
        }
        if setting == "voice-tone" {
            let slot = tx.filter(|&i| i < 2).ok_or("请选择有效的发射器")?;
            let transmitter = status.tx[slot].as_ref().ok_or("发射器未连接")?;
            if !transmitter
                .product_name
                .as_deref()
                .is_some_and(|n| n.contains("Mini 2"))
            {
                return Err("音色设置仅支持 DJI Mic Mini 2".into());
            }
            manager
                .set_tx(&id, slot, &setting, &value)
                .map_err(|e| e.to_string())?;
        } else {
            if tx.is_some() {
                return Err("此设置不支持单独指定发射器".into());
            }
            manager
                .set(&id, &setting, &value)
                .map_err(|e| e.to_string())?;
        }
        // A completed USB transfer is not an acknowledgement. Confirm via a fresh heartbeat.
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
            let status = manager.status(&id).map_err(|e| e.to_string())?;
            if !status.connected {
                return Err("设置过程中设备断开".into());
            }
            let actual = if setting == "voice-tone" {
                status.tx[tx.unwrap()]
                    .as_ref()
                    .and_then(|t| t.voice_tone.as_deref())
            } else {
                status.settings.get(&setting).map(String::as_str)
            };
            if actual == Some(value.as_str()) {
                return Ok(());
            }
        }
        Err("设备尚未确认新设置，请刷新查看实际状态".into())
    }

    struct DeviceSet(HDEVINFO);
    impl Drop for DeviceSet {
        fn drop(&mut self) {
            unsafe {
                let _ = SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }

    unsafe fn property(
        set: HDEVINFO,
        info: &SP_DEVINFO_DATA,
        guid: u128,
        pid: u32,
    ) -> Option<Vec<u8>> {
        let key = DEVPROPKEY {
            fmtid: GUID::from_u128(guid),
            pid,
        };
        let mut kind = DEVPROPTYPE(0);
        let mut buffer = vec![0u8; 2048];
        let mut size = 0;
        SetupDiGetDevicePropertyW(
            set,
            info,
            &key,
            &mut kind,
            Some(&mut buffer),
            Some(&mut size),
            0,
        )
        .ok()?;
        buffer.truncate(size as usize);
        Some(buffer)
    }
    fn wide(bytes: &[u8]) -> String {
        String::from_utf16_lossy(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .take_while(|v| *v != 0)
                .collect::<Vec<_>>(),
        )
    }
    fn remote() -> Result<Value, String> {
        unsafe {
            let enumerator: Vec<u16> = "BTHLE\0".encode_utf16().collect();
            let set = DeviceSet(
                SetupDiGetClassDevsW(
                    None,
                    PCWSTR(enumerator.as_ptr()),
                    HWND::default(),
                    DIGCF_ALLCLASSES | DIGCF_PRESENT,
                )
                .map_err(|e| e.to_string())?,
            );
            for index in 0..256 {
                let mut info = SP_DEVINFO_DATA {
                    cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                    ..Default::default()
                };
                if SetupDiEnumDeviceInfo(set.0, index, &mut info).is_err() {
                    break;
                }
                let name = property(set.0, &info, 0xa45c254e_df1c_4efd_8020_67d146a850e0, 14)
                    .map(|v| wide(&v))
                    .unwrap_or_default();
                if name != "IINE_keyboard" {
                    continue;
                }
                let battery = property(set.0, &info, 0x104ea319_6ee2_4701_bd47_8ddbf425bbe5, 2)
                    .and_then(|v| v.first().copied())
                    .filter(|v| *v <= 100);
                let mut instance = [0u16; 512];
                SetupDiGetDeviceInstanceIdW(set.0, &info, Some(&mut instance), None)
                    .map_err(|e| e.to_string())?;
                let id = String::from_utf16_lossy(
                    &instance[..instance
                        .iter()
                        .position(|v| *v == 0)
                        .unwrap_or(instance.len())],
                );
                let address = id
                    .split('\\')
                    .nth(1)
                    .and_then(|s| s.strip_prefix("DEV_"))
                    .and_then(|s| u64::from_str_radix(s, 16).ok());
                let connected = address.and_then(|address| {
                    let operation = BluetoothLEDevice::FromBluetoothAddressAsync(address).ok()?;
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while operation.Status().ok()? == windows::Foundation::AsyncStatus::Started {
                        if Instant::now() >= deadline {
                            let _ = operation.Cancel();
                            return None;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    let device = operation.GetResults().ok()?;
                    let result = device
                        .ConnectionStatus()
                        .ok()
                        .map(|s| s == BluetoothConnectionStatus::Connected);
                    let _ = device.Close();
                    result
                });
                return Ok(
                    json!({"name": name, "connected": connected, "battery": battery, "checkedAt": now(), "error": null}),
                );
            }
        }
        Ok(
            json!({"name": "IINE_keyboard", "connected": false, "battery": null, "checkedAt": now(), "error": null}),
        )
    }
}
