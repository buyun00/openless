import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { detectOS } from './WindowChrome';
import { isTauri } from '../lib/ipc';
import { Btn, Card } from '../pages/_atoms';

interface CaptureState {
  enabled: boolean;
  supported: boolean;
  status: string;
  app: string;
  records: { session: number; app: string; before: string; after: string }[];
}
export function EditCaptureCard() {
  const [state, setState] = useState<CaptureState | null>(null);
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  const enabled = isTauri && detectOS() === 'win';
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    let busy = false;
    const refresh = async () => {
      if (busy) return;
      busy = true;
      try {
        const next = await invoke<CaptureState>('get_edit_capture');
        if (alive) {
          setState(next);
          setError('');
        }
      } catch (e) {
        if (alive) setError(String(e));
      } finally {
        busy = false;
      }
    };
    void refresh();
    const timer = setInterval(() => {
      if (!document.hidden) void refresh();
    }, 1000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [enabled]);
  async function configure(on: boolean, clear = false) {
    setPending(true);
    try {
      setState(await invoke<CaptureState>('configure_edit_capture', { enabled: on, clear }));
      setError('');
    } catch (e) {
      setError(String(e));
    } finally {
      setPending(false);
    }
  }
  if (!enabled) return null;
  return (
    <Card
      padding={16}
      style={{
        display: 'flex',
        flexDirection: 'column',
        minWidth: 0,
        height: 280,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          flexWrap: 'wrap',
          gap: 10,
          flexShrink: 0,
        }}
      >
        <strong style={{ fontSize: 14 }}>修改捕获记录 · {state?.records.length ?? 0}</strong>
        <div style={{ display: 'flex', gap: 8 }}>
          <Btn
            size="sm"
            disabled={pending || !state}
            onClick={() => void configure(!state?.enabled)}
          >
            {state?.enabled ? '停止捕获' : '开启捕获'}
          </Btn>
          <Btn
            size="sm"
            variant="ghost"
            disabled={pending || !state?.records.length}
            onClick={() => void configure(!!state?.enabled, true)}
          >
            清空记录
          </Btn>
        </div>
      </div>
      <details style={{ fontSize: 12, color: 'var(--ol-ink-3)', marginTop: 10, flexShrink: 0 }}>
        <summary style={{ cursor: 'pointer' }}>捕获说明 · 仅记录，不自动学习</summary>
        <p style={{ lineHeight: 1.7, margin: '8px 0 0' }}>
          开启后，在同一输入框修改听写文字，停手至少 3 秒再返回查看。 每次观察最多 60
          秒，切换窗口或输入框会停止。本次运行保留最近 20 条，重启后清空。
        </p>
      </details>
      <p role="status" style={{ fontSize: 12, color: 'var(--ol-ink-2)' }}>
        {error || state?.status || '加载中…'}
        {state?.app ? ` · ${state.app}` : ''}
      </p>
      <div
        className="ol-noscrollbar"
        role="region"
        aria-label="修改捕获记录列表"
        tabIndex={0}
        style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}
      >
        {!state?.records.length && (
          <p style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
            尚无修改记录。开启捕获后，在 QQ 或 ChatGPT 中修改听写文字，记录会自动出现在这里。
          </p>
        )}
        {state?.records.map((record) => (
          <div
            key={record.session}
            style={{
              borderTop: '1px solid var(--ol-line)',
              paddingTop: 10,
              marginTop: 10,
              fontSize: 12,
            }}
          >
            <strong>{record.app}</strong>
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(min(240px, 100%), 1fr))',
                gap: 12,
                marginTop: 8,
              }}
            >
              <div>
                <span style={{ color: 'var(--ol-ink-3)' }}>听写原文</span>
                <p style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', userSelect: 'text' }}>
                  {record.before}
                </p>
              </div>
              <div>
                <span style={{ color: 'var(--ol-ink-3)' }}>手工修改后</span>
                <p style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', userSelect: 'text' }}>
                  {record.after}
                </p>
              </div>
            </div>
          </div>
        ))}
      </div>
    </Card>
  );
}
