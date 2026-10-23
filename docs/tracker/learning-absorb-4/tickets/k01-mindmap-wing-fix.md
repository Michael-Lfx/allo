---
id: k01
title: "Mindmap 负坐标翼裁剪修复——两翼中心对称 + dy 补偿 + 布局单测"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# k01: Mindmap 负坐标翼裁剪修复

**What to build:** 结构图 Tab 思维导图的一整翼当前被 SVG 视口裁掉（不可见），
中心主题框还压住另一翼首列。照 learnhub a928e46 的修法改两翼中心对称布局，
文本基线改引擎无关 dy 补偿，并抽出纯函数布局配单测锁死「所有节点必须在画布内」。

## allo 现状（bug 实锤）

`ui/src/renderer/pages/learning/components/structure/Mindmap.tsx`：

- `placeSide(-1, right)`（:83）把右翼放在**负坐标区**（spineX=-250、块 x=-520）；
  `<svg>` 无 viewBox、无 transform 平移（:101）——**负坐标整翼被视口裁剪，
  主干 ≥2 时有一翼完全不可见**。
- 中心框 `centerX±90`（:104-112，centerX=width/2=615）与 +侧翼首列块
  （x=445..595，首块 y 39..65）重叠——同 learnhub「中心主题框横向压翼首列块」。
- 三处 `dominantBaseline='central'`（:117/:173/:200）——learnhub 实测该属性
  在其渲染引擎不生效，基线骑框；改 dy 补偿。

## learnhub 参考（a928e46，无票真机走查修复）

`src/pages/course-detail/ui/structure/mindmap.tsx`：

- 引入 `CENTER_X = BLOCK_X + BLOCK_W/2 + 40`；翼坐标全部 `CENTER_X + side*SPINE_X`、
  `CENTER_X + side*BLOCK_X`，`width = CENTER_X*2`，主题框/文本锚 `CENTER_X`——
  两翼中心对称、全部落画布内。
- `dominantBaseline` 全部替换为 `dy="0.35em"`（引擎无关）。
- 徽标随翼镜像 `cx = b.x + side*(BLOCK_W/2 - 18)`（allo 版无徽标，不适用）。

## 实施要点

- [ ] 布局计算抽成纯函数模块（如 `structure/mindmapLayout.ts`，导出
      `computeLayout(structure): { groups, width, height }`），`Mindmap.tsx`
      只消费；颜色注入保持现状（palette 回调传入或返回侧并行）。
- [ ] 坐标改 `CENTER_X + side*SPINE_X / CENTER_X + side*BLOCK_X` 中心对称；
      贝塞尔边锚点同步（先例：learnhub `M CENTER_X 40 C …`）。
- [ ] 三处 `dominantBaseline='central'` → 删属性、加 `dy='0.35em'`。
- [ ] 点击选块（BlockDetail）行为不变；两主题（light/dark）走查。
- [ ] i18n 零新键（纯几何修复）；`bun run typecheck` + 相关 check 门过。

## 验收

- **布局单测**（bun test，`.test.ts`，先例 `rulesOfHooks.test.ts`）：构造
  ≥2 主干（两翼非空）与 1 主干（单翼）两种 fixture，断言：
  - 每个 spine rect / block rect / 中心框都在 `[0,width]×[0,height]` 内；
  - 左翼 max_x < 中心框 min_x，右翼 min_x > 中心框 max_x（不压列）。
- **双臂对照（版本臂）**：修复前 tip 与修复后各跑一次桌面端（或 dev:ui 带结构
  数据），同一课程（≥2 主干）截图并排，确认修复前缺一翼、修复后两翼齐——
  对照卡落 Resolution（截图路径或文字实录）。
- `bun run typecheck` 净增零错误；学习面既有测试不受影响。

## 非目标

- 不动 dagre 图谱/路线图/矩阵组件（若同 bug 另行确认再开票）。
- 不加徽标、不加缩放平移。

## 备注

本票是纯前端几何修复，无 LLM 面，不配环境开关；「双臂」取修复前后版本对照。
