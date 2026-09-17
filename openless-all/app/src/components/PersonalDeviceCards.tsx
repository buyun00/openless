import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '../lib/ipc/shared';
import { detectOS } from './WindowChrome';
import './PersonalDeviceCards.css';

interface Transmitter {
  product_name: string | null;
  firmware: string | null;
  battery: number | null;
  charging: boolean | null;
  voice_tone: string | null;
}
interface Microphone {
  id: string;
  name: string;
  connected: boolean;
  status: {
    tx: (Transmitter | null)[];
    rx: { firmware: string | null } | null;
    gain_dial: number | null;
    protocol_version: number | null;
    settings: Record<string, string>;
  } | null;
}
interface Devices {
  supported: boolean;
  remote: {
    name: string;
    connected: boolean | null;
    battery: number | null;
    error?: string;
    checkedAt: number;
  };
  microphones: Microphone[];
  probe: { present: number; accessible: number; permission_issue: boolean };
}

export function djiBatteryLabel(gauge: number | null): string {
  return gauge === null || gauge < 1 || gauge > 7
    ? '电量未知'
    : `${8 - gauge}/7 格 · ${['满电', '较充足', '中高', '中等', '偏低', '低电量', '即将耗尽'][gauge - 1]}`;
}

const OPTIONS: Record<string, { label: string; values: [string, string][] }> = {
  'noise-cancel-power': {
    label: '降噪',
    values: [
      ['off', '关闭'],
      ['on', '开启'],
    ],
  },
  'noise-cancel': {
    label: '降噪强度',
    values: [
      ['basic', '基础'],
      ['strong', '强'],
    ],
  },
  'low-cut': {
    label: '低切',
    values: [
      ['off', '关闭'],
      ['on', '开启'],
    ],
  },
  'clip-limiter': {
    label: '防爆音限幅',
    values: [
      ['off', '关闭'],
      ['on', '开启'],
    ],
  },
  stereo: {
    label: '声道',
    values: [
      ['mono', '单声道'],
      ['stereo', '立体声'],
    ],
  },
  'voice-tone': {
    label: '音色',
    values: [
      ['standard', '自然'],
      ['rich', '饱满'],
      ['bright', '明亮'],
    ],
  },
};

function SettingSelect({
  setting,
  value,
  disabled,
  onChange,
  label,
}: {
  setting: string;
  value?: string | null;
  disabled: boolean;
  onChange: (value: string) => void;
  label?: string;
}) {
  const spec = OPTIONS[setting];
  return (
    <label className="ol-device-setting">
      <span>{label ?? spec.label}</span>
      <select
        aria-label={label ?? spec.label}
        value={value ?? ''}
        disabled={disabled || value == null}
        onChange={(e) => onChange(e.target.value)}
      >
        {value == null && <option value="">尚未读取</option>}
        {spec.values.map(([id, text]) => (
          <option key={id} value={id}>
            {text}
          </option>
        ))}
      </select>
    </label>
  );
}

export function PersonalDeviceCards() {
  const [data, setData] = useState<Devices | null>(null);
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState('');
  const alive = useRef(false);
  const fetching = useRef(false);
  const writing = useRef(false);
  const enabled = isTauri && detectOS() === 'win';
  const refresh = useCallback(async () => {
    if (fetching.current) return;
    fetching.current = true;
    try {
      const next = await invoke<Devices>('get_personal_devices');
      if (alive.current) {
        setData(next);
        setError('');
      }
    } catch (e) {
      if (alive.current) setError(String(e));
    } finally {
      fetching.current = false;
    }
  }, []);
  useEffect(() => {
    if (!enabled) return;
    alive.current = true;
    void refresh();
    const timer = setInterval(() => {
      if (!document.hidden && !writing.current) void refresh();
    }, 5000);
    const onVisible = () => {
      if (!document.hidden) void refresh();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      alive.current = false;
      clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [enabled, refresh]);

  async function change(deviceId: string, setting: string, value: string, tx?: number) {
    if (writing.current) return;
    writing.current = true;
    setPending(true);
    setNotice('正在等待设备确认…');
    try {
      await invoke('set_personal_microphone', { deviceId, setting, value, tx: tx ?? null });
      if (alive.current) setNotice('设置已由设备确认');
    } catch (e) {
      if (alive.current) setNotice(`设置失败：${String(e)}`);
    } finally {
      writing.current = false;
      if (alive.current) {
        setPending(false);
        void refresh();
      }
    }
  }
  if (!enabled) return null;
  const remote = data?.remote;
  const mic = data?.microphones.find((m) => m.connected) ?? data?.microphones[0];
  const status = mic?.status;
  const connected = !!mic?.connected && !error;
  const remoteConnected = remote?.connected;
  return (
    <section className="ol-personal-devices" aria-label="我的设备">
      <article className="ol-device-card">
        <div className="ol-device-heading">
          <div>
            <span className="ol-device-kicker">蓝牙遥控器</span>
            <h3>IINE_keyboard</h3>
          </div>
          <span className="ol-device-status" data-connected={remoteConnected === true && !error}>
            {error
              ? '读取失败'
              : remoteConnected === true
                ? '已连接'
                : remoteConnected === false
                  ? '未连接'
                  : data
                    ? '连接状态未知'
                    : '读取中'}
          </span>
        </div>
        <div className="ol-device-charge">
          <strong>{!error && remote?.battery != null ? `${remote.battery}%` : '—'}</strong>
          <span>{remoteConnected ? '系统报告电量' : '上次系统电量'}</span>
        </div>
        <div
          className="ol-device-battery-track"
          data-charge={
            error || remote?.battery == null
              ? 'unknown'
              : remote.battery > 60
                ? 'high'
                : remote.battery > 40
                  ? 'medium'
                  : remote.battery > 20
                    ? 'low'
                    : 'critical'
          }
        >
          <div style={{ width: `${!error ? (remote?.battery ?? 0) : 0}%` }} />
        </div>
        <p className="ol-device-note">通过 Windows 蓝牙读取 · 每 30 秒检查</p>
        {remote?.error && <p className="ol-device-error">{remote.error}</p>}
      </article>

      <article className="ol-device-card">
        <div className="ol-device-heading">
          <div>
            <span className="ol-device-kicker">无线麦克风</span>
            <h3>DJI Mic Mini 2</h3>
          </div>
          <span className="ol-device-status" data-connected={connected}>
            {connected ? '接收器已连接' : data ? '未连接' : '读取中'}
          </span>
        </div>
        {connected && status ? (
          <>
            <div className="ol-device-transmitters">
              {status.tx.map((tx, index) => (
                <div key={index} className="ol-device-tx">
                  <div className="ol-device-tx-line">
                    <span>发射器 {index + 1}</span>
                    <strong>{tx ? djiBatteryLabel(tx.battery) : '未连接'}</strong>
                    <span>{tx?.charging ? '充电中' : ''}</span>
                  </div>
                  <div
                    className="ol-device-gauge"
                    aria-label={
                      tx ? `发射器 ${index + 1}：${djiBatteryLabel(tx.battery)}` : '未连接'
                    }
                    data-charge={
                      tx?.battery == null || tx.battery < 1 || tx.battery > 7
                        ? 'unknown'
                        : tx.battery <= 3
                          ? 'high'
                          : tx.battery === 4
                            ? 'medium'
                            : tx.battery === 5
                              ? 'low'
                              : 'critical'
                    }
                  >
                    {Array.from({ length: 7 }, (_, i) => (
                      <i
                        key={i}
                        data-filled={
                          tx?.battery != null &&
                          tx.battery >= 1 &&
                          tx.battery <= 7 &&
                          i < 8 - tx.battery
                        }
                      />
                    ))}
                  </div>
                </div>
              ))}
            </div>
            <p className="ol-device-note">
              电量格数表示档位，不等于容量百分比 · 接收器 USB 供电
              {status.gain_dial != null
                ? ` · 增益 ${status.gain_dial > 0 ? '+' : ''}${status.gain_dial} dB`
                : ''}
            </p>
            <details className="ol-device-settings">
              <summary>麦克风设置与设备信息</summary>
              <div className="ol-device-settings-grid">
                {Object.keys(OPTIONS)
                  .filter((id) => id !== 'voice-tone')
                  .map((setting) => (
                    <SettingSelect
                      key={setting}
                      setting={setting}
                      value={status.settings[setting]}
                      disabled={
                        pending ||
                        !connected ||
                        (setting === 'stereo' && status.settings['safety-track'] === 'on')
                      }
                      onChange={(value) => void change(mic!.id, setting, value)}
                    />
                  ))}
                {status.tx.map((tx, index) =>
                  tx?.product_name?.includes('Mini 2') ? (
                    <SettingSelect
                      key={`tone-${index}`}
                      setting="voice-tone"
                      label={`发射器 ${index + 1} 音色`}
                      value={tx.voice_tone}
                      disabled={pending || !connected}
                      onChange={(value) => void change(mic!.id, 'voice-tone', value, index)}
                    />
                  ) : null,
                )}
              </div>
              {status.settings['safety-track'] === 'on' && (
                <p className="ol-device-note">安全音轨已开启，声道切换暂不可用。</p>
              )}
              <p className="ol-device-note">
                接收器固件 {status.rx?.firmware ?? '读取中'}
                {status.tx
                  .map((tx, i) =>
                    tx ? ` · TX${i + 1} ${tx.product_name ?? ''} ${tx.firmware ?? ''}` : '',
                  )
                  .join('')}
              </p>
            </details>
          </>
        ) : (
          <p className="ol-device-note">
            {data?.probe.present
              ? '已发现接收器，暂未收到状态。请退出 DJI Mic Control，避免两个程序占用控制接口。'
              : '请连接 DJI USB 接收器并开启麦克风。'}
          </p>
        )}
        {notice && (
          <p role="status" className="ol-device-note">
            {notice}
          </p>
        )}
      </article>
      {error && (
        <p role="alert" className="ol-device-error">
          设备信息读取失败：{error}
        </p>
      )}
    </section>
  );
}
