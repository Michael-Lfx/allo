# bsk-2-02 验证剧本：动效降级与双主题兼容 (Reduced Motion & Theme Dual Verification)

> **对应波次**：Wave 2 (Phase 2 / Gate 2)
> **任务卡**：T2-08
> **上游契约**：[ui-motion.zh.md](../ui-motion.zh.md) §2 与 [wave-2-decision-tts.zh.md](wave-2-decision-tts.zh.md) §4
> **验收映射**：TC-AM-017 / Gate 2

---

## 1. 目标与范围

验证桌面端 SPA 设置面板中“语音伴侣”详情页在无障碍减弱动效及双主题下的合规性与表现力：
1. **Reduced Motion 即时降级**：通过 Chrome DevTools Protocol (CDP) 模拟操作系统级 `prefers-reduced-motion: reduce` 设置；在减弱动效开启时，所有状态过渡、就绪图标缩放与发丝轨道推进即时切换（0ms 过渡，无位移），且全部文本与语义完整保留，不影响任何功能交互；
2. **双主题及全预设主题兼容**：在 Light（如 `codex-neutral`）与 Dark（如 `rhythm-dark`）以及其他内置主题下，页面完全使用语义设计令牌，文本、背景、边框、状态图标在各个主题下对比度符合 WCAG AAA 标准（正文 ≥ 4.5:1，图标 ≥ 3:1）；
3. **自动化门禁闭环**：通过 `bun run check:theme` 与 `bun run check:i18n` 严格检验。

---

## 2. 前置条件

1. 启动 WebUI 开发服务：`bun run dev:web`（`http://localhost:5173`）；
2. 自动化测试环境或 Chromium 浏览器支持 CDP 调试协议；
3. 导航至 `#/settings/voice`。

---

## 3. 执行步骤 (Steps)

### 场景 A：Reduced Motion 减弱动效验证

| 步序 | 操作 | 预期现象 |
| :--- | :--- | :--- |
| 1 | 打开 Chromium 开发者工具，通过 `Emulation.setEmulatedMedia` 开启 `prefers-reduced-motion: reduce` | 页面无刷新即时响应媒体查询变更 |
| 2 | 触发安装流程，观察 `NotInstalled → ProbingSource → Downloading → VerifyingSha256` 状态变迁 | 状态文本即时刷新；发丝轨道推进即时呈现目标位置，无缓动过渡动画（`transition: none`） |
| 3 | 推进至 `Ready` 终态 | `CheckOne` 图标直接呈现最终尺寸与状态，无 240ms 缩放微动效；右侧开关与按钮立即可用 |
| 4 | 点击【释放空间】触发两步确认 | 按钮文本原地即时切换为【确认释放？】，无任何过渡动画 |
| 5 | 关闭 `prefers-reduced-motion` 恢复正常动效 | 页面即刻恢复 180ms/240ms 动效过渡能力 |

### 场景 B：双主题与全预设主题验证

| 步序 | 操作 | 预期现象 |
| :--- | :--- | :--- |
| 1 | 在设置或调试栏切换主题为浅色系（`codex-neutral`） | 页面背景平滑适配浅色底，文本呈深灰/黑（`text-t-primary`），就绪绿色与错误红色高对比清晰可辨 |
| 2 | 切换主题为深色系（`rhythm-dark`） | 页面背景平滑适配深色底，文本呈浅色（`text-t-primary`），发丝轨道与边框对比度合规 |
| 3 | 检查全部 7 种状态下的组件渲染 | 行容器边框、按钮悬停态、文本提示在深浅模式下均无对比度缺失或刺眼反差 |
| 4 | 运行自动化脚本 `bun run check:theme` | 全部主题合同通过 |

---

## 4. 机械断言口径 (Assertions)

1. **Reduced Motion 计算样式断言 (Computed Style Assertions)**：
   - 当 `window.matchMedia('(prefers-reduced-motion: reduce)').matches === true` 时：
     - 就绪图标 `getComputedStyle(icon).transitionDuration === '0s'` 或 `transitionProperty === 'none'`；
     - 就绪图标 `getComputedStyle(icon).transform` 严格等于无位移的静止矩阵（`matrix(1, 0, 0, 1, 0, 0)` 或 `none`）；
     - 发丝轨道容器与内部填充条的 `transition` 属性为 `none` 或 `0s`；
     - 所有操作按钮（下载、启用开关、两步确认）输入即时响应，无动画阻塞。
2. **主题设计令牌断言 (Theme Design Token Assertions)**：
   - 检查 `VoiceSettings.tsx` 源码与渲染产物：
     - 不包含硬编码颜色值（如 `#ffffff`、`#000000`、`rgb(...)` 等非令牌属性）；
     - 所有颜色引用均来自 Arco 语义令牌或 Flowy 变量（如 `text-primary-6`、`text-danger-6`、`bg-fill-2`、`text-t-primary`、`text-t-secondary`）；
     - 运行 `bun run check:theme` 返回状态码 `0`。
3. **i18n 完整度断言**：
   - 运行 `bun run check:i18n` 返回状态码 `0`，`zh-CN` 与 `en-US` 键值 100% 对齐。

---

## 5. 通过标准 (Pass Criteria)

- 全部 3 项机械断言 100% 通过；
- CDP 减弱动效切换即时生效，无遗漏的持续动画或位移动画；
- `bun run check:theme` 与 `bun run check:i18n` 自动化门禁全部绿灯；
- 双主题视觉截图对比正常，符合 Quiet Kinetic 高品质视觉标准。
