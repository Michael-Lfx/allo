/**
 * 目录 store 投影的排序规则。
 *
 * 刻意做成纯函数、无依赖：这是“最新”排序中唯一无需连接客户端即可被测试钉住的
 * 部分，因此组件调用它而非内联比较逻辑。
 */

/** “最新”排序所读取的唯一字段（一个 `StoreItem` 满足它）。 */
export interface PublishDated {
  published_at?: string | null;
}

/**
 * 最新：先按日期（最新的在前），再排无日期的；每个分组内部由渲染出的名称决定，因此
 * 顺序是确定的。
 *
 * `YYYY-MM-DD` 作为纯字符串比较即正确，因此此处不做任何解析。市场没有给出日期的条目
 * **不**被当作“尽可能最旧的日期”——它只是排在带日期的条目之后，这样“未声明日期”
 * 与“很久以前声明过日期”在视觉上保持可区分。
 */
export function sortNewestFirst<T extends PublishDated>(
  items: readonly T[],
  nameOf: (item: T) => string,
  lang: string,
): T[] {
  return [...items].sort((a, b) => {
    const left = a.published_at ?? "";
    const right = b.published_at ?? "";
    if (left !== right) {
      if (!left) return 1;
      if (!right) return -1;
      return right.localeCompare(left);
    }
    return nameOf(a).localeCompare(nameOf(b), lang);
  });
}

/**
 * 该列表是否任何条目都带有日期。
 *
 * 只有此时才提供“最新”控件：对一个没有任何条目拥有该字段的排序选项，会是一个什么都不
 * 做的控件，而真实的各市场今天根本不声明 `publishedAt`。
 */
export function hasAnyPublishedAt(items: readonly PublishDated[]): boolean {
  return items.some((item) => Boolean(item.published_at));
}
