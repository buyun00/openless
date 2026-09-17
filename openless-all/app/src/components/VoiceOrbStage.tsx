import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { SiriGL } from './SiriGL';
import { getCapsulePillMetrics } from '../lib/capsuleLayout';
import type { CapsuleState } from '../lib/types';
import type { OS } from './WindowChrome';
import './CapsuleStyles.css';

export interface VoiceOrbStageProps {
  os: OS;
  state: CapsuleState;
  level: number;
  /** 预备态：录音光条渲染成「待命」呼吸形态，不接真实电平。见 CapsulePayload.warming。 */
  warming?: boolean;
  /** 预备→就绪的平均耗时（ms），驱动展开动画的预测节奏。见 SiriGL warmupMs。 */
  warmupMs?: number;
  message?: string;
}

/**
 * 带深色圆角外壳的光效舞台：
 *   - recording：彩虹光谱声波横贯舞台，振幅随真实麦克风电平起伏；
 *   - transcribing / polishing：波形收拢后，由单色呼吸圆点表示处理中；
 *   - done / cancelled：立即卸载光效，不保留收尾圆点；
 *   - error：冻结光效 + 浮一行发光红字说明原因（唯一保留的文字信息）。
 * 外壳与细边框让光效在浅色、深色桌面背景上都有清晰边界。
 */
export function VoiceOrbStage({
  os,
  state,
  level,
  warming,
  warmupMs,
  message,
}: VoiceOrbStageProps) {
  const { t } = useTranslation();
  const metrics = useMemo(() => getCapsulePillMetrics(os), [os]);

  // done / cancelled / error 冻结最后形态淡出，不再切换 phase。
  const lastPhaseRef = useRef<'wave' | 'orb'>('wave');
  let phase = lastPhaseRef.current;
  if (state === 'recording') phase = 'wave';
  else if (state === 'transcribing' || state === 'polishing') phase = 'orb';
  lastPhaseRef.current = phase;
  const isOrb = phase === 'orb';

  // 性能：波形淡出彻底结束（.55s delay + .6s duration）后卸载它的绘制循环 ——
  // 思考期间不再为一块不可见的 canvas 每帧跑 fragment。回到录音态立即重挂
  //（shader 编译已被驱动缓存，重建近零耗时）。
  const [waveAlive, setWaveAlive] = useState(true);
  useEffect(() => {
    if (!isOrb) {
      setWaveAlive(true);
      return undefined;
    }
    const timer = setTimeout(() => setWaveAlive(false), 1300);
    return () => clearTimeout(timer);
  }, [isOrb]);

  // Completion is already visible in the inserted text; do not linger on a final dot.
  if (state === 'done' || state === 'cancelled' || state === 'idle') return null;

  return (
    <div
      style={{
        width: metrics.width,
        height: metrics.height,
        boxSizing: metrics.boxSizing,
        fontFamily: 'var(--ol-font-sans)',
        position: 'relative',
        pointerEvents: 'none',
        // Keep the transparent host and its positioning stable; halve the visible stage.
        transform: 'scale(0.5)',
        transformOrigin: 'center',
      }}
    >
      <div
        aria-hidden="true"
        style={{
          position: 'absolute',
          left: '50%',
          top: '50%',
          transform: 'translate(-50%, -50%)',
          boxSizing: 'border-box',
          width: isOrb ? 20 : 230,
          height: isOrb ? 20 : 64,
          borderRadius: 64,
          background: 'rgba(22, 28, 40, 0.92)',
          border: '2px solid rgba(150, 174, 210, 0.45)',
          boxShadow: '0 8px 24px rgba(0, 0, 0, 0.22), inset 0 2px 0 rgba(255, 255, 255, 0.05)',
          opacity: isOrb ? 0 : 1,
          transition: 'width .35s ease, height .35s ease, opacity .25s ease .15s',
        }}
      />
      {waveAlive && (
        <div
          style={{
            position: 'absolute',
            inset: 0,
            // Clip to the shell's inner edge, including while it contracts into a dot.
            clipPath: `inset(${(metrics.height - (isOrb ? 16 : 60)) / 2}px ${(metrics.width - (isOrb ? 16 : 226)) / 2}px round 32px)`,
            transition: 'clip-path .35s ease',
          }}
        >
          <SiriGL
            mode="wave"
            colorful
            level={level}
            resolved={!isOrb}
            warming={warming}
            warmupMs={warmupMs}
            style={{
              position: 'absolute',
              inset: 0,
              width: '100%',
              height: '100%',
              // Narrow the waveform to fit, while retaining most of its vertical motion.
              transform: 'scale(0.55, 0.85)',
              transformOrigin: 'center',
              // 收缩汇聚进行时波形保持可见，收成中央光点后再淡出，与圆点环的淡入交叠。
              opacity: isOrb ? 0 : 1,
              transition: isOrb ? 'opacity .6s ease-out .55s' : 'opacity .25s ease-out',
            }}
          />
        </div>
      )}
      {isOrb && (
        <div
          style={{
            position: 'absolute',
            left: '50%',
            top: '50%',
            width: 20,
            height: 20,
            marginLeft: -10,
            marginTop: -10,
            animation: 'siri-orb-in .3s ease-out .55s both',
          }}
        >
          <span className="ol-siri-processing-dot" />
        </div>
      )}
      {state === 'error' && <span style={errorGlowTextStyle}>{message || t('capsule.error')}</span>}
    </div>
  );
}

const errorGlowTextStyle: CSSProperties = {
  position: 'absolute',
  bottom: 24,
  left: '50%',
  transform: 'translateX(-50%)',
  maxWidth: 400,
  fontSize: 12,
  fontWeight: 600,
  lineHeight: 1.4,
  textAlign: 'center',
  color: 'var(--ol-err)',
  padding: '6px 12px',
  background: 'var(--ol-capsule-pill-bg)',
  border: '1px solid var(--ol-capsule-pill-border)',
  borderRadius: 12,
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
};
