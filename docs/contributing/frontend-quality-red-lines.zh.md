# 前端开发质量红线与历史教训 (Frontend Quality Red Lines & Lessons Learned)

> **定位**：Flowy 跨端前端（`ui/` 与 `web/`）核心质量约束、历史踩坑复盘与机检规则。  
> **一句话原则**：**“零 Mock 数据入产线，双语国际化零漏翻，双主题完整适配零死 CSS。”**

---

## 1. 核心红线一：生产运行时零 Mock 污染 (Zero Mock Contamination)

### 1.1 历史教训与事故复盘 (The Issue)
在过往迭代中，曾有开发者为在本地快速预览特定 UI 状态，而在生产级 Context（如 `BillingContext.tsx`、`CreditsContext.tsx`、`AuthContext.tsx`）或通用 Hook 中硬编码 Mock 资产余额、伪造账户状态或旁路 Token。此类临时调试代码一旦疏忽混入主干并打入 Release 构建，将严重污染真实用户资产视图甚至引发越权事故。

### 1.2 绝对规则 (The Rule)
- **严禁生产代码掺杂测试数据**：坚决禁止在生产 Context（`*Context.tsx`）、自定义 Hook、状态机或运行时逻辑中硬编码 Mock 余额、假账号、Bypass 凭据或测试兜底状态；
- **预览隔离机制**：UI 状态预览必须使用完全隔离的 Sandbox 文件、独立测试页（Test Harness）或临时 HTML 构件，绝不允许作为线上代码的回退分支（Fallback）。

### 1.3 提交与审查边界 (The Boundary)
对核心计费（Billing）、额度（Credits）、资产与认证（Auth）上下文的任何变更，必须保持最小颗粒度并在**独立的专用 PR** 中提交审查，严禁夹带在通用的 UI 样式调整或业务功能 PR 中混淆视听。

---

## 2. 核心红线二：全量 i18n 覆盖与零回退泄漏 (Full i18n Coverage & Zero Fallback Leakage)

### 2.1 历史教训与事故复盘 (The Issue)
开发过程中偶见使用 `t('key', { defaultValue: '中文' })`，但遗漏在 `en-US.json`（或 `zh-CN.json`）中声明对应键。在缺失该键的语言环境下，i18next 会静默回退至 `defaultValue`，导致英文用户界面中突兀地泄漏未翻译的中文文本。

### 2.2 绝对规则 (The Rule)
- **对称声明铁律**：所有用户可见文本——包括按钮文字、标签（Pill/Badge）、气泡提示（Tooltip/Popover）、表单占位符、ARIA 标签、窗口标题及错误 Toast，**必须对称声明在 `zh-CN.json` 与 `en-US.json` 两份语言字典中**（`ui/src/renderer/services/i18n/locales/`）；
- **禁止单方占位**：严禁在缺少对应英文翻译的情况下将中文直接作为单向 Default Value 发布。

### 2.3 验证与门禁步骤 (Verification)
1. 运行 `bun run gen:i18n` 自动提取并刷新类型定义 `i18n-keys.d.ts`；
2. 运行 `bun run check:i18n` 执行双语对称性与漏翻静态检查；
3. 为新增 UI 编写单元测试时，必须同时断言中英两套语言环境的文案渲染。

---

## 3. 核心红线三：双主题（浅色/深色）兼容与 CSS 完整性 (Dual-Theme Compatibility & CSS Completeness)

### 3.1 历史教训与事故复盘 (The Issue)
- **硬编码绝对颜色**：在样式中使用绝对色值（如 `#ffffff`, `#000000`），在用户切换深色/浅色主题时导致文字与背景同色（隐形字）或对比度极低；
- **残缺的 UnoCSS 边框类**：仅声明方向边框宽度类（如 `border-t`、`border-b`）而漏写样式类（如 `border-t-solid`），导致某些浏览器中边框不渲染，同时触发工程门禁警告。

### 3.2 绝对规则 (The Rule)
- **语义化设计令牌**：每一个 UI 组件必须优雅适配 Light 和 Dark 两种模式。统一使用语义化设计令牌（如 `text-t-primary`, `text-t-secondary`, `bg-fill-1`, `var(--border-base)`, `var(--flowy-attention)`）；
- **双模对比度保障**：允许创新视觉与鲜艳点缀色，但必须以主题感知的变量呈现（如定义新 Theme Token 或带有安全 Fallback 的 `var()`）；纯色仅在两个主题下均具备高对比度时可用（如彩色实体按钮上的白色文字）；
- **边框样式完整声明**：使用 UnoCSS 边框宽度工具类时，必须紧跟显式样式类（例如 `border-t border-t-solid border-t-border-base`）。

### 3.3 验证与门禁步骤 (Verification)
1. 视觉双向走查：在应用中手动切换 Light 与 Dark 模式各巡检一次；
2. 主题契约检查：运行 `bun run check:theme`；
3. 死 CSS 工具类检查：运行 `bun run check:dead-css` 与 `bun run check`，确保零 Warning 退出。
