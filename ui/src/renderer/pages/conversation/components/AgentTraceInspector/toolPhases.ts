/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { asRecord, type ProjectedToolExecution } from './useAgentTraces';

export interface ToolPhase {
  name: string;
  us: number;
}

export interface ToolPhaseRow extends ToolPhase {
  nested: boolean;
  share: number;
}

export interface ToolPhaseBreakdown {
  totalUs: number;
  rows: ToolPhaseRow[];
}

const STAGE_ORDER = ['hooks.pre', 'tool.execute', 'tool.post_process', 'hooks.post'] as const;
const NESTED_PARENT = 'tool.execute';

function parsePhases(value: unknown): ToolPhase[] {
  if (!Array.isArray(value)) return [];
  const phases: ToolPhase[] = [];
  for (const item of value) {
    const record = asRecord(item);
    if (!record || typeof record.name !== 'string') continue;
    const us = Number(record.us);
    if (!Number.isFinite(us) || us < 0) continue;
    phases.push({ name: record.name, us });
  }
  return phases;
}

/** `null` when the trace predates phase timing or the tool recorded none. */
export function toolPhaseBreakdown(tool: ProjectedToolExecution): ToolPhaseBreakdown | null {
  const finished = asRecord(tool.completed ?? tool.failed);
  const phases = parsePhases(finished?.phases);
  if (phases.length === 0) return null;

  const stageIndex = (name: string) => (STAGE_ORDER as readonly string[]).indexOf(name);
  const stages = phases
    .filter((phase) => stageIndex(phase.name) >= 0)
    .sort((a, b) => stageIndex(a.name) - stageIndex(b.name));
  const details = phases.filter((phase) => stageIndex(phase.name) < 0);

  const stageSum = stages.reduce((sum, phase) => sum + phase.us, 0);
  const recordedUs = Number(finished?.duration_us);
  const totalUs = Math.max(Number.isFinite(recordedUs) ? recordedUs : 0, stageSum);
  const share = (us: number) => (totalUs > 0 ? Math.min(1, us / totalUs) : 0);

  const rows: ToolPhaseRow[] = [];
  for (const stage of stages) {
    rows.push({ ...stage, nested: false, share: share(stage.us) });
    if (stage.name === NESTED_PARENT) {
      for (const detail of details) rows.push({ ...detail, nested: true, share: share(detail.us) });
    }
  }
  if (!stages.some((stage) => stage.name === NESTED_PARENT)) {
    for (const detail of details) rows.push({ ...detail, nested: false, share: share(detail.us) });
  }
  return { totalUs, rows };
}

export function formatPhaseDuration(us: number): string {
  if (!Number.isFinite(us) || us < 0) return '';
  if (us < 1000) return `${Math.round(us)}µs`;
  if (us < 1_000_000) {
    const ms = us / 1000;
    return `${ms < 10 ? ms.toFixed(1).replace(/\.0$/, '') : Math.round(ms)}ms`;
  }
  const seconds = us / 1_000_000;
  return `${seconds < 10 ? seconds.toFixed(2).replace(/\.?0+$/, '') : seconds.toFixed(1).replace(/\.0$/, '')}s`;
}

export function phaseLabelKey(name: string): string {
  return 'conversation.agentTrace.phase_' + name.replace(/[^a-z0-9]+/gi, '_');
}
