import React, { useEffect, useId, useMemo, useRef, useState } from 'react';
import { NOTIF_BLUE } from '../bot/decor';
import { BotEngine, type BotFrame } from '../bot/engine';
import { DEMI_VIEWBOX, RAYON } from '../bot/repere';
import { mixHex } from '../bot/skins';
import { STATE_BY_ID, type StateId } from '../bot/states';
import { motionFromMood } from '../messages/motion';
import { deskLookTarget, pointerOffset } from './puffGaze';
import type { CharacterProps, CompanionMotion } from './types';

/** Cream body — inverse of bloub's ink so the figure reads on dark and light wallpaper. */
const BODY = '#f4f1ea';
/** Dark paper behind eye-mask holes so pupils stay visible on a cream body. */
const PAPER = '#1a1a1e';

const MOTION_STATE: Record<CompanionMotion, StateId> = {
  idle: 'idle',
  thinking: 'thinking',
  wink: 'wink',
  wide: 'wide',
  alert: 'alert',
  notify: 'notify',
  exclaim: 'exclaim',
  sleep: 'sleep',
  play: 'play',
  orbit: 'orbit',
  burst: 'burst',
  comet: 'comet',
};

const Puff: React.FC<CharacterProps> = ({ mood, activity, size = 150, motion }) => {
  const resolved = motion ?? motionFromMood(mood, activity);
  const motionRef = useRef(resolved);
  motionRef.current = resolved;
  const uid = useId().replace(/:/g, '');
  const svgRef = useRef<SVGSVGElement | null>(null);
  const pointerRef = useRef<{ x: number; y: number } | null>(null);
  const engineRef = useRef<BotEngine | null>(null);
  if (!engineRef.current) engineRef.current = new BotEngine(RAYON, MOTION_STATE[resolved]);
  const [frame, setFrame] = useState<BotFrame>(() => engineRef.current!.sample(0));

  useEffect(() => {
    const engine = engineRef.current!;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      setFrame(engine.sample(0));
      return;
    }
    let raf = 0;
    let last = 0;
    let clock = 0;
    let aiming = false;

    const onMove = (event: PointerEvent) => {
      if (event.pointerType === 'touch') return;
      pointerRef.current = { x: event.clientX, y: event.clientY };
    };
    const onLeave = () => {
      pointerRef.current = null;
    };
    window.addEventListener('pointermove', onMove);
    document.addEventListener('pointerleave', onLeave);

    const tick = (ms: number) => {
      raf = requestAnimationFrame(tick);
      const dt = last ? Math.min((ms - last) / 1000, 0.064) : 0;
      last = ms;
      clock += dt;
      const want = MOTION_STATE[motionRef.current];
      if (engine.state !== want) engine.setState(want, clock);
      const face = STATE_BY_ID.get(engine.state)?.baseFace;
      const box = svgRef.current?.getBoundingClientRect();
      const pointer = pointerRef.current;
      if (face && box && box.width > 0 && box.height > 0) {
        const { nx, ny } = pointer
          ? pointerOffset(box, pointer.x, pointer.y)
          : { nx: 0, ny: 0 };
        engine.setLook(deskLookTarget(nx, ny, pointer !== null), clock);
        aiming = true;
      } else if (aiming) {
        engine.setLook(null, clock);
        aiming = false;
      }
      setFrame(engine.sample(clock));
    };
    raf = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener('pointermove', onMove);
      document.removeEventListener('pointerleave', onLeave);
    };
  }, []);

  const vb = DEMI_VIEWBOX;
  const maskId = `puff-mask-${uid}`;
  const dots = useMemo(() => {
    return frame.dots.map((dot, i) => {
      const fill = dot.color ?? (dot.depth === undefined ? BODY : mixHex(PAPER, BODY, dot.depth));
      const key = frame.dotsBehind ? `pb${i}` : `pf${i}`;
      if (dot.d) {
        return (
          <path
            key={key}
            d={dot.d}
            transform={`translate(${dot.x} ${dot.y}) rotate(${dot.rot ?? 0}) scale(${RAYON})`}
            fill={fill}
            opacity={dot.opacity}
          />
        );
      }
      return <circle key={key} cx={dot.x} cy={dot.y} r={dot.r} fill={fill} opacity={dot.opacity} />;
    });
  }, [frame.dots, frame.dotsBehind]);

  return (
    <div className='nomi-ch nomi-puff' style={{ width: size, height: size }}>
      <svg
        ref={svgRef}
        width={size}
        height={size}
        viewBox={`${-vb} ${-vb} ${vb * 2} ${vb * 2}`}
        role='img'
        aria-hidden='true'
      >
        <defs>
          <mask id={maskId} maskUnits='userSpaceOnUse' x={-vb} y={-vb} width={vb * 2} height={vb * 2}>
            <path d={frame.bodyPath} fill='#fff' />
            {frame.eyes.map((eye, i) => (
              <path key={i} d={eye.d} transform={eye.matrix} opacity={eye.alpha} fill='#000' />
            ))}
            {frame.notch ? <circle cx={frame.notch.x} cy={frame.notch.y} r={frame.notch.r} fill='#000' /> : null}
          </mask>
          {frame.arcs.map((arc) => (
            <linearGradient
              key={arc.id}
              id={`${uid}-${arc.id}`}
              gradientUnits='userSpaceOnUse'
              x1={arc.grad.x1}
              y1={arc.grad.y1}
              x2={arc.grad.x2}
              y2={arc.grad.y2}
            >
              {arc.grad.stops.map((color, i) => (
                <stop
                  key={i}
                  offset={arc.grad.stops.length <= 1 ? 0 : i / (arc.grad.stops.length - 1)}
                  stopColor={color}
                />
              ))}
            </linearGradient>
          ))}
        </defs>
        <g fill='none' strokeLinecap='round'>
          {frame.arcs.map((arc) => (
            <path
              key={`b${arc.id}`}
              d={arc.back}
              stroke={`url(#${uid}-${arc.id})`}
              strokeWidth={arc.width}
              opacity={arc.opacity}
            />
          ))}
        </g>
        {frame.dotsBehind ? <g>{dots}</g> : null}
        <g opacity={frame.bodyAlpha}>
          <path d={frame.bodyPath} fill={PAPER} />
          <g mask={`url(#${maskId})`}>
            <rect x={-vb} y={-vb} width={vb * 2} height={vb * 2} fill={BODY} />
          </g>
        </g>
        {!frame.dotsBehind ? <g>{dots}</g> : null}
        {frame.notif ? (
          <circle cx={frame.notif.x} cy={frame.notif.y} r={frame.notif.r} fill={NOTIF_BLUE} />
        ) : null}
        <g fill='none' strokeLinecap='round'>
          {frame.arcs.map((arc) => (
            <path
              key={`f${arc.id}`}
              d={arc.front}
              stroke={`url(#${uid}-${arc.id})`}
              strokeWidth={arc.width}
              opacity={arc.opacity}
            />
          ))}
        </g>
      </svg>
    </div>
  );
};

export default Puff;
