# Windows：开发循环无法写入仓库以外的目录（Low 完整性标签）

这是一个**机器层面**的故障，会让 `bun run dev` 看起来像代码坏了，实际与仓库无关。
2026 年 11 月在一台 Windows 11 工作站上实际遇到，记录在此，避免下一个人再花一整天。

## 症状

开发循环在三个不同层次失败，全是"拒绝访问"类错误；如果窗口能走到创建那一步，会闪现后退出：

| 层次 | 报错 |
| --- | --- |
| C/C++ 构建 | `LINK : fatal error LNK1104: 无法打开文件 "...\Temp\lnk{GUID}.tmp"` 或 `cl : error D8050: 无法执行 c1.dll` |
| 后端启动 | `thread 'nomifun-backend' panicked at .../bootstrap/tracing_init.rs: failed to create backend log file appender: ... Os { code: 5, kind: PermissionDenied }` |
| Tauri 窗口 | `ERROR tauri_runtime_wry: failed to create webview: WebView2 error: WindowsError(Error { code: HRESULT(0x8000FFFF) })`（profile 不存在）或 `HRESULT(0x800700AA)`（profile 已存在） |

## 根因

检出目录被打了 Windows 的**低完整性（Low mandatory integrity）标签**：

```powershell
icacls "D:\path\to\repo" | Select-String Mandatory
# Mandatory Label\Low Mandatory Level:(OI)(CI)(NW)
```

位于被标记目录树中的可执行文件会继承该标签，而**从 Low 标签镜像启动的进程会以
Low 完整性级别运行**。用会打印自身令牌的探针（`whoami /groups`）实测：

| 同一份二进制 | 进程完整性级别 |
| --- | --- |
| 放在仓库内 | `Low Mandatory Level`（S-1-16-4096） |
| 复制到仓库外 | `Medium Mandatory Level`（S-1-16-8192） |

Windows 的强制完整性控制（MIC）禁止"低向高写"。于是 Low 进程既写不了 `%TEMP%`，
也写不了按用户数据目录和 `%LOCALAPPDATA%` 下的 WebView2 profile —— 但写**被标记的
目录树内部**仍然正常。这一条规则同时解释了上面三个症状：链接器建不了临时文件、
后端建不了日志文件、WebView 运行时初始化不了 profile。

这不是本仓库造成的，未被标记的检出完全正常。

## 一条命令确认

```powershell
icacls "D:\path\to\repo\build.noindex\debug\deps\Flowy.exe" | Select-String Mandatory
```

- 显示 `Medium` → 不是这个原因，继续排查其他方向；
- 显示 `Low` → 确认命中（`icacls` 查仓库根目录可以看出标签是在哪一层被设上的）。

当时排查中**被排除**、且停下后故障依旧的项：停 `CodexSandboxService`、结束全部
桌面 AI 客户端进程、停杀软实时防护驱动。标签是持久化的 NTFS 状态，不是运行中的
进程，所以重启机器或关掉肇事程序都不会恢复。

## 修复

在**管理员** PowerShell 中执行：

```powershell
icacls "<repo>" /setintegritylevel Medium /T /C /Q
```

`/T` 会遍历整棵树；在 `build.noindex` 已填充（约 10 万文件）的检出上约需两分半，
命令是幂等的。

验证：

```powershell
icacls "<repo>\build.noindex\debug\deps\Flowy.exe" | Select-String Mandatory   # Medium
icacls "<repo>" | Select-String Mandatory                                      # Medium
```

之后 `bun run dev` 恢复正常，后端重新使用默认的按用户 dev 数据目录
（Windows 上是 `%LOCALAPPDATA%\Flowy\Nomi-dev`），WebView2 profile 回到
`%LOCALAPPDATA%\<identifier>`。

回滚（不推荐，仅当标签是有意设置的）：

```powershell
icacls "<repo>" /setintegritylevel Low /T /C /Q
```

## 预防

标签是某个把该检出当作 `workspace-write` 工作区的桌面 AI/沙箱工具打上的：它把整个
目录树标成 Low，使工具启动的进程只能写工作区内。**工具关闭后标签仍留在 NTFS 上**，
所以之后即使没有进程在运行，开发循环照样坏。

- 不要把该检出作为这类工具的沙箱工作区打开。若已登记，从它的工作区列表里移除，
  或改用被标记路径之外的检出。
- 这类工具升级/重装后，重跑上面那条 `icacls` 自检；若标签回来了，重复修复步骤。
- **不要**用"把 `TEMP`、`NOMIFUN_DATA_DIR`、`WEBVIEW2_USER_DATA_FOLDER` 挪进仓库"
  来绕过：那只是隐藏症状，会搬走所有开发者的 dev 数据，并且让该检出上运行的其它
  流程（测试、打包、`nomicore`）继续坏着。
