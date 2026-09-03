/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type InspectStage = 'request' | 'response' | 'tool';

export interface InspectTarget {
  modelCallId: string;
  stage: InspectStage;
  toolCallId?: string;
}

/**
 * UI-only selection state. eventSeq also identifies non-call timeline events
 * such as turn boundaries and observation gaps.
 */
export interface TraceSelection {
  eventSeq: number;
  inspectTarget: InspectTarget | null;
}

export function sameInspect(a: InspectTarget | null, b: InspectTarget | null): boolean {
  return (
    a != null &&
    b != null &&
    a.modelCallId === b.modelCallId &&
    a.stage === b.stage &&
    a.toolCallId === b.toolCallId
  );
}
