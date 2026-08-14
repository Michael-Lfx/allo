/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import {
  BLUEPRINT_CURSOR_END,
  BLUEPRINT_SUPPORT_LINES,
  BLUEPRINT_AMBIENT_MARKS,
  BLUEPRINT_FRAGMENTS,
  BLUEPRINT_MARKS,
  BLUEPRINT_ROUTE_SEGMENTS,
  BLUEPRINT_VERIFY_PATH,
  BLUEPRINT_VIEWBOX,
  getBlueprintArrivedFragmentIds,
  getBlueprintFocusIds,
  getBlueprintRouteStep,
} from './blueprintScene';
import type { BlueprintAmbientMark, BlueprintDetail, BlueprintFragment } from './blueprintScene';
import type {
  BlueprintRouteStep,
  CloudActivationLevel,
  IntentFieldMode,
  IntentFieldPhase,
} from './authTypes';

type PointerState = {
  x: number;
  y: number;
  focusIds: readonly string[];
} | null;

type BlueprintStyle = CSSProperties & {
  '--blueprint-shift-x'?: string;
  '--blueprint-shift-y'?: string;
};

export interface IntentFieldProps {
  mode: IntentFieldMode;
  phase: IntentFieldPhase;
  activationLevel?: CloudActivationLevel;
  inputEnergy?: number;
  blueprintStep?: BlueprintRouteStep;
  children?: ReactNode;
  className?: string;
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

const isBrandSafeZone = (x: number, y: number) => (
  (x < 0.34 && y < 0.27) || (x < 0.44 && y > 0.74)
);

const renderFragmentCore = (fragment: BlueprintFragment, isSettled: boolean) => {
  const { variant, x, y, width, height } = fragment;
  const innerX = x + 12;
  const innerY = y + 14;
  const innerWidth = Math.max(width - 24, 24);
  const innerHeight = Math.max(height - 28, 20);
  const topY = y + 17;

  if (fragment.role === 'companion') {
    if (variant === 'terminal-rows') {
      const quietLineY = Math.min(innerY + 29, y + height - 9);
      return (
        <>
          <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
          <path className='flowy-blueprint__fragment-line' d={`M ${x + 12} ${topY} H ${x + width - 12}`} />
          <circle className='flowy-blueprint__fragment-dot' cx={x + 14} cy={y + 10} r='2' />
          <circle className='flowy-blueprint__fragment-dot' cx={x + 22} cy={y + 10} r='2' />
          <path className='flowy-blueprint__fragment-accent' d={`M ${innerX} ${innerY + 9} H ${innerX + innerWidth * 0.58}`} />
          <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${quietLineY} H ${innerX + innerWidth * 0.72}`} />
        </>
      );
    }

    if (variant === 'diff-block') {
      return (
        <>
          <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
          <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${y + 17} H ${innerX + innerWidth * 0.72} M ${innerX} ${y + 31} H ${innerX + innerWidth * 0.48}`} />
        </>
      );
    }

    if (variant === 'table-grid') {
      return (
        <>
          <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
          <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${innerY + 8} H ${innerX + innerWidth} M ${innerX} ${innerY + 27} H ${innerX + innerWidth * 0.72}`} />
        </>
      );
    }

    if (variant === 'summary-bars') {
      return (
        <>
          <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
          <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${innerY + 8} H ${innerX + innerWidth * 0.78} M ${innerX} ${innerY + 24} H ${innerX + innerWidth * 0.54}`} />
        </>
      );
    }
  }

  if (variant === 'command-lines') {
    return (
      <>
        <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
        <circle className='flowy-blueprint__fragment-dot' cx={x + 12} cy={y + 12} r='2' />
        <circle className='flowy-blueprint__fragment-dot' cx={x + 20} cy={y + 12} r='2' />
        <path className='flowy-blueprint__fragment-accent' d={`M ${innerX} ${innerY + 3} H ${innerX + innerWidth * 0.34}`} />
        <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${innerY + 18} H ${innerX + innerWidth * 0.76} M ${innerX} ${innerY + 30} H ${innerX + innerWidth * 0.48}`} />
      </>
    );
  }

  if (variant === 'file-sheet') {
    const fold = Math.min(18, width * 0.18);
    return (
      <>
        <path className='flowy-blueprint__fragment-shell' d={`M ${x + 1} ${y + 1} H ${x + width - fold} L ${x + width - 1} ${y + fold} V ${y + height - 1} H ${x + 1} Z`} />
        <path className='flowy-blueprint__fragment-line' d={`M ${x + width - fold} ${y + 1} V ${y + fold} H ${x + width - 1}`} />
        <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${innerY + 4} H ${innerX + innerWidth * 0.62} M ${innerX} ${innerY + 17} H ${innerX + innerWidth * 0.82} M ${innerX} ${innerY + 30} H ${innerX + innerWidth * 0.42}`} />
      </>
    );
  }

  if (variant === 'terminal-rows') {
    const rowOne = Math.min(innerY + 17, y + height - 13);
    const rowTwo = Math.min(innerY + 31, y + height - 8);
    const rowLast = Math.min(innerY + 44, y + height - 4);
    return (
      <>
        <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
        <path className='flowy-blueprint__fragment-line' d={`M ${x + 12} ${topY} H ${x + width - 12}`} />
        <circle className='flowy-blueprint__fragment-dot' cx={x + 14} cy={y + 10} r='2' />
        <circle className='flowy-blueprint__fragment-dot' cx={x + 22} cy={y + 10} r='2' />
        <path className='flowy-blueprint__fragment-accent' d={`M ${innerX} ${rowOne} H ${innerX + innerWidth * 0.62}`} />
        <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${rowTwo} H ${innerX + innerWidth * 0.78} M ${innerX} ${rowLast} H ${innerX + innerWidth * 0.48}`} />
      </>
    );
  }

  if (variant === 'browser-pane') {
    const resultRow = Math.min(innerY + 39, y + height - 9);
    return (
      <>
        <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
        <path className='flowy-blueprint__fragment-line' d={`M ${x + 12} ${topY} H ${x + width - 12}`} />
        <circle className='flowy-blueprint__fragment-dot' cx={x + 14} cy={y + 10} r='2' />
        <circle className='flowy-blueprint__fragment-dot' cx={x + 22} cy={y + 10} r='2' />
        <path className='flowy-blueprint__fragment-accent' d={`M ${innerX} ${innerY + 11} H ${innerX + innerWidth * 0.52} M ${innerX} ${innerY + 24} H ${innerX + innerWidth * 0.74}`} />
        <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${resultRow} H ${innerX + innerWidth * 0.48}`} />
      </>
    );
  }

  if (variant === 'diff-block') {
    const diffRowOne = y + 16;
    const diffRowTwo = Math.min(y + 31, y + height - 20);
    const diffAccentRow = Math.min(y + 43, y + height - 11);
    const diffRowFour = y + height - 7;
    return (
      <>
        <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
        <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${diffRowOne} H ${innerX + innerWidth * 0.76} M ${innerX} ${diffRowTwo} H ${innerX + innerWidth * 0.58}`} />
        <path className='flowy-blueprint__fragment-accent' d={`M ${innerX} ${diffAccentRow} H ${innerX + innerWidth * 0.82}`} />
        <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${diffRowFour} H ${innerX + innerWidth * 0.4}`} />
      </>
    );
  }

  if (variant === 'table-grid') {
    const tableTop = innerY + 7;
    const tableMiddle = Math.min(innerY + 24, y + height - 15);
    const tableBottom = Math.min(innerY + 39, y + height - 8);
    return (
      <>
        <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
        <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${tableTop} H ${innerX + innerWidth}`} />
        <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${tableMiddle} H ${innerX + innerWidth} M ${innerX} ${tableBottom} H ${innerX + innerWidth}`} />
      </>
    );
  }

  if (variant === 'summary-bars') {
    return (
      <>
        <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
        <path className='flowy-blueprint__fragment-line' d={`M ${innerX} ${innerY + 7} H ${innerX + innerWidth * 0.78} M ${innerX} ${innerY + 21} H ${innerX + innerWidth * 0.56} M ${innerX} ${innerY + 35} H ${innerX + innerWidth * 0.82}`} />
        {isSettled && <path className='flowy-blueprint__fragment-accent flowy-blueprint__fragment-complete-mark' d={`M ${innerX + innerWidth * 0.73} ${innerY + innerHeight - 8} L ${innerX + innerWidth * 0.77} ${innerY + innerHeight - 4} L ${innerX + innerWidth * 0.85} ${innerY + innerHeight - 13}`} />}
      </>
    );
  }

  return (
    <>
      <rect className='flowy-blueprint__fragment-shell' x={x} y={y} width={width} height={height} rx='4' />
      <path className='flowy-blueprint__fragment-accent' d={`M ${innerX} ${innerY + 12} L ${innerX + 7} ${innerY + 19} L ${innerX + 18} ${innerY + 7}`} />
      <path className='flowy-blueprint__fragment-line' d={`M ${innerX + 30} ${innerY + 12} H ${innerX + innerWidth * 0.78} M ${innerX + 30} ${innerY + 25} H ${innerX + innerWidth * 0.54} M ${innerX} ${innerY + 42} H ${innerX + innerWidth * 0.84}`} />
    </>
  );
};

const renderBlueprintDetails = (
  fragment: BlueprintFragment,
  displayedStep: BlueprintRouteStep,
  isSettled: boolean,
) => (fragment.details ?? []).map((detail: BlueprintDetail, index) => {
  const isVisible = detail.revealAt === 0 || displayedStep >= detail.revealAt || isSettled;
  const detailClassName = [
    'flowy-blueprint__fragment-detail',
    `is-${detail.kind}`,
    detail.emphasis === 'accent' ? 'is-accent' : 'is-quiet',
    isVisible ? 'is-detail-visible' : '',
  ].filter(Boolean).join(' ');
  const x = fragment.x + detail.x;
  const y = fragment.y + detail.y;
  const width = detail.width ?? 24;

  if (detail.kind === 'dot') {
    return <circle key={`${fragment.id}-detail-${index}`} className={detailClassName} cx={x} cy={y} r='1.8' />;
  }
  if (detail.kind === 'check') {
    return (
      <path
        key={`${fragment.id}-detail-${index}`}
        className={detailClassName}
        d={`M ${x} ${y + 5} L ${x + 4} ${y + 9} L ${x + 12} ${y - 1}`}
      />
    );
  }
  return (
    <path
      key={`${fragment.id}-detail-${index}`}
      className={detailClassName}
      d={`M ${x} ${y} H ${x + width}`}
    />
  );
});

const renderFragmentBody = (
  fragment: BlueprintFragment,
  isSettled: boolean,
  displayedStep: BlueprintRouteStep,
) => (
  <>
    {renderFragmentCore(fragment, isSettled)}
    {renderBlueprintDetails(fragment, displayedStep, isSettled)}
  </>
);

const renderAmbientMark = (mark: BlueprintAmbientMark, isVisible: boolean) => {
  const className = [
    'flowy-blueprint__ambient-mark',
    `is-${mark.kind}`,
    isVisible ? 'is-mark-visible' : '',
  ].filter(Boolean).join(' ');
  const size = mark.size ?? 10;
  return (
    <g key={mark.id} className={className} transform={`translate(${mark.x} ${mark.y})`}>
      <path d={`M ${-size / 2} 0 H ${size / 2} M 0 ${-size / 2} V ${size / 2}`} />
      <circle cx='0' cy='0' r={mark.kind === 'completion' ? '1.8' : '1.2'} />
    </g>
  );
};

const IntentField: React.FC<IntentFieldProps> = ({
  mode,
  phase,
  activationLevel = 0,
  inputEnergy = 0,
  blueprintStep,
  children,
  className,
}) => {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const pointerFrameRef = useRef<number | null>(null);
  const pendingPointerRef = useRef<PointerState>(null);
  const routeTimerRef = useRef<number | null>(null);
  const displayedStepRef = useRef<BlueprintRouteStep>(0);
  const [pointer, setPointer] = useState<PointerState>(null);
  const [reducedMotion, setReducedMotion] = useState(false);
  const [finePointer, setFinePointer] = useState(false);
  const [compactViewport, setCompactViewport] = useState(false);
  const [pageHidden, setPageHidden] = useState(false);
  const [displayedStep, setDisplayedStep] = useState<BlueprintRouteStep>(0);
  const [executionCursorState, setExecutionCursorState] = useState<'hidden' | 'play' | 'done'>('hidden');
  const executionCursorPhaseRef = useRef<IntentFieldPhase | null>(null);

  const safeActivationLevel = clamp(activationLevel, 0, 6) as CloudActivationLevel;
  const safeInputEnergy = clamp(inputEnergy, 0, 1);
  const targetStep = getBlueprintRouteStep(mode, phase, safeInputEnergy, blueprintStep);
  const isEmailCapturePreview = mode === 'cloud'
    && phase === 'input'
    && blueprintStep === undefined
    && targetStep === 1;
  const inputRouteProgress = isEmailCapturePreview ? safeInputEnergy : 1;
  const arrivedFragmentIds = useMemo(
    () => getBlueprintArrivedFragmentIds(displayedStep),
    [displayedStep]
  );

  useEffect(() => {
    const motionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
    const pointerQuery = window.matchMedia('(min-width: 900px) and (hover: hover) and (pointer: fine)');
    const compactQuery = window.matchMedia('(max-width: 639px)');
    const updateMotion = (event: MediaQueryListEvent | MediaQueryList) => {
      const matches = 'matches' in event ? event.matches : false;
      setReducedMotion(matches);
      if (matches) setPointer(null);
    };
    const updatePointer = (event: MediaQueryListEvent | MediaQueryList) => {
      const matches = 'matches' in event ? event.matches : false;
      setFinePointer(matches);
      if (!matches) setPointer(null);
    };
    const updateCompactViewport = (event: MediaQueryListEvent | MediaQueryList) => {
      const matches = 'matches' in event ? event.matches : false;
      setCompactViewport(matches);
      if (matches) setPointer(null);
    };

    updateMotion(motionQuery);
    updatePointer(pointerQuery);
    updateCompactViewport(compactQuery);
    motionQuery.addEventListener?.('change', updateMotion);
    pointerQuery.addEventListener?.('change', updatePointer);
    compactQuery.addEventListener?.('change', updateCompactViewport);

    return () => {
      motionQuery.removeEventListener?.('change', updateMotion);
      pointerQuery.removeEventListener?.('change', updatePointer);
      compactQuery.removeEventListener?.('change', updateCompactViewport);
    };
  }, []);

  // The blueprint is intentionally static on touch/coarse-pointer devices.
  // This also prevents a late media-query update from replaying route or
  // cursor motion after the page has already rendered its semantic state.
  const motionDisabled = reducedMotion || compactViewport || !finePointer;

  useEffect(() => {
    const onVisibilityChange = () => {
      const hidden = document.visibilityState === 'hidden';
      setPageHidden(hidden);
      if (hidden) {
        pendingPointerRef.current = null;
        setPointer(null);
      }
    };
    onVisibilityChange();
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => document.removeEventListener('visibilitychange', onVisibilityChange);
  }, []);

  useEffect(() => {
    if (routeTimerRef.current !== null) {
      window.clearTimeout(routeTimerRef.current);
      routeTimerRef.current = null;
    }

    if (motionDisabled || pageHidden || phase === 'success') {
      displayedStepRef.current = targetStep;
      setDisplayedStep(targetStep);
      return;
    }

    if (targetStep === displayedStepRef.current) return;

    const moveRoute = () => {
      const currentStep = displayedStepRef.current;
      if (currentStep === targetStep) {
        routeTimerRef.current = null;
        return;
      }
      const direction = currentStep < targetStep ? 1 : -1;
      const nextStep = (currentStep + direction) as BlueprintRouteStep;
      displayedStepRef.current = nextStep;
      setDisplayedStep(nextStep);
      if (nextStep !== targetStep) {
        routeTimerRef.current = window.setTimeout(moveRoute, 400);
      } else {
        routeTimerRef.current = null;
      }
    };

    moveRoute();
    return () => {
      if (routeTimerRef.current !== null) {
        window.clearTimeout(routeTimerRef.current);
        routeTimerRef.current = null;
      }
    };
  }, [motionDisabled, pageHidden, phase, targetStep]);

  useEffect(() => {
    if (phase !== 'verifying') {
      executionCursorPhaseRef.current = phase;
      setExecutionCursorState('hidden');
      return;
    }

    if (executionCursorPhaseRef.current !== 'verifying') {
      executionCursorPhaseRef.current = 'verifying';
      setExecutionCursorState(motionDisabled || pageHidden ? 'done' : 'play');
      return;
    }

    if (motionDisabled || pageHidden) setExecutionCursorState('done');
  }, [motionDisabled, pageHidden, phase]);

  useEffect(() => () => {
    if (pointerFrameRef.current !== null) window.cancelAnimationFrame(pointerFrameRef.current);
    if (routeTimerRef.current !== null) window.clearTimeout(routeTimerRef.current);
  }, []);

  const flushPointer = useCallback(() => {
    pointerFrameRef.current = null;
    setPointer(pendingPointerRef.current);
  }, []);

  const schedulePointer = useCallback(() => {
    if (pointerFrameRef.current !== null) return;
    pointerFrameRef.current = window.requestAnimationFrame(flushPointer);
  }, [flushPointer]);

  const handlePointerMove = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (motionDisabled || pageHidden || !finePointer || document.visibilityState === 'hidden') return;
    if (event.pointerType === 'touch') return;
    const root = rootRef.current;
    if (!root) return;
    const rect = root.getBoundingClientRect();
    const x = clamp((event.clientX - rect.left) / Math.max(rect.width, 1), 0, 1);
    const y = clamp((event.clientY - rect.top) / Math.max(rect.height, 1), 0, 1);
    if (isBrandSafeZone(x, y)) {
      pendingPointerRef.current = null;
    } else {
      pendingPointerRef.current = {
        x,
        y,
        focusIds: getBlueprintFocusIds(x * 1000, y * 680),
      };
    }
    schedulePointer();
  }, [finePointer, motionDisabled, pageHidden, schedulePointer]);

  const handlePointerLeave = useCallback(() => {
    pendingPointerRef.current = null;
    if (motionDisabled || pageHidden || !finePointer) {
      setPointer(null);
      return;
    }
    schedulePointer();
  }, [finePointer, motionDisabled, pageHidden, schedulePointer]);

  const sceneClassName = [
    'flowy-intent-field',
    `flowy-intent-field--${mode}`,
    `flowy-intent-field--${phase}`,
    reducedMotion ? 'is-reduced-motion' : '',
    compactViewport ? 'is-compact-viewport' : '',
    pageHidden ? 'is-page-hidden' : '',
    className ?? '',
  ].filter(Boolean).join(' ');

  const pointerStyle = pointer ? {
    transform: `translate(${pointer.x * 1000}px ${pointer.y * 680}px)`,
  } : undefined;
  const pointerActive = Boolean(pointer && !motionDisabled && !pageHidden && finePointer);
  const executionCursorVisible = phase === 'verifying' && executionCursorState !== 'hidden';
  const executionCursorAtEnd = executionCursorState === 'done';

  const fragmentNodes = useMemo(
    () => BLUEPRINT_FRAGMENTS.map((fragment) => {
      const isPrimary = fragment.role === 'primary';
      const isEmailPreviewFragment = isPrimary && isEmailCapturePreview && fragment.routeStep === 1;
      const isArrived = arrivedFragmentIds.includes(fragment.id)
        && (!isEmailPreviewFragment || inputRouteProgress >= 0.55);
      const isActive = isPrimary && displayedStep > 0 && fragment.routeStep === displayedStep;
      const isSettled = isPrimary && phase === 'success';
      const isFaulted = isPrimary && phase === 'error' && fragment.routeStep === Math.max(displayedStep, 1);
      const isFocused = isPrimary && Boolean(pointer?.focusIds.includes(fragment.id));
      const centerX = fragment.x + fragment.width / 2;
      const centerY = fragment.y + fragment.height / 2;
      const pointerX = pointer ? pointer.x * 1000 : centerX;
      const pointerY = pointer ? pointer.y * 680 : centerY;
      const shiftX = isFocused ? clamp((centerX - pointerX) * 0.018, -3, 3) : 0;
      const shiftY = isFocused ? clamp((centerY - pointerY) * 0.018, -3, 3) : 0;
      const style: BlueprintStyle = {
        '--blueprint-shift-x': `${shiftX}px`,
        '--blueprint-shift-y': `${shiftY}px`,
        transform: `translate(${shiftX}px, ${shiftY}px)`,
      };
      const fragmentClassName = [
        'flowy-blueprint__fragment',
        `is-${fragment.role}`,
        `is-${fragment.variant}`,
        isActive ? 'is-active' : '',
        isArrived ? 'is-arrived' : '',
        isSettled ? 'is-settled' : '',
        isFaulted ? 'is-faulted' : '',
        isFocused ? 'is-pointer-focused' : '',
      ].filter(Boolean).join(' ');

      return (
        <g
          key={fragment.id}
          className={fragmentClassName}
          data-blueprint-fragment={fragment.id}
          data-blueprint-role={fragment.role}
          data-blueprint-active={isActive ? 'true' : 'false'}
          data-blueprint-arrived={isArrived ? 'true' : 'false'}
          data-blueprint-input-preview={isEmailPreviewFragment ? 'true' : 'false'}
          data-blueprint-route-step={fragment.routeStep}
          style={style}
        >
          <g transform={`rotate(${fragment.rotation} ${fragment.x + fragment.width / 2} ${fragment.y + fragment.height / 2})`}>
            {renderFragmentBody(fragment, isSettled, displayedStep)}
          </g>
        </g>
      );
    }),
    [arrivedFragmentIds, displayedStep, inputRouteProgress, isEmailCapturePreview, phase, pointer, safeActivationLevel]
  );

  return (
    <div
      ref={rootRef}
      className={sceneClassName}
      data-blueprint-mode={mode}
      data-blueprint-phase={phase}
      data-blueprint-step={displayedStep}
      data-blueprint-target-step={targetStep}
      data-blueprint-activation={safeActivationLevel}
      data-blueprint-input-energy={safeInputEnergy.toFixed(2)}
      data-blueprint-reduced-motion={reducedMotion ? 'true' : 'false'}
      onPointerEnter={handlePointerMove}
      onPointerMove={handlePointerMove}
      onPointerLeave={handlePointerLeave}
    >
      <svg
        className='flowy-blueprint'
        viewBox={BLUEPRINT_VIEWBOX}
        preserveAspectRatio='xMidYMid slice'
        aria-hidden='true'
        focusable='false'
      >
        <g className='flowy-blueprint__support-lines' data-blueprint-lane='support'>
          {BLUEPRINT_SUPPORT_LINES.map((supportLine) => (
            <path
              key={supportLine.id}
              className='flowy-blueprint__support-line'
              d={supportLine.path}
              pathLength='1'
              data-blueprint-support-line={supportLine.id}
            />
          ))}
        </g>

        <g className='flowy-blueprint__route-segments'>
          {BLUEPRINT_ROUTE_SEGMENTS.map((segment) => {
            const isInputPreviewSegment = isEmailCapturePreview && segment.step === 1;
            const isReached = segment.step < displayedStep
              || (segment.step === displayedStep && (!isInputPreviewSegment || inputRouteProgress >= 1));
            const isCurrent = segment.step === displayedStep && displayedStep > 0;
            const strokeDashoffset = isReached
              ? 0
              : isInputPreviewSegment && displayedStep === 1
                ? 1 - inputRouteProgress
                : 1;
            return (
              <g key={segment.id} data-blueprint-route-segment={segment.id} data-blueprint-route-step={segment.step}>
                <path className='flowy-blueprint__route-segment' d={segment.path} pathLength='1' />
                <path
                  className={`flowy-blueprint__route-segment-active${isReached ? ' is-reached' : ''}${isCurrent ? ' is-current' : ''}`}
                  d={segment.path}
                  pathLength='1'
                  strokeDasharray='1'
                  strokeDashoffset={strokeDashoffset}
                />
              </g>
            );
          })}
        </g>

        <g className='flowy-blueprint__ambient-marks'>
          {BLUEPRINT_AMBIENT_MARKS.map((mark) => renderAmbientMark(mark, displayedStep >= mark.routeStep))}
        </g>

        <g className='flowy-blueprint__documents'>{fragmentNodes}</g>

        {executionCursorVisible && (
          <g
            className='flowy-blueprint__execution-cursor'
            transform={executionCursorAtEnd ? `translate(${BLUEPRINT_CURSOR_END.x} ${BLUEPRINT_CURSOR_END.y})` : undefined}
          >
            <circle cx='0' cy='0' r='4'>
              {executionCursorState === 'play' && (
                <animateMotion
                  path={BLUEPRINT_VERIFY_PATH}
                  dur='480ms'
                  begin='0s'
                  calcMode='spline'
                  keyTimes='0;1'
                  keySplines='0.16 1 0.3 1'
                  rotate='0'
                  fill='freeze'
                />
              )}
            </circle>
          </g>
        )}

        <g className='flowy-blueprint__marks'>
          {BLUEPRINT_MARKS.map((mark) => (
            <g key={`${mark.x}-${mark.y}`} transform={`translate(${mark.x} ${mark.y})`}>
              <path d={`M ${-mark.size / 2} 0 H ${mark.size / 2} M 0 ${-mark.size / 2} V ${mark.size / 2}`} />
              <circle cx='0' cy='0' r='1.5' />
            </g>
          ))}
        </g>

        <g
          className={`flowy-blueprint__pointer-marker${pointerActive ? ' is-visible' : ''}`}
          style={pointerStyle}
        >
          <path d='M -14 0 H 14 M 0 -14 V 14' />
          <circle cx='0' cy='0' r='4' />
        </g>

        <g className='flowy-blueprint__registration'>
          <path d='M 116 198 H 170 M 116 198 V 220 M 866 532 H 922 M 922 532 V 510' />
          <path d='M 720 102 H 698 M 720 102 V 124 M 932 280 H 954 M 954 280 V 258' />
        </g>
      </svg>
      <div className='flowy-intent-field__content'>{children}</div>
    </div>
  );
};

export default IntentField;
