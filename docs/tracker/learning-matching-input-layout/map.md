# Map: 匹配题输入布局波（l1）

<!-- wayfinder:map -->
<!-- 状态：**已关波（2026-11-18）**：l1 施工收口（label 40% 真实基准 +
      select 60% 有界；根因=flex-1 基准 0 被 Arco Select width:100% 的
      basis 吸收全部收缩量）——渲染测试 6 绿、typecheck/theme/dead-css
      绿。真机复验欠账：匹配题左列正常换行、select 宽度合理。
      原状态：开图（2026-11-18）。 -->

## 已核验事实（2026-11-18 对码）

- 布局根因（ActivityInput.tsx:101-116 MatchingInput）：行为
  `flex items-center gap-8px`，label `<span className='min-w-80px flex-1'>`
  （flex:1 1 **0**——基准宽 0），Arco Select 默认 `width:100%`（flex-basis
  ≈ 整行）。收缩按 shrink×basis 分摊：label 基准 0 不收缩、Select 吸收
  全部收缩 → label 塌缩到 min-width 80px、Select 占满剩余。长短语左列
  竖排 = 截图形态。
- 影响面：ActivityInput 被课时练习与复习卡片共用（:124 注释）——两处
  同坏同修。
- 次要观察（不在本票）：matching 左列/候选渲染为裸文本 `{left}`，其它
  题型选项走 InlineMarkdown 管线（:157/:176 注释）——一致性留观。

## 票表

| id | 票 | 严重度 | 类别 |
| --- | --- | --- | --- |
| l1 | 匹配题行布局：label 列基准宽修正 + select 宽度约束 | 中（练习主场景排版异常） | 呈现 |

## 裁决待定

- l1 修法取 R-a（label 40% 基准 + select flex-1）——无真分歧，直接施工。
