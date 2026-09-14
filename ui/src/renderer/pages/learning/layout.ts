/**
 * 学习模块内容宽度契约（标准布局 / 满宽布局）。
 *
 * 「左右边距太大」在宽屏上有三处独立来源，这里把它们收敛成一张表，避免三个
 * 渲染面各自漂移：
 *
 *   1. `.app-page-shell` 的 40px 内边距（`styles/layout.css`，全局共享，本契约
 *      不动它——它是内容不贴窗口边的最后一层保护）；
 *   2. 各面的内容列限宽上限（本文件的 STANDARD_* 常量）居中后留下的空白；
 *   3. 课时正文自身的行宽上限（STANDARD_READING_MAX_WIDTH）在它的 flex 槽位里
 *      居中后留下的空白。
 *
 * 满宽布局解决 2 与 3：去掉列宽上限让内容撑满页壳内边距，并把课时阅读区改成
 * 占满整行的边框面板，由**面板与边框**而不是**更长的文字行**去吸收余宽——正文
 * 行宽只从 760px 放宽到 1040px。这是刻意的取舍：中文长文行宽超过约 45 个汉字
 * 就开始跳行，所以「铺满」与「可读」冲突时以可读为先，余宽交给结构承担。
 */

/** 标准布局的内容列限宽：课程列表与大纲工作区。 */
export const STANDARD_LIST_MAX_WIDTH = 'md:max-w-1200px';

/** 标准布局的学习图工作区限宽（DAG 比列表宽一档，横向节点更多）。 */
export const STANDARD_GRAPH_MAX_WIDTH = 'md:max-w-1400px';

/** 标准布局的课时正文行宽（≈47 汉字/行，连续阅读舒适区）。 */
export const STANDARD_READING_MAX_WIDTH = 'max-w-760px';

/** 满宽布局的课时正文行宽（≈65 汉字/行，可读上限的放宽档）。 */
export const WIDE_READING_MAX_WIDTH = 'max-w-1040px';

/** 标准布局的大纲栏宽度。 */
export const STANDARD_OUTLINE_WIDTH = 'lg:w-264px';

/**
 * 满宽布局的大纲栏宽度：随视口三档放大，把宽屏余量更多地让给课时标题。
 *
 * 注意这里是 **content-box** 宽度——本仓库没有全局 `border-box` 重置（按元素
 * 显式写 `box-border`），而大纲栏带 `p-12px` 与 1px 边框，所以实际占位比标称值
 * 宽约 26px（280/340/420 → 约 306/366/446）。标准布局的 264px 同样是 content-box，
 * 两侧口径一致是刻意的：加 `box-border` 会把标准布局的大纲栏悄悄收窄 26px。
 */
export const WIDE_OUTLINE_WIDTH = 'lg:w-280px xl:w-340px 2xl:w-420px';

/**
 * 内容列限宽类名。满宽布局返回空串——不限宽，内容撑满 `.app-page-shell`
 * 的内边距，余宽由面板与边框承担。
 */
export function contentColumnWidth(wide: boolean, standardCap: string): string {
  return wide ? '' : standardCap;
}

/**
 * 课时阅读区外层面板类名。标准布局下这层只承担 flex 生长与防溢出（与原
 * `<section>` 的身份一致，不产生可见边框）；满宽布局下它长出边框与内边距，
 * 用面板本身占住整行余宽。
 */
export function readingPanelClass(wide: boolean): string {
  return wide
    ? 'flex min-w-0 flex-1 flex-col rd-10px border border-solid border-[var(--color-border-2)] p-16px lg:p-20px'
    : 'flex min-w-0 flex-1 flex-col';
}

/** 大纲栏宽度类名。 */
export function outlineWidthClass(wide: boolean): string {
  return wide ? WIDE_OUTLINE_WIDTH : STANDARD_OUTLINE_WIDTH;
}
