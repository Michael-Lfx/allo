# Flowy 视觉方向（P2）

两板择一落地。**已选：Ink Studio**。Warm Atelier 仅作对照，不实现。

## A · Ink Studio（落地）

- **气质**：冷静工作站、墨色结构、单一青石强调；像「把想法直接落成成果」的精密台面，而不是聊天玩具。
- **参考**：Linear（密度与焦点）、Cursor（编辑器气质）、Arc（分栏层次）——只借纪律，不借皮肤。
- **Flowy 识别元素**：分栏墨色 brand 面 + 点阵/网格预览、主输入单焦点舞台、成果揭幕用青石描边而非彩色大底。
- **禁止**：紫粉霓虹铺底、奶油衬线海报风、同权卡片墙、多主 CTA、无语义循环 glow。

## B · Warm Atelier（否决）

- 气质：暖纸张 + 衬线标题 + 陶土强调。
- 否决理由：与 AI 产品常见「奶油海报」同质，削弱 Flowy 工作站识别度，且与现有 ink 登录分栏冲突。

## Token 落点

- `ui/src/renderer/styles/flowy-visual-system.css` — 字阶、4/8 间距、圆角、表面、强调/状态色（浅/深语义一致）
- `ui/src/renderer/styles/theme-control-contract.css` — Button/Input/Select/Card/Tabs/Tooltip/Toast/Skeleton 状态
- 密度：`--flowy-density-rail|session|content-pad|workspace`
