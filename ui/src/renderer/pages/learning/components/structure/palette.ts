/** 主干稳定配色（按主干在结构源中的出现顺序取色；图与详情共用一源）。
 * 固定色板属数据语义（主干身份），不随明暗主题切换；底色/文字/边线仍走主题令牌。 */
export const GROUP_COLORS = [
  '#60a5fa',
  '#34d399',
  '#f472b6',
  '#fbbf24',
  '#a78bfa',
  '#22d3ee',
  '#fb923c',
  '#94a3b8',
  '#f87171',
  '#4ade80',
  '#e879f9',
  '#38bdf8',
] as const;

export type ColorFor = (spineName: string) => string;

export function buildGroupPalette(spineNames: string[]): ColorFor {
  const map = new Map<string, string>(
    spineNames.map((name, index) => [name, GROUP_COLORS[index % GROUP_COLORS.length]])
  );
  return (raw) => map.get(raw) ?? '#94a3b8';
}

/** 边型配色（assumes 门 / composes 组装 / contrasts 对照）。 */
export const EDGE_COLORS: Record<string, string> = {
  assumes: '#4f9bff',
  composes: '#8896c7',
  contrasts: '#ffa726',
};

/** 枢纽描边（琥珀）。 */
export const HUB_COLOR = '#fbbf24';

/** assumes = 门的方向映射（布局与渲染的单一来源）：assumes 的「先有的一方」
 * 是 target——布局时前置排左、画箭头时前置 → 需求方，读作「先过这道前置门
 * 才轮到本块」；composes/contrasts 保持契约方向 source → target。
 * 返回 `[from, to]`（节点名）。 */
export function edgeEndpoints(edge: {
  source: string;
  target: string;
  edge_type: string;
}): [string, string] {
  return edge.edge_type === 'assumes' ? [edge.target, edge.source] : [edge.source, edge.target];
}
