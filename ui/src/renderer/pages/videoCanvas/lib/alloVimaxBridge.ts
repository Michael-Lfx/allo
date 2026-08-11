/**
 * Quality guards when regenerating media that originated from a ViMax Agent session.
 */

import type { CanvasNodeData } from '@oc/types/canvas';

export type AlloVimaxShotMeta = {
  kind?: string;
  sessionId?: string;
  sceneKey?: string;
  shotIdx?: number;
  voiceClauses?: string[];
  ffDesc?: string;
  lfDesc?: string;
  motionDesc?: string;
  visualDesc?: string;
  characterIdxs?: number[];
  artifactRel?: string;
};

export type AlloCreativeSidecar = {
  version?: number;
  source?: string;
  sessionId?: string;
  montageProjectId?: string;
  workflow?: string;
  title?: string;
  style?: string;
  writeBack?: {
    enabled?: boolean;
    sessionId?: string;
    montageProjectId?: string;
    policy?: string;
  };
};

export function readAlloVimax(node?: CanvasNodeData | null): AlloVimaxShotMeta | null {
  const raw = node?.metadata?.alloVimax;
  if (!raw || typeof raw !== 'object') return null;
  return raw as AlloVimaxShotMeta;
}

export function readAlloCreative(project: { alloCreative?: unknown } | null | undefined): AlloCreativeSidecar | null {
  const raw = project?.alloCreative;
  if (!raw || typeof raw !== 'object') return null;
  return raw as AlloCreativeSidecar;
}

/** Ensure FIXED SPEAKER VOICE clauses survive Canvas regenerations. */
export function enrichPromptWithVimaxVoiceGuards(
  prompt: string,
  node?: CanvasNodeData | null,
  siblingNodes: CanvasNodeData[] = [],
): string {
  const base = (prompt || '').trim();
  const clauses = collectVoiceClauses(node, siblingNodes);
  if (!clauses.length) return base;

  const missing = clauses.filter((clause) => clause && !base.includes(clause));
  if (!missing.length) return base;
  return [base, ...missing].filter(Boolean).join('\n');
}

function collectVoiceClauses(node: CanvasNodeData | null | undefined, siblings: CanvasNodeData[]): string[] {
  const out: string[] = [];
  const meta = readAlloVimax(node);
  for (const clause of meta?.voiceClauses || []) {
    if (clause?.trim()) out.push(clause.trim());
  }

  const voiceFromNode = node?.metadata?.characterVoiceInstructions?.trim();
  if (voiceFromNode) out.push(voiceFromNode);

  for (const sibling of siblings) {
    if (sibling.metadata?.workflowKind !== 'character') continue;
    const instructions = sibling.metadata?.characterVoiceInstructions?.trim();
    if (instructions) out.push(instructions);
    const alloRaw = sibling.metadata?.alloVimax as { voiceClause?: string } | undefined;
    const fromAllo = alloRaw?.voiceClause?.trim();
    if (fromAllo) out.push(fromAllo);
  }

  const seen = new Set<string>();
  return out.filter((c) => {
    if (seen.has(c)) return false;
    seen.add(c);
    return true;
  });
}

export function preserveAlloVimaxOnRegenerate(
  sourceNode: CanvasNodeData | undefined,
): Record<string, unknown> {
  const allo = sourceNode?.metadata?.alloVimax;
  if (!allo || typeof allo !== 'object') return {};
  return { alloVimax: allo };
}
