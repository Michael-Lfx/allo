# 多端 Shell 策略（Windows PowerShell / Unix bash）

状态：已采纳（2026-11-17）
适用范围：`flowy-agent-core` 的宿主 shell 工具、`flowy-agent` 的生产工具注册。
桌面宿主与 web 后端都按**实际运行宿主**的 OS 选择，不在前端做平台判断。

## 决策

1. Windows 默认注册 `powershell` 工具，复用 pi-rust `rpi-tools` 的
   `create_powershell_tool`；不要求 Git Bash 或其他 POSIX bash。
2. macOS／Linux 默认注册 `bash` 工具，保持 pi-rust 的 POSIX 语义。
3. 平台选择放在 core 的一个入口 `pi_shell_definition(cwd)`，宿主只注册这一个
   shell，不同时暴露 bash 和 powershell。
4. 第一版不做启动探测、不加 `shell_path` 配置、不新增偏好项。

## 原因

1. pi-rust 的 bash 工具要求 POSIX bash：Windows 依赖 Git Bash 或可用的
   `bash.exe`；本机实测只有 DSH 沙箱 bash 与 WSL 占位程序，应用内 bash 调用
   返回 `Bash/E_ACCESSDENIED`，模型连续重试后只能报告失败。
2. Windows 原生 PowerShell 始终可用，且 pi-rust 已实现安全的脚本暂存、
   `-EncodedCommand`、执行策略与超时处理。
3. 探测方案需要超时、缓存与 UI 提示，还可能出现“探测通过、实际被宿主
   沙箱拒绝”的误判；按平台固定选择更确定。
4. 同时注册 bash 与 powershell 会让模型在失败后反复换工具；单一入口避免
   这类重试浪费。

## 实现

1. `flowy-agent-core/src/tools/pi.rs`
   - `pi_shell_definition(cwd)`：Windows → `pi_powershell_definition(cwd)`；
     其他平台 → `pi_bash_definition(cwd)`。
   - PowerShell 工具通过现有 `AgentTool → ToolDefinition` 适配器接入，
     registry policy、guard、取消、UI 投影保持不变。
2. `flowy-agent/src/handle.rs`
   - 生产 registry 只注册 `pi_shell_definition(cwd)`。
   - 文件工具与路径 guard 不变；shell 工具仍不参与文件路径 guard。
3. 工具描述
   - Windows 上覆盖为“使用 PowerShell 语法执行 Windows 命令”，避免沿用
     pi 原文“Prefer bash”导致模型误判；执行实现仍完全来自 pi-rust。
4. 运行时临时目录
   - 桌面端启动时在 `<app-data>/runtime-tmp` 创建应用私有临时目录，并把
     `TMP`／`TEMP`／`TMPDIR` 指向它；PowerShell staged script 不再依赖系统
     `%TEMP%`。
   - 目录创建或可写探针失败时保留系统临时目录，并记录 boot warning。

## 行为差异

1. Windows 历史里出现 `powershell` 工具调用；macOS/Linux 仍是 `bash`。
2. Windows 上即使安装了 Git Bash，也不默认提供 `bash`；后续可用
   `shell_path` 或显式 GitHub Bash 选项扩展。
3. 模型在 Windows 需要写 PowerShell 语法；工具描述会说明。
4. 安全边界与 bash 相同：都是宿主授权的命令执行；没有新增沙箱。

## 后续扩展（未做）

1. `FLOWY_SHELL_PATH` 或偏好项：为 Windows 高级用户恢复 Git Bash。
2. 启动探测 + 能力提示：在无 PowerShell 的极简 Windows 环境隐藏 shell。
3. 跨平台工具名归一：如需统一历史展示，可把 UI 行名统一为“Shell”。

## 验证

1. core 单测：平台选择返回正确工具名；适配器 content/details 映射。
2. Windows ignored 集成用例：`Write-Output 'ps-ok'` 真实执行。
3. Unix：保持 bash ignored 用例，编译检查通过。
4. 全量：core/agent 测试、workspace check、desktop build。
