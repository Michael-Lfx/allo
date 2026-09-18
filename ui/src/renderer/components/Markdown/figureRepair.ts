/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { httpRequest } from '@/common/adapter/httpBridge';

export interface RepairFigureRequest {
  /** Fence language of the broken figure. */
  language: 'svg' | 'jsxgraph';
  /** Original figure source code. */
  code: string;
  /** Error message the renderer produced. */
  error: string;
}

export interface RepairFigureResponse {
  /** Corrected figure body, rendered in place of the broken one. */
  code: string;
}

/**
 * Send a broken lesson figure (source + renderer error) to the course
 * completer and get a corrected figure body back for in-place re-render.
 */
export const repairFigure = (request: RepairFigureRequest) =>
  httpRequest<RepairFigureResponse>('POST', '/api/learning/figures/repair', request);
