/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { ipcBridge } from '@/common';
import { downloadTextContent } from '@/renderer/utils/file/download';
import { isDesktopShell } from '@/renderer/utils/platform';
import type { EvalBusinessMatrixRow, EvalBusinessReport } from './api';

const CSV_BOM = '\uFEFF';

const TASK_HEADERS = ['T01 联网调研', 'T02 项目汇总', 'T03 销售分析'] as const;

const STOP_GUIDE_ROWS: Array<[string, string]> = [
  ['正常结束', '模型认为可以收工，主动停下来。不等于产物一定写全。'],
  ['停在工具调用', '最后一步还在调工具，循环没有正常收口。'],
  ['输出被截断', '碰到 token 上限，回答被砍断，文件可能只写到一半。'],
  ['轮次用尽', '碰到最大对话轮数，强制停。'],
  ['模型拒绝', '模型拒答，没有继续做题。'],
  ['评测被取消', '人手取消，或策略超时取消。'],
  ['出错', '评测进程自己失败（网络、权限、异常）。这行出现时优先看错误内容。'],
];

type BrowserSaveFilePicker = (options?: {
  suggestedName?: string;
  types?: Array<{ description?: string; accept: Record<string, string[]> }>;
}) => Promise<FileSystemFileHandle>;

export type ExportKind = 'html' | 'csv';

export type ExportBusinessReportResult =
  | { status: 'cancelled' }
  | { status: 'saved'; path: string };

export type ExportBusinessReportOptions = {
  htmlFilterName?: string;
  csvFilterName?: string;
};

export function csvCell(value: string): string {
  if (/[",\n\r]/.test(value)) {
    return `"${value.replace(/"/g, '""')}"`;
  }
  return value;
}

export function businessReportFilename(runId: string, kind: ExportKind = 'html'): string {
  const slug = runId.replace(/[\\/:*?"<>|]/g, '-').slice(0, 8) || 'report';
  return `eval-report-${slug}.${kind}`;
}

export function exportKindFromPath(path: string): ExportKind {
  return path.toLowerCase().endsWith('.csv') ? 'csv' : 'html';
}

function matrixLine(row: EvalBusinessMatrixRow): string[] {
  return [row.label, row.hint?.trim() || '', row.t01, row.t02, row.t03];
}

export function buildBusinessReportCsv(report: EvalBusinessReport): string {
  const header = ['项目', '含义', ...TASK_HEADERS];
  const lines: string[][] = [
    ['商务评测报告'],
    ['云端模型', report.model ?? '—'],
    ['套件', report.suite],
    ['运行 ID', report.run_id],
    ['通过', `${report.passed_cases}/${report.unique_cases} 题`],
    [],
    ['目标达成'],
    header,
    ...report.goal_rows.map(matrixLine),
    [],
    ['Agent 循环效率'],
    header,
    ...report.efficiency_rows.map(matrixLine),
    [],
    ['结束原因怎么读（不等于本题对错）'],
    ['状态', '含义'],
    ...STOP_GUIDE_ROWS.map(([label, meaning]) => [label, meaning]),
    [],
    ['参考项（advisory）不计入本题是否通过。T03 算术允许约 1% 误差。'],
  ];
  return lines.map((cells) => cells.map((cell) => csvCell(String(cell))).join(',')).join('\r\n');
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function cellToneClass(value: string): string {
  const text = value.trim();
  if (!text || text === '—' || text === '未跑') return 'is-muted';
  const hasFail =
    text.includes('✗') ||
    text.startsWith('未通过') ||
    text.startsWith('未评分') ||
    text.startsWith('出错');
  const hasPass = text.includes('✓') || text.startsWith('Gate 全过');
  const truncated = text.includes('截断') || text.includes('掐断') || text.includes('停在工具');
  if (hasFail && hasPass) return 'is-warn';
  if (hasFail) return 'is-bad';
  if (truncated) return 'is-warn';
  if (hasPass) return 'is-good';
  if (/失败 [1-9]/.test(text)) return 'is-warn';
  return '';
}

function htmlCell(value: string, className = ''): string {
  const tone = cellToneClass(value);
  const cls = [className, tone].filter(Boolean).join(' ');
  const body = escapeHtml(value).replace(/\r\n|\n|\r/g, '<br>');
  return `<td${cls ? ` class="${cls}"` : ''}>${body || '—'}</td>`;
}

function htmlTable(title: string, lede: string, rows: EvalBusinessMatrixRow[]): string {
  const body = rows
    .map((row) => {
      return `<tr>
        ${htmlCell(row.label, 'col-label')}
        ${htmlCell(row.hint?.trim() || '—', 'col-hint')}
        ${htmlCell(row.t01)}
        ${htmlCell(row.t02)}
        ${htmlCell(row.t03)}
      </tr>`;
    })
    .join('');
  return `<section>
    <h2>${escapeHtml(title)}</h2>
    <p class="lede">${escapeHtml(lede)}</p>
    <table>
      <thead>
        <tr>
          <th>项目</th>
          <th>含义</th>
          ${TASK_HEADERS.map((title) => `<th>${escapeHtml(title)}</th>`).join('')}
        </tr>
      </thead>
      <tbody>${body}</tbody>
    </table>
  </section>`;
}

export function buildBusinessReportHtml(report: EvalBusinessReport): string {
  const passed = `${report.passed_cases} / ${report.unique_cases}`;
  const scoreClass =
    report.unique_cases > 0 && report.passed_cases === report.unique_cases ? 'is-good' : 'is-warn';
  const stopGuide = STOP_GUIDE_ROWS.map(
    ([label, meaning]) =>
      `<div class="guide-row"><dt>${escapeHtml(label)}</dt><dd>${escapeHtml(meaning)}</dd></div>`
  ).join('');

  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>商务评测报告 · ${escapeHtml(report.run_id.slice(0, 8))}</title>
<style>
  :root {
    --ink: #1c1915;
    --muted: #6a6158;
    --paper: #f4efe6;
    --card: #fffdf8;
    --line: #d8cec0;
    --pine: #1e3a34;
    --good: #1f6b4a;
    --bad: #9b2c2c;
    --warn: #8a5a12;
  }
  * { box-sizing: border-box; }
  html, body { margin: 0; padding: 0; background: var(--paper); color: var(--ink); }
  body {
    font: 15px/1.55 "Segoe UI", "PingFang SC", "Microsoft YaHei UI", "Noto Sans SC", sans-serif;
    padding: 32px 28px 64px;
  }
  .sheet {
    max-width: 1080px;
    margin: 0 auto;
    background: var(--card);
    border: 1px solid var(--line);
    box-shadow: 0 18px 40px rgba(28, 25, 21, 0.08);
  }
  header {
    border-top: 8px solid var(--pine);
    padding: 28px 32px 24px;
    border-bottom: 1px solid var(--line);
  }
  .kicker {
    margin: 0 0 8px;
    font-size: 11px;
    letter-spacing: 0.18em;
    text-transform: uppercase;
    color: var(--pine);
    font-weight: 700;
  }
  h1 { margin: 0 0 16px; font-size: 28px; letter-spacing: -0.03em; font-weight: 650; }
  .meta { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px 20px; margin: 0; }
  .meta div { margin: 0; }
  .meta dt { margin: 0; font-size: 11px; color: var(--muted); letter-spacing: 0.04em; }
  .meta dd { margin: 2px 0 0; font-weight: 600; word-break: break-all; }
  .score { margin: 18px 0 0; font-size: 20px; font-weight: 700; }
  .score.is-good { color: var(--good); }
  .score.is-warn { color: var(--warn); }
  section { padding: 24px 32px; }
  section + section { border-top: 1px solid var(--line); }
  h2 { margin: 0 0 6px; font-size: 18px; }
  .lede { margin: 0 0 14px; color: var(--muted); max-width: 62ch; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th, td { border: 1px solid var(--line); padding: 10px 12px; vertical-align: top; text-align: left; }
  th { background: #efe8dc; font-size: 12px; letter-spacing: 0.02em; }
  tbody tr:nth-child(even) td { background: #fbf7f0; }
  td.col-label { font-weight: 650; white-space: nowrap; width: 7.5em; }
  td.col-hint { color: var(--muted); width: 22%; }
  td.is-good { color: var(--good); font-weight: 650; }
  td.is-bad { color: var(--bad); font-weight: 650; }
  td.is-warn { color: var(--warn); font-weight: 650; }
  td.is-muted { color: #9a9187; }
  .guide { display: grid; gap: 10px; }
  .guide-row { display: grid; grid-template-columns: 8.5em 1fr; gap: 12px; }
  .guide-row dt { font-weight: 700; }
  .guide-row dd { margin: 0; color: var(--muted); }
  footer { padding: 0 32px 28px; color: var(--muted); font-size: 12px; }
  @media (max-width: 800px) {
    body { padding: 12px; }
    header, section, footer { padding-left: 16px; padding-right: 16px; }
    .meta { grid-template-columns: 1fr 1fr; }
    .guide-row { grid-template-columns: 1fr; gap: 2px; }
    td.col-label { white-space: normal; }
  }
  @media print {
    body { background: white; padding: 0; }
    .sheet { box-shadow: none; border: none; }
  }
</style>
</head>
<body>
  <article class="sheet">
    <header>
      <p class="kicker">Agent 评测实验室</p>
      <h1>商务评测报告</h1>
      <dl class="meta">
        <div><dt>云端模型</dt><dd>${escapeHtml(report.model || '—')}</dd></div>
        <div><dt>套件</dt><dd>${escapeHtml(report.suite)}</dd></div>
        <div><dt>运行 ID</dt><dd>${escapeHtml(report.run_id)}</dd></div>
        <div><dt>状态</dt><dd>${escapeHtml(report.status)}</dd></div>
      </dl>
      <p class="score ${scoreClass}">${escapeHtml(passed)} 题通过</p>
    </header>
    ${htmlTable(
      '目标达成',
      '这张表看 AGENT 有没有把该交的东西交出来。本题结果只看结构 Gate，参考项失败仍可通过。',
      report.goal_rows
    )}
    ${htmlTable(
      'Agent 循环效率',
      '这张表看跑得是否干净。耗时、轮次、工具失败是效率信号；结束原因只解释为什么停，不是对错判定。',
      report.efficiency_rows
    )}
    <section>
      <h2>结束原因怎么读</h2>
      <p class="lede">结束原因 ≠ 本题对错。只有「本题结果」和结构 Gate 决定是否通过。</p>
      <dl class="guide">${stopGuide}</dl>
    </section>
    <footer>参考项（advisory）不计入本题是否通过。T03 算术允许约 1% 误差。本文件可直接用浏览器打开或打印。</footer>
  </article>
</body>
</html>
`;
}

function isAbortLike(error: unknown): boolean {
  return (
    error != null &&
    typeof error === 'object' &&
    'name' in error &&
    (error as { name?: unknown }).name === 'AbortError'
  );
}

function ensureExportPath(path: string, kind: ExportKind): string {
  const lower = path.toLowerCase();
  if (kind === 'csv') return lower.endsWith('.csv') ? path : `${path}.csv`;
  if (lower.endsWith('.html') || lower.endsWith('.htm')) return path;
  return `${path}.html`;
}

function browserSaveFilePicker(): BrowserSaveFilePicker | null {
  if (typeof window === 'undefined') return null;
  return (window as Window & { showSaveFilePicker?: BrowserSaveFilePicker }).showSaveFilePicker ?? null;
}

export async function exportBusinessReport(
  report: EvalBusinessReport,
  options?: ExportBusinessReportOptions
): Promise<ExportBusinessReportResult> {
  const htmlName = options?.htmlFilterName?.trim() || 'HTML';
  const csvName = options?.csvFilterName?.trim() || 'CSV';
  const filename = businessReportFilename(report.run_id, 'html');

  const write = async (dest: string): Promise<ExportBusinessReportResult> => {
    const kind = exportKindFromPath(dest);
    const path = ensureExportPath(dest, kind);
    const body = kind === 'csv' ? `${CSV_BOM}${buildBusinessReportCsv(report)}` : buildBusinessReportHtml(report);
    if (isDesktopShell()) {
      const saved = await ipcBridge.fs.writeFile.invoke({ path, data: body });
      if (!saved) throw new Error('write_failed');
      try {
        await ipcBridge.shell.showItemInFolder.invoke(path);
      } catch {
        // The file is already written; revealing the folder is optional.
      }
      return { status: 'saved', path };
    }
    downloadTextContent(body, path.split(/[\\/]/).pop() || filename, kind === 'csv' ? 'text/csv;charset=utf-8;' : 'text/html;charset=utf-8');
    return { status: 'saved', path };
  };

  if (isDesktopShell()) {
    let path: string | null;
    try {
      path = await ipcBridge.dialog.showSave.invoke({
        defaultPath: filename,
        filters: [
          { name: htmlName, extensions: ['html'] },
          { name: csvName, extensions: ['csv'] },
        ],
      });
    } catch (error) {
      if (isAbortLike(error)) return { status: 'cancelled' };
      throw error;
    }
    if (!path) return { status: 'cancelled' };
    return write(path);
  }

  const picker = browserSaveFilePicker();
  if (picker) {
    try {
      const handle = await picker({
        suggestedName: filename,
        types: [
          { description: htmlName, accept: { 'text/html': ['.html'] } },
          { description: csvName, accept: { 'text/csv': ['.csv'] } },
        ],
      });
      const kind = exportKindFromPath(handle.name || filename);
      const body = kind === 'csv' ? `${CSV_BOM}${buildBusinessReportCsv(report)}` : buildBusinessReportHtml(report);
      const writable = await handle.createWritable();
      try {
        await writable.write(body);
        await writable.close();
      } catch (error) {
        try {
          await writable.abort?.();
        } catch {
          // Preserve the original write error.
        }
        throw error;
      }
      return { status: 'saved', path: handle.name?.trim() || filename };
    } catch (error) {
      if (isAbortLike(error)) return { status: 'cancelled' };
      throw error;
    }
  }

  const body = buildBusinessReportHtml(report);
  downloadTextContent(body, filename, 'text/html;charset=utf-8');
  return { status: 'saved', path: filename };
}
