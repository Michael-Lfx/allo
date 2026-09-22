/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { EvalBusinessReport } from './api';
import {
  buildBusinessReportCsv,
  buildBusinessReportHtml,
  businessReportFilename,
  csvCell,
  exportKindFromPath,
} from './businessReportExport';

const report: EvalBusinessReport = {
  run_id: 'abcdef12-rest',
  suite: 'imported-pack',
  model: 'gpt-x',
  status: 'completed',
  passed_cases: 2,
  failed_cases: 1,
  unique_cases: 3,
  tasks: [],
  goal_rows: [
    {
      label: 'Brief, "quoted"',
      hint: '题目要求的文件',
      t01: '✓',
      t02: '✗',
      t03: '—',
    },
  ],
  efficiency_rows: [
    {
      label: '结束原因',
      hint: '不等于本题对错',
      t01: '正常结束（模型主动收尾）',
      t02: '出错：<script>alert(1)</script>',
      t03: '输出被截断（达到 token 上限）',
    },
  ],
};

describe('business report export', () => {
  test('quotes commas, quotes, and line breaks', () => {
    expect(csvCell('plain')).toBe('plain');
    expect(csvCell('a,b')).toBe('"a,b"');
    expect(csvCell('say "hi"')).toBe('"say ""hi"""');
    expect(csvCell('line\nbreak')).toBe('"line\nbreak"');
  });

  test('csv includes a meaning column and stop-reason legend', () => {
    const csv = buildBusinessReportCsv(report);
    expect(csv.startsWith('\uFEFF')).toBe(false);
    expect(csv.includes('\r\n')).toBe(true);
    expect(csv).toContain('含义');
    expect(csv).toContain('结束原因怎么读');
    expect(csv).toContain('"Brief, ""quoted"""');
    expect(csv).toContain('不等于本题对错');
  });

  test('html is a readable report and escapes cell content', () => {
    const html = buildBusinessReportHtml(report);
    expect(html.startsWith('<!DOCTYPE html>')).toBe(true);
    expect(html).toContain('商务评测报告');
    expect(html).toContain('结束原因怎么读');
    expect(html).toContain('正常结束');
    expect(html).toContain('&lt;script&gt;alert(1)&lt;/script&gt;');
    expect(html.includes('<script>alert(1)</script>')).toBe(false);
    expect(html).toContain('col-hint');
  });

  test('filename and kind follow the destination extension', () => {
    expect(businessReportFilename(report.run_id)).toBe('eval-report-abcdef12.html');
    expect(businessReportFilename(report.run_id, 'csv')).toBe('eval-report-abcdef12.csv');
    expect(exportKindFromPath('C:\\\\tmp\\\\a.csv')).toBe('csv');
    expect(exportKindFromPath('C:\\\\tmp\\\\a.html')).toBe('html');
  });
});
