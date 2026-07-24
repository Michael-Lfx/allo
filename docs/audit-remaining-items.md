# Flowy 项目审查 — 未完成项清单

> 审查日期: 2026-07-22
> 本文档记录审查中发现但尚未修复的问题，按严重等级排列。

## 已修复项（本次）

| 编号 | 严重度 | 问题 | 状态 |
|------|--------|------|------|
| C1 | Critical | localStorage 密码明文存储 | ✅ 已修复 |
| C2 | Critical | unsafe 裸指针绕过借用检查器 | ✅ 已修复 |
| H4 | High | 生产代码 expect/unwrap panic 风险 | ✅ 已修复 |
| H5 | High | browser-engine Mutex 中毒不容忍 | ✅ 已修复 |
| H6 | High | Preset 列表 N+1 查询 | ✅ 已修复 |
| M1 | Medium | 消息搜索前导通配符 LIKE 全表扫描 | ✅ 已修复 |
| M2 | Medium | 热路径消息克隆（分析后保留，添加注释） | ⚠️ 评估后保留 |
| M9 | Medium | context_window 硬编码 Claude 特定值 | ✅ 已修复 |
| M12 | Medium | 固定窗口限流边界突发 | ✅ 已修复 |

## Critical — 必须修复

### C3. 安全审批/出口功能未接线（fail-open）
- **位置**: `crates/agent/nomi-browser-engine/src/firewall.rs`、`evaluate.rs`、`download.rs`、`crates/agent/nomi-browser/src/tool.rs`
- **问题**: F1-sec TODO 群——出口防火墙 GatePost 当前放行+仅留痕，JS evaluate 授权未实时读取，下载红线判定未接入 facade，SecretStore 未接入。安全边界功能均为占位实现。
- **风险**: SSRF/数据外泄/未授权 evaluate 的拦截链路未闭环
- **建议**: 确认 F1 接线完成，或将占位实现改为 fail-closed（默认拒绝）

## High — 尽快修复

### H1. rehype-raw 无 sanitize — 条件性 XSS 通道
- **位置**: `ui/src/renderer/components/Markdown/index.tsx`（行 154）
- **问题**: `allowHtml` 启用时原始 HTML 直接注入 DOM，无 `rehype-sanitize`。当前用于渲染远程 release notes。
- **风险**: 若更新源被劫持即为远程 XSS
- **建议**: 追加 `rehype-sanitize`（放行 KaTeX 标签）或移除 allowHtml

### H2. ~30 个未使用依赖（含服务端/原生模块）
- **位置**: `ui/package.json`
- **问题**: express、grammy、jsonwebtoken、@napi-rs/canvas、electron-log 等完全未使用
- **风险**: 供应链攻击面扩大、安装时间膨胀
- **建议**: 一次性 `bun remove` 清理

### H3. 桌面端 CSP 被完全禁用
- **位置**: `apps/desktop/tauri.conf.json`（行 14-16）
- **问题**: `"csp": null`，webview 内无脚本/资源加载限制
- **风险**: 对可触达 shell/文件/agent 的高权限应用是纵深防御缺口
- **建议**: 配置明确 CSP（限制 script-src、connect-src 到本地 host）

### H7. jsonwebtoken v9/v10 版本分裂 + oauth2 使用 RC 版
- **位置**: `Cargo.toml`（行 118/193）、`crates/backend/nomifun-auth/Cargo.toml`
- **问题**: auth crate pin jsonwebtoken v9（不再收安全补丁），oauth2 用 5.0.0-rc.1
- **建议**: 计划迁移到 jsonwebtoken v10；跟踪 oauth2 正式版发布

## Medium — 建议近期处理

### M3. Vite 构建无 chunk 分割策略
- **位置**: `ui/vite.config.ts`（行 108-112）
- **问题**: 主 vendor chunk 1.8MB，无 manualChunks；dist 总计 34MB 嵌入桌面二进制
- **建议**: 添加 manualChunks 分离 react/arco/i18next；考虑 Arco 按需 CSS

### M4. 全量导入 Arco CSS + hljs 190 种语言
- **位置**: `ui/src/renderer/main.tsx`（行 28）、`MermaidBlock.tsx`
- **问题**: 全组件库样式 + 全部 hljs 语言定义一次性加载
- **建议**: Arco 按需样式；`react-syntax-highlighter/dist/esm/light` + 注册实际语言

### M5. 弱密码黑名单仅 5 条
- **位置**: `crates/backend/nomifun-auth/src/validation.rs`（行 9）
- **问题**: 远低于行业标准
- **建议**: 扩展到 top-100 常见密码 + 字符多样性要求

### M6. Deep-link scheme 三方不一致
- **位置**: `tauri.conf.json` vs `tauri.dev.conf.json` vs 代码注释/UI
- **问题**: 实际注册 `flowy`，文档/代码引用 `nomifun://`，外部集成链接无法唤起应用
- **建议**: 统一为 `nomifun`（生产）/ `nomifun-dev`（开发）

### M7. json_extract 查询无函数索引
- **位置**: `crates/backend/nomifun-db/src/repository/sqlite_conversation.rs`（行 920-957）
- **问题**: `json_extract(extra, '$.workspace')` 无法命中普通索引
- **建议**: 建 SQLite 表达式索引或将热字段提升为实体列

### M8. reqwest 0.12/0.13 + toml 0.8/0.9/1.0 版本分裂未文档化
- **位置**: `Cargo.lock`、根 `Cargo.toml`
- **问题**: tauri-plugin-updater 引入 reqwest 0.13，安全公告需分别跟踪
- **建议**: 在根 Cargo.toml 注释中补记来源；评估升级 workspace reqwest

### M10. API_KEY 环境变量优先于 provider 特定变量
- **位置**: `crates/agent/nomi-config/src/config.rs`（行 806-809）
- **问题**: `API_KEY` 优先于 `ANTHROPIC_API_KEY`/`OPENAI_API_KEY`，多 provider 环境易误用
- **建议**: 调整为 provider 特定变量优先

### M11. 未安装 cargo-audit，无自动 CVE 检查
- **问题**: 无法自动检测依赖链已知 CVE
- **建议**: CI 中接入 `cargo audit` 或 `cargo deny check advisories`

## Low — 日常迭代处理

| 问题 | 位置 | 建议 |
|------|------|------|
| ~13 处注释掉的死代码/TODO | Router.tsx、Sider/index.tsx 等 | feature flag 或删除 |
| .cargo/config.toml 全局劫持 crates.io 到中国镜像 | `.cargo/config.toml` | 反转默认，mirror 放入 config.local.toml |
| i18n 少量硬编码字符串（4 处） | DiffViewer、Markdown 组件 | 走 i18n 系统 |
| @types 包错放 dependencies | `ui/package.json` | 移至 devDependencies |
| tsconfig noUnusedLocals/Parameters: false | `ui/tsconfig.json` | 启用以检测死代码 |
| docker-compose 端口绑定 0.0.0.0 | `docker-compose.yml` | 改为 127.0.0.1 |
| Service Worker 缓存无容量上限 | `ui/public/sw.js` | 添加 LRU 淘汰策略 |
| imageGenCore.ts 死代码文件 | `ui/src/common/chat/imageGenCore.ts` | 删除文件及 @napi-rs/canvas 依赖 |
| vite.config.ts 过时 Electron 注释 | `ui/vite.config.ts` | 删除过时注释 |
