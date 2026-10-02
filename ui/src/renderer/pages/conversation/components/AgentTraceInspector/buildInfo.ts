/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ObservationBuildInfo } from './useAgentTraces';

const SHORT_SHA_LENGTH = 7;

function knownSha(build: ObservationBuildInfo): string | null {
  const sha = build.git_sha?.trim();
  return sha && sha !== 'unknown' ? sha : null;
}

function formatBuildTime(value: string | null | undefined): string | null {
  if (!value) return null;
  const seconds = Number(value);
  if (!Number.isFinite(seconds) || seconds <= 0) return value;
  return new Date(seconds * 1000).toLocaleString();
}

/** `v1.5.3 · 0123456`, with `dirtyLabel` appended when the tree had local edits. */
export function formatBuildBrief(build: ObservationBuildInfo, dirtyLabel: string): string {
  const sha = knownSha(build);
  const parts = [`v${build.app_version}`];
  if (sha) parts.push(sha.slice(0, SHORT_SHA_LENGTH));
  if (sha && build.git_dirty) parts.push(dirtyLabel);
  return parts.join(' · ');
}

/** Everything needed to match a trace to source, on one copyable line. */
export function formatBuildFull(build: ObservationBuildInfo, dirtyLabel: string): string {
  const sha = knownSha(build);
  const parts = [`v${build.app_version}`];
  if (sha) parts.push(build.git_dirty ? `${sha} (${dirtyLabel})` : sha);
  if (build.profile && build.profile !== 'unknown') parts.push(build.profile);
  const builtAt = formatBuildTime(build.build_time);
  if (builtAt) parts.push(builtAt);
  parts.push(`${build.os}/${build.arch}`);
  return parts.join(' · ');
}
