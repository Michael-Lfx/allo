import { ChartColumn } from "lucide-react";

import { useUpstreamNodes } from "@oc/components/canvas/canvas-node-graph-context";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { getNodeResourceKind } from "@oc/lib/canvas/node-registry";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasNodeData } from "@oc/types/canvas";

type ChartNodeContentProps = {
    node: CanvasNodeData;
    theme: CanvasTheme;
};

type ChartRow = Record<string, string | number>;
type ParsedChart = { rows: ChartRow[]; xKey: string; series: string[] };

/**
 * 把一段文本解析成图表数据；解析不出来返回 null。
 * 必须不抛：数据来自模型输出，非法 JSON 是常态。
 */
export function parseChartSource(text: string): ParsedChart | null {
    const trimmed = text.trim();
    if (!trimmed) return null;
    const rows = parseJsonRows(trimmed) ?? parseCsvRows(trimmed);
    if (!rows?.length) return null;

    const keys = Object.keys(rows[0]);
    if (!keys.length) return null;
    const series = keys.filter((key) => rows.every((row) => typeof row[key] === "number"));
    const xKey = keys.find((key) => !series.includes(key)) || "__index";
    if (!series.length) return null;
    if (xKey === "__index") rows.forEach((row, index) => {
        row.__index = index + 1;
    });
    return { rows, xKey, series };
}

function parseJsonRows(text: string): ChartRow[] | null {
    try {
        const parsed: unknown = JSON.parse(text);
        if (!Array.isArray(parsed)) return null;
        const rows = parsed.filter((item): item is ChartRow => Boolean(item) && typeof item === "object" && !Array.isArray(item));
        return rows.length ? rows.map(normalizeRow) : null;
    } catch {
        return null;
    }
}

function parseCsvRows(text: string): ChartRow[] | null {
    const lines = text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
    if (lines.length < 2) return null;
    const header = lines[0].split(",").map((cell) => cell.trim());
    if (header.length < 2) return null;
    return lines.slice(1).map((line) => {
        const cells = line.split(",").map((cell) => cell.trim());
        return normalizeRow(Object.fromEntries(header.map((key, index) => [key, cells[index] ?? ""])));
    });
}

function normalizeRow(row: Record<string, unknown>): ChartRow {
    return Object.fromEntries(Object.entries(row).map(([key, value]) => {
        if (typeof value === "number") return [key, value];
        const text = String(value ?? "").trim();
        const numeric = text !== "" && Number.isFinite(Number(text));
        return [key, numeric ? Number(text) : text];
    }));
}

const SERIES_COLORS = ["#4f6ee8", "#22c55e", "#06b6d4", "#a855f7", "#ef4444"];

/**
 * 图表节点：把上游文本（JSON 数组或 CSV）渲染成简易 SVG 柱状/折线。
 * allo 未引入 recharts，用原生 SVG 兜底。
 */
export function ChartNodeContent({ node, theme }: ChartNodeContentProps) {
    const upstream = useUpstreamNodes(node.id);
    const inherited = upstream.find((item) => getNodeResourceKind(item) === "text");
    const source = node.metadata?.content || inherited?.metadata?.content || inherited?.metadata?.prompt || "";
    const parsed = parseChartSource(source);
    const asLine = node.metadata?.chartKind === "line";

    if (!parsed) {
        return (
            <div className="flex h-full w-full flex-col items-center justify-center gap-2 px-4 text-center" style={{ color: theme.node.muted }}>
                <ChartColumn className="size-5 opacity-60" />
                <span style={{ fontSize: "var(--fs-label)" }}>
                    {source.trim()
                        ? canvasT("videoCanvas.extension.chartParseFail", "无法解析为图表数据（需要 JSON 数组或 CSV）")
                        : canvasT("videoCanvas.extension.chartEmpty", "连接输出 JSON 数组或 CSV 的文本节点")}
                </span>
            </div>
        );
    }

    return (
        <div
            className="flex h-full w-full flex-col gap-1 px-2 py-2"
            data-canvas-no-zoom
            style={{ color: theme.node.text }}
            onWheel={(event) => event.stopPropagation()}
            onMouseDown={(event) => event.stopPropagation()}
        >
            <SimpleChartSvg parsed={parsed} asLine={asLine} theme={theme} />
            {parsed.series.length > 1 ? (
                <div className="flex flex-wrap gap-2 px-1" style={{ fontSize: "var(--fs-tiny)", color: theme.node.muted }}>
                    {parsed.series.map((key, index) => (
                        <span key={key} className="inline-flex items-center gap-1">
                            <span className="inline-block size-2 rounded-sm" style={{ background: SERIES_COLORS[index % SERIES_COLORS.length] }} />
                            {key}
                        </span>
                    ))}
                </div>
            ) : null}
        </div>
    );
}

function SimpleChartSvg({ parsed, asLine, theme }: { parsed: ParsedChart; asLine: boolean; theme: CanvasTheme }) {
    const pad = { top: 12, right: 12, bottom: 28, left: 36 };
    const width = 400;
    const height = 220;
    const plotW = width - pad.left - pad.right;
    const plotH = height - pad.top - pad.bottom;
    const values = parsed.rows.flatMap((row) => parsed.series.map((key) => Number(row[key]) || 0));
    const maxY = Math.max(...values, 1);
    const n = parsed.rows.length;
    const stepX = n > 1 ? plotW / (n - (asLine ? 1 : 0)) : plotW;

    return (
        <svg viewBox={`0 0 ${width} ${height}`} className="min-h-0 w-full flex-1" preserveAspectRatio="xMidYMid meet">
            {[0, 0.25, 0.5, 0.75, 1].map((t) => {
                const y = pad.top + plotH * (1 - t);
                return (
                    <g key={t}>
                        <line x1={pad.left} x2={pad.left + plotW} y1={y} y2={y} stroke={theme.node.stroke} strokeDasharray="3 3" />
                        <text x={pad.left - 6} y={y + 3} textAnchor="end" fill={theme.node.muted} fontSize={10}>{Math.round(maxY * t)}</text>
                    </g>
                );
            })}
            {asLine
                ? parsed.series.map((key, seriesIndex) => {
                    const points = parsed.rows.map((row, index) => {
                        const x = pad.left + (n === 1 ? plotW / 2 : index * stepX);
                        const y = pad.top + plotH * (1 - (Number(row[key]) || 0) / maxY);
                        return `${x},${y}`;
                    }).join(" ");
                    return <polyline key={key} fill="none" stroke={SERIES_COLORS[seriesIndex % SERIES_COLORS.length]} strokeWidth={2} points={points} />;
                })
                : parsed.rows.map((row, index) => {
                    const groupW = Math.max(8, (plotW / n) * 0.7);
                    const barW = groupW / parsed.series.length;
                    const groupX = pad.left + (plotW / n) * index + ((plotW / n) - groupW) / 2;
                    return parsed.series.map((key, seriesIndex) => {
                        const value = Number(row[key]) || 0;
                        const h = (value / maxY) * plotH;
                        const x = groupX + seriesIndex * barW;
                        const y = pad.top + plotH - h;
                        return <rect key={`${index}-${key}`} x={x} y={y} width={Math.max(1, barW - 1)} height={h} fill={SERIES_COLORS[seriesIndex % SERIES_COLORS.length]} rx={1} />;
                    });
                })}
            {parsed.rows.map((row, index) => {
                const x = asLine
                    ? pad.left + (n === 1 ? plotW / 2 : index * stepX)
                    : pad.left + (plotW / n) * index + plotW / n / 2;
                return (
                    <text key={index} x={x} y={height - 8} textAnchor="middle" fill={theme.node.muted} fontSize={10}>
                        {String(row[parsed.xKey] ?? "")}
                    </text>
                );
            })}
        </svg>
    );
}
