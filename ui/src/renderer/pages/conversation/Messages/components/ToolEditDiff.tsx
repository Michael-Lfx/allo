/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import InlineDiff from '@renderer/components/beautifulUi/inlineDiff/InlineDiff';
import {
  INLINE_DIFF_COLLAPSE_LINE_THRESHOLD,
  countDiffLines,
} from '@renderer/components/beautifulUi/inlineDiff/inlineDiffModel';
import React from 'react';
import type { EditDiffPreview } from './buildEditDiff';

const ToolEditDiff: React.FC<{ preview: EditDiffPreview; defaultExpanded?: boolean }> = ({
  preview,
  defaultExpanded,
}) => (
  <InlineDiff
    filename={preview.displayName}
    hunks={preview.hunks}
    insertions={preview.insertions}
    deletions={preview.deletions}
    defaultExpanded={defaultExpanded ?? countDiffLines(preview.hunks) <= INLINE_DIFF_COLLAPSE_LINE_THRESHOLD}
  />
);

export default ToolEditDiff;
