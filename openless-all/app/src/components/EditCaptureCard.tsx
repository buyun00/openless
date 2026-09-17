import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { detectOS } from './WindowChrome';
import { isTauri } from '../lib/ipc';
import { Icon } from './Icon';
import { Btn, Card } from '../pages/_atoms';

interface CaptureState {
  enabled: boolean;
  supported: boolean;
  status: string;
  app: string;
  records: { session: number; app: string; before: string; after: string }[];
  learning: {
    source: string;
    target: string;
    count: number;
    status: string;
    dictionary_id: string | null;
    blocked: boolean;
    learned_at?: string | null;
  }[];
}
export function EditCaptureCard() {
  const [state, setState] = useState<CaptureState | null>(null);
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  const [showAllWords, setShowAllWords] = useState(false);
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
  const latest = state?.records[0];
  const words = (state?.learning ?? [])
    .filter((word) => word.dictionary_id)
    .slice()
    .sort((a, b) => (b.learned_at ?? '').localeCompare(a.learned_at ?? ''))
    .filter((word, index, all) => all.findIndex((other) => other.target === word.target) === index);
  const visibleWords = showAllWords ? words : words.slice(0, 6);
  async function undo(word: CaptureState['learning'][number]) {
    setPending(true);
    try {
      setState(
        await invoke<CaptureState>('undo_edit_learning', {
          source: word.source,
          target: word.target,
        }),
      );
      setError('');
    } catch (e) {
      setError(String(e));
    } finally {
      setPending(false);
    }
  }
  return (
    <Card padding={18} style={{ minWidth: 0 }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
          marginBottom: 12,
        }}
      >
        <strong style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink-2)' }}>
          自动纠词
        </strong>
        <details style={{ fontSize: 12, color: 'var(--ol-ink-3)', maxWidth: '100%' }}>
          <summary
            aria-label="捕获管理"
            title="捕获管理"
            style={{ cursor: 'pointer', listStyle: 'none', textAlign: 'right' }}
          >
            <Icon name="more" size={16} />
          </summary>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              flexWrap: 'wrap',
              gap: 8,
              paddingTop: 8,
            }}
          >
            <span>{state?.status || '加载中…'}</span>
            <Btn
              size="sm"
              variant="ghost"
              disabled={pending || !state}
              onClick={() => void configure(!state?.enabled)}
            >
              {state?.enabled ? '暂停捕获' : '开启捕获'}
            </Btn>
            <Btn
              size="sm"
              variant="ghost"
              disabled={pending || !state?.records.length}
              onClick={() => void configure(!!state?.enabled, true)}
            >
              清空修改记录
            </Btn>
          </div>
        </details>
      </div>
      {error && (
        <p role="alert" style={{ fontSize: 12, color: 'var(--ol-ink-2)' }}>
          {error}
        </p>
      )}
      {!state?.enabled && state && (
        <p style={{ fontSize: 12, color: 'var(--ol-ink-3)', margin: '0 0 12px' }}>
          捕获已暂停，可在右上角菜单中开启。
        </p>
      )}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(min(260px, 100%), 1fr))',
          gap: 14,
        }}
      >
        {[
          { label: '修改前', text: latest?.before, accent: false },
          { label: '修改后', text: latest?.after, accent: true },
        ].map(({ label, text, accent }) => (
          <section
            key={label}
            aria-label={label}
            style={{
              minWidth: 0,
              padding: '14px 16px',
              borderRadius: 10,
              background: accent
                ? 'color-mix(in srgb, var(--ol-blue) 5%, transparent)'
                : 'color-mix(in srgb, var(--ol-ink) 3%, transparent)',
            }}
          >
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                fontSize: 11,
                color: accent ? 'var(--ol-blue)' : 'var(--ol-ink-3)',
              }}
            >
              <Icon name={accent ? 'pencil' : 'doc'} size={13} />
              {label}
            </div>
            <p
              style={{
                margin: '10px 0 0',
                minHeight: 44,
                whiteSpace: 'pre-wrap',
                overflowWrap: 'anywhere',
                userSelect: 'text',
                fontSize: 14,
                lineHeight: 1.75,
                color: text ? 'var(--ol-ink-2)' : 'var(--ol-ink-3)',
              }}
            >
              {text ?? (state ? '暂无修改记录' : '加载中…')}
            </p>
          </section>
        ))}
      </div>
      <section
        aria-label="最近入库的新词"
        style={{ marginTop: 18, paddingTop: 14, borderTop: '1px solid var(--ol-line)' }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            marginBottom: 10,
          }}
        >
          <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
            最近入库{words.length > 0 ? ` · ${words.length}` : ''}
          </span>
          {words.length > 6 && (
            <Btn size="sm" variant="ghost" onClick={() => setShowAllWords(!showAllWords)}>
              {showAllWords ? '收起' : '查看全部'}
            </Btn>
          )}
        </div>
        <div aria-live="polite" style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
          {visibleWords.map((word) => (
            <div
              key={word.dictionary_id}
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 8,
                maxWidth: '100%',
                padding: '7px 10px',
                borderRadius: 8,
                border: '1px solid var(--ol-line)',
                fontSize: 12,
              }}
            >
              <Icon name="check" size={13} style={{ color: 'var(--ol-blue)', flexShrink: 0 }} />
              <span style={{ color: 'var(--ol-ink-3)', overflowWrap: 'anywhere' }}>
                {word.source}
              </span>
              <span aria-hidden="true" style={{ color: 'var(--ol-ink-3)' }}>
                →
              </span>
              <strong style={{ color: 'var(--ol-ink)', overflowWrap: 'anywhere' }}>
                {word.target}
              </strong>
              <button
                type="button"
                aria-label={`撤销入库：${word.target}`}
                title="撤销入库，并停止学习此纠词"
                disabled={pending}
                onClick={() => void undo(word)}
                style={{
                  display: 'flex',
                  background: 'transparent',
                  border: 0,
                  padding: 2,
                  color: 'var(--ol-ink-3)',
                  cursor: pending ? 'wait' : 'pointer',
                }}
              >
                <Icon name="x" size={12} />
              </button>
            </div>
          ))}
          {!words.length && (
            <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
              暂无新词，自动入库后会显示在这里。
            </span>
          )}
        </div>
      </section>
    </Card>
  );
}
