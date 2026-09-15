# 模型选择器升级执行计划:品牌图标 + 定位语 + 推荐置顶

## 交接状态

- 日期:2026-09-15
- 工作分支:`feat/chat-model-picker-showcase`(从 main `241c3023a` 切出,含已合并的图片发送松绑 PR #217)
- 当前状态:仅含本计划文档;实施未开始,后续修改全部在此分支进行
- Git 约束:commit 无 AI 署名;hooks 不绕过;回退 = 整分支删除

## 背景与目标

聊天模型选择器当前每行只有 名称 + 倍率,用户无法感知模型能力差异与定位。目标形态(参考截图):每行 = 品牌图标 + 名称 + 一句定位语 + 倍率;指定推荐模型 **deepseek-v4.1-flash**(性价比 + 视觉 + 能力均衡)置顶云端模型组并带「推荐」徽章。

**不做的事**(用户已拍板):不做免费/Beta 徽章;不做"套餐内/积分计费"分组 Tab;现有"自动模型/云端模型"分组保留。

## 事实基础(三路探索 + 本地库实证)

### 数据链现状

- 模型目录来自远端 catalog(`GET /api/v2/model/availableListClaw?category=1`),本地后端 `provider_sync.rs` 同步落库 `provider_models`;
- 名称:`provider_models.description` 列存显示名(**已被 label 占用,定位语不可复用此列**);
- 倍率:`params._flowy_catalog_credit_rate`;
- wire 上有 `icon` 字段,媒体目录(category 4/6/8)已消费,**聊天目录是否填值未确认**;本地同步从未持久化 icon;
- 前端 `syncOcModels.ts:73-74` 已在读 `params.icon ?? params._flowy_catalog_icon`(视频画布侧),后端从未写入——前向兼容点已存在;
- 内置品牌 logo 现货:`crates/backend/nomifun-assets/assets/logos/`(deepseek.svg、zhipu.svg、kimi.svg、minimax.png、qwen.svg 等),经 `/api/assets/logos/**` 路由提供,前端 `resolveBackendAssetUrl` 解析;`videoCanvas/lib/catalogIcon.ts` 已有"服务端 icon 优先 + 内置品牌兜底 + onError 降级 + dark:invert"完整范式。

### 在架模型清单(读自本地同步库,2026-09-15)

| 规范 id(剥 `AIPC-` 小写) | 倍率 | 视觉 | 环境差异 |
|---|---|---|---|
| auto-intelligence / balance / cost | — | — | description 已存 智能/平衡/经济 |
| deepseek-v4-flash | ×0.5 | ✗ | |
| deepseek-v4-pro | ×1.5 | ✗ | |
| **deepseek-v4.1-flash(推荐)** | ×0.5 | ✓ | **仅 dev 库有** |
| deepseek-v4-flash-vision-exp | ×0.5 | ✓ | **仅 prod 库有**,前缀变体 |
| glm-5 | ×1 | ✗ | |
| glm-5.2 | ×1.6 | ✗ | id 含 `.` |
| kimi-k2.5 / kimi-k2.6 | ×1 / ×1.4 | ✓ | 原始 id 混合大小写 `AIPC-Kimi-K2.5` |
| minimax-m2.7 / minimax-m3 | ×0.5 / ×0.8 | ✗ / ✓ | |
| qwen3.7-plus / qwen3.7-max | ×1.3 / ×4 | ✓ | |
| qwen3.8-flash | ×0.14 | ✓ | |

**关键风险:prod/dev 目录漂移**。推荐模型 v4.1-flash 目前只在 dev 目录——置顶逻辑必须在"目录中不存在该模型"时静默不生效(不报错、不留空位),这正是降级设计的用例。

## 架构设计

```
icon 解析(两级兜底,纯函数):
  规范化 id → 注册表精确/前缀命中品牌 → 内置 /api/assets/logos/ai-* 资产
    → 空(行样式回退现状,只显示名称+倍率)

tagline 解析(两级继承):
  规范化 id → 注册表精确命中 → I18nKey → t()
    → 最长前缀继承(vision-exp 变体继承基座型号文案)
      → 空(不渲染第二行)

推荐置顶:
  注册表 recommended: true → buildChatModelPickerViewModel 稳定排序
  将该模型移到 cloudModels 组首;目录不存在该 id 时无操作
```

新模型上架四层防线:① 无注册项 → 回退旧行样式;② 品牌前缀命中 → 至少有图标;③ dev 环境 console.warn 暴露未注册模型;④ 长期推动 catalog 下发 tagline/icon,解析器加"服务端优先"一级即可前向兼容。

## 改动清单(第一期,纯前端)

### Step 1 — 共享图标逻辑上移动

- 新建 `ui/src/renderer/utils/model/modelLogos.ts`:从 `ui/src/renderer/pages/videoCanvas/lib/catalogIcon.ts` 上移 `resolveModelFallbackIcon`、`isMonochromeLogo`、`logoAsset`(`resolveBackendAssetUrl` 封装);
- `catalogIcon.ts` 改为 re-export 保持 videoCanvas 消费方与 `catalogIcon.test.ts` 不断;或更新引用(二选一,实施时以改动最小为准)。

### Step 2 — modelShowcase 注册表 + 单测

新建 `ui/src/renderer/utils/model/modelShowcase.ts`:

```ts
interface ModelShowcaseEntry {
  taglineKey?: I18nKey;      // 编译期校验键存在(仓内 Record<X, I18nKey> 主流模式)
  recommended?: boolean;
}
// 键 = 规范化 id(剥 AIPC-/flowy/ 前缀、小写)
// i18n 键段 slug 规则:`.` → `-`(GLM-5.2 → glm-5-2),`-`/camelCase 合法
export const MODEL_SHOWCASE: Record<string, ModelShowcaseEntry> = { ... };
export function normalizeShowcaseModelId(model: string): string;
export function resolveModelShowcase(model: string): {
  icon: string;             // 内置资产 URL 或 ''
  taglineKey?: I18nKey;     // 精确 → 最长前缀继承
  recommended: boolean;
};
```

- 图标:先精确,再按品牌关键字(deepseek/glm/zhipu/kimi/moonshot/minimax/qwen…)走 `resolveModelFallbackIcon`;
- 注册项含 `deepseek-v4.1-flash: { taglineKey, recommended: true }`;
- 新建 `modelShowcase.test.ts`:规范化(AIPC-/flowy/ 前缀、混合大小写 Kimi-K2.5、含点 glm-5.2)、精确/前缀/未命中三级、recommended 标记、所有 taglineKey 非空校验。

### Step 3 — 视图模型投影 + 推荐排序

`ui/src/renderer/utils/model/chatModelPicker.ts`:
- `ChatModelOption` 增加 `showcase: { icon: string; taglineKey?: I18nKey; recommended: boolean }`(在 `modelOption()` 内调 `resolveModelShowcase` 填充,纯函数无 t() 依赖);
- `buildChatModelPickerViewModel`:cloudModels 收集后做稳定排序,recommended 项移到组首(仅当存在);
- `chatModelPicker.test.ts`:新增 showcase 投影断言 + 推荐置顶排序断言(含"推荐模型不在目录时排序不变"用例)。

### Step 4 — 桌面菜单行改造(一处改动,三入口覆盖)

`ui/src/renderer/components/model/ChatModelPickerMenu.tsx` 的 `modelRow`:
- 左侧 20px 圆角图标 `<img>`(参照 videoCanvas `model-picker.tsx` 的 ModelIcon:`onError` 时 src 置空 → 不渲染图标;`dark:invert` 仅对 `isMonochromeLogo` 命中者;`referrerPolicy='no-referrer'`);
- 中间双行:名称行 + 定位语行(`taglineKey` 存在才渲染,`t(taglineKey)`);
- 右侧保留 `ModelCreditRateHint`;recommended 项在倍率左侧加「推荐」chip(i18n 键 `conversation.modelPicker.recommended`,`data-testid='chat-model-option-recommended-badge'`);
- auto 行:三档位定位语经同一注册表注入(键 `auto-intelligence` 等),版式与 cloud 行对齐;
- **必须保住结构测试锁定字面量**:`title={option.model}`、`aria-label={fullLabel}`、`min-w-0 flex-1 truncate`、`chat-model-picker-menu-meta`;禁用子串 `search`;auto 行不加 `›`。

`ui/src/renderer/components/chat/SendBox/sendbox.css`(855-881):
- `.chat-model-picker-menu-item` 固定 `height: 40px` → `min-height`(建议 52-56px)放行双行;新增图标/tagline/徽章样式;
- **不可动** `max-height: min(360px, ...)` 字面量(结构测试锁定)。

覆盖说明:`NomiModelSelector`/`GuidModelSelector` 均复用本组件,桌面端无需另行改动。

### Step 5 — 移动端 sheet(同一分支内跟进)

- `MobileActionSheet/types.ts`:`MobileActionSheetOption` 加 `icon?: ReactNode`(label/description 已是 ReactNode,无反向断言);
- `MobileActionSheet.tsx` `renderSubmenuOption`:渲染 icon 槽位(保留 `role='button'`、`aria-pressed`、`data-testid`);
- `MobileActionSheet.module.css`:submenu option 图标样式;
- `NomiSendBox.tsx` `toMobileModelOption`:description 排布改为 `tagline ? \`${tagline} · ${rate}\` : 现状`(供应商名让位给定位语,品牌由图标表达);recommended 在 label ReactNode 内加 chip;**禁止**命名 `const modelOptions: MobileActionSheetOption[]`(结构测试反向锁定,现有 `modelGroups` 命名保留)。

### Step 6 — i18n

zh-CN 与 en-US 的 `conversation.json` 的 `modelPicker` 组内新增(键段已 slug 化):

```jsonc
"tagline": {
  "auto-intelligence": "最聪明 · 复杂任务",   // en: "Smartest, for hard tasks"
  "auto-balance":      "省心均衡 · 日常推荐",   // en: "Balanced, everyday pick"
  "auto-cost":         "经济省积分",           // en: "Most economical"
  "deepseek-v4-flash": "轻快应答 · 高性价比",   // en: "Fast & light, great value"
  "deepseek-v4-pro":   "深度推理 · 复杂写作",   // en: "Deep reasoning & writing"
  "deepseek-v4-1-flash": "高性价比 · 支持视觉", // en: "Great value, with vision"  ← 推荐
  "glm-5":             "均衡通用",             // en: "Well-rounded"
  "glm-5-2":           "旗舰通用 · 长上下文",   // en: "Flagship, long context"
  "kimi-k2-5":         "视觉理解 · 长文档",     // en: "Vision & long docs"
  "kimi-k2-6":         "进阶视觉推理",         // en: "Advanced vision"
  "minimax-m2-7":      "经济实用",             // en: "Budget-friendly"
  "minimax-m3":        "多模态全能",           // en: "Multimodal all-rounder"
  "qwen3-7-plus":      "全能视觉",             // en: "Versatile vision"
  "qwen3-7-max":       "顶配性能",             // en: "Top-tier performance"
  "qwen3-8-flash":     "极致性价比 · 支持图片"  // en: "Ultra value, with vision"
},
"recommended": "推荐"  // en: "Recommended"
```

文案为草稿(依据倍率 + traits 定位,非官方宣称),产品可随时改 locale JSON 单独发版更新。操作序列:双语 JSON → `bun run gen:i18n` → `bun run check:i18n` → `bun run typecheck`(注册表 `I18nKey` 字面量校验生效);仿 `sshLocales.test.ts` 新增 tagline 组双语言对称测试。

### Step 7 — 测试收尾

- `ChatModelPickerMenu.structure.test.ts`:新增正向锁定(图标 img、recommended badge testid、tagline 渲染),保留既有全部锁定;
- `modelShowcase.test.ts`(Step 2)、`chatModelPicker.test.ts`(Step 3);
- 若 Step 1 动了 videoCanvas 引用路径:跟进 `catalogIcon.test.ts` / `syncOcModels.test.ts`;
- 全量门禁:typecheck + 相关测试文件单跑 + check:i18n;全量 `test:ui` 的 segfault 与 3 个预存失败按基线记忆判定,不当回归。

## Commit 拆分策略

1. `refactor(ui): 上移模型品牌图标兜底逻辑到共享 utils`(Step 1,纯搬迁,videoCanvas 行为不变)
2. `feat(ui): 模型选择器增加品牌图标、定位语与推荐置顶`(Step 2-4 + 6 中桌面相关键)
3. `feat(ui): 移动端模型 sheet 支持图标与定位语`(Step 5)
4. `test(ui): 选择器展示层结构测试与 i18n 对称测试`(Step 7,若未随前序提交)

每个 commit 自洽:相关测试 + typecheck 通过后再提下一个。

## 验证清单

自动化:上述每步验证 + 零残留 grep(`resolveModelShowcase` 引用点齐全、无遗留旧 import 路径)。

实机(dev 环境,含 v4.1-flash):
1. 桌面菜单:每行 图标+名称+定位语+倍率;deepseek-v4.1-flash 位于云端组首位且带「推荐」徽章;
2. Auto 行:三档位定位语展示;切换 tier 正常;
3. 中英切换:定位语双语正确;
4. 暗色主题:图标显示正常(openai/xai 类单色 logo 不在当前目录,invert 逻辑仅防御);
5. 移动端 sheet:图标+定位语排布不破版;
6. **prod 环境回归**:prod 目录无 v4.1-flash,确认选择器正常、无推荐徽章、无空白行(降级生效);
7. Guid 首页选择器(复用组件,确认渲染一致)。

## 残留风险与跟进项

- 推荐模型 id 与 prod 目录漂移:若 prod 长期不上 v4.1-flash,推荐置顶在 prod 永不出现——需要产品与 catalog 发布节奏对齐;
- 文案为草稿,以产品定稿为准;
- **第二期(不在本分支)**:`provider_sync.rs` 持久化 `_flowy_catalog_icon`(nomifun-db provider.rs 常量+seed 字段+`initial_catalog_params`/`merge_catalog_params` 分支、sqlite_provider.rs 调用点、provider_sync 填充,约 30 行)+ 诊断日志加 `icon_present` 字段实机确认聊天目录 icon 是否下发;前端解析器随后加"服务端 icon 优先"一级,videoCanvas 聊天模型图标同源受益。
