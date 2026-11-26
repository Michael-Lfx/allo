# Windows 构建 / rebase / 验证踩坑记录

状态：2026-11-17 实测通过（Windows）；macOS / Linux 本轮未验证。
适用：`flowy18_wip` 嵌套 Cargo 工作区（GPUI 桌面端）在 Windows 上 rebase 最新 main、编译、跑测试。
本文只记录环境与操作，不改变产品行为；配套的测试失败分析见 [session-index-storage-error-test.md](session-index-storage-error-test.md)。

## 一、Git：拉取与 rebase

本机默认 `git`（niugit）的 schannel 会报
`schannel: AcquireCredentialsHandle failed: SEC_E_NO_CREDENTIALS`，走 `127.0.0.1:7890` 代理还会
`TLS connect error`。**只读拉取用真实 Git + OpenSSL + 不走代理**：

```bash
"/d/Git/cmd/git.exe" -c http.sslBackend=openssl -c http.proxy= -c https.proxy= \
  fetch origin --prune
```

注意：

- 工作区沙箱默认禁止写 `.git`（`touch .git/FETCH_HEAD` 会 Permission denied）；fetch / rebase / branch 需要放开 `.git` 写权限。
- rebase 前必须留回滚点，再冻结未提交改动：

```bash
git branch backup/<分支名>-pre-main-rebase-<日期> HEAD
git stash push -u -m "pre-main-rebase wip <日期>"
git rebase origin/main
```

- `git stash pop` 冲突时 stash 条目会保留，解析完成后按需 `git stash drop`。
- 本次结果：4 个提交重放到 `origin/main 36349baa3`；只有第 1 个提交（云端模型目录）冲突，
  涉及 `crates/flowy-app/src/app/mod.rs` 与 `crates/flowy-studio/src/{composer,panel,studio}.rs`；
  后 3 个提交自动应用；wip 恢复时只有 `panel.rs` 一处冲突（`StudioConfig` 内 `LlmConfig` 字面量需补 `..LlmConfig::default()`）。
- 解决策略：**以 main 的新 UI 骨架（`UiContext` / `StudioConfig` / `ChatComposer`）为准**，
  把分支功能移植上去；不要整文件取分支版本，否则会把 main 的 UI 重构回滚掉。

## 二、依赖与 registry

1. 本机 HTTPS 到 crates.io 不可用（同上的 schannel 问题）。仓库根 `.cargo/config.toml` 把
   crates-io 换成 `sparse+https://rsproxy.cn/index/`；`flowy18_wip/.cargo/config.toml`
   （本地文件，不进 git）又换成 `sparse+http://127.0.0.1:18778/index/`。后者有时不在线。
2. 离线解析（`--offline`）要求索引和 `.crate` 缓存都在。缺条目时可从 rsproxy 缓存补，不必改仓库配置：

```bash
# 索引（sparse cache）
cp ~/.cargo/registry/index/rsproxy.cn-*/.cache/<前缀>/<crate> \
   ~/.cargo/registry/index/127.0.0.1-*/.cache/<前缀>/<crate>
# 缺 .crate 时用纯 HTTP 下载（rsproxy 支持 http）
curl -sSL -f -o ~/.cargo/registry/cache/127.0.0.1-*/<name>-<ver>.crate \
  http://rsproxy.cn/api/v1/crates/<name>/<ver>/download
```

3. **registry source 身份会影响产物复用**：cargo 把 `rsproxy-sparse` 与 `local-http-mirror`
   视为不同 source，`build.noindex` 里的指纹不能共用，切换会触发大面积重编。
   固定用同一份本地 config，或命令里显式统一：

```bash
cargo ... --config 'source.crates-io.replace-with="rsproxy-sparse"'
```

4. `flowy18_wip/.cargo/config.toml` 是**本机私有**文件：新机器（含 macOS 同事）没有它，
   也不应该提交。构建前先确认当前生效的是哪个 source。

## 三、Windows MSVC 原生编译（ring 等）

现象：`cargo test -p flowy-app` 在 `ring 0.17.14` 的 build script 失败：

```
cl: 命令行 error D8050 :无法执行“...\c1.dll”: 未能将命令行放入调试记录中
error occurred in cc-rs: command did not execute successfully ... "cl.exe" ... -Z7 ...
```

已排除与已验证：

- 用 cc-rs 打印的完全相同参数手工调用 `cl.exe` **成功**；VS 安装与 `c1.dll` 本身没问题。
- 单独导入 vcvars64 环境、`CC=cl.exe`、改 `TEMP/TMP`、换 registry source、直接调用 rustup cargo，**都不能修好**。
- 生效的 workaround：**关闭原生调试信息**（仅本地验证用，不写入仓库配置），
  且必须在**带 VS 开发环境的终端**里执行。两种等价做法（方式 B 为本轮实测，
  方式 A 为分支既有验证记录）：

  **方式 B（本轮实测，Git Bash）**：先导入 vcvars64 环境，再关闭依赖调试信息：

  ```bash
  # 当前 shell 已带 vcvars64 的 INCLUDE/LIB/LIBPATH，且 VC bin 在 PATH 里
  cargo test --offline --ignore-rust-version \
    --config 'profile.dev.package."*".debug=false' \
    -p flowy-app --no-run
  ```

  **方式 A（分支既有记录，cmd / 开发者终端）**：

  ```bat
  cd /d <repo>\flowy18_wip
  set CC=cl.exe
  set CARGO_PROFILE_DEV_DEBUG=0
  cargo test --offline --ignore-rust-version -p flowy-app --no-run
  ```

  方式 A 的记录见 `docs/cloud-model-catalog/VERIFICATION.md`（`CC/CXX=cl.exe`、`CARGO_PROFILE_DEV_DEBUG=0`）。

- **VS 环境怎么进**：开始菜单打开 **x64 Native Tools Command Prompt for VS 2022**
  或 Developer PowerShell for VS 2022（方式 A 直接在里面跑）。Git Bash 用户需要先把
  vcvars64 的 `INCLUDE` / `LIB` / `LIBPATH` 和 VC bin 目录导入当前 shell（方式 B）；
  导入不了就用方式 A，不要在普通终端硬跑，否则会重复 D8050。

- 该配置下构建在 12m34s 内完成（`Finished test profile ...`），ring 不再触发 D8050。
- 根因未定论（指向 MSVC 调试记录生成与进程环境），因此**不要把该配置固化成仓库默认**；
  换机器/换 VS 版本后先重试默认配置，仍失败再用本 workaround。
- 进一步排除（2026-11-17）：用 cc-rs 日志中的**完全相同参数、相同 cwd、相同混合分隔符**
  在 VS 环境里手工重放 `cl.exe`，可以正常编出 `.obj`；失败只发生在 cargo → build script →
  cc-rs 这条调用链下。因此不像是参数/路径问题，更像是 agent 沙箱对 cargo 子进程的限制
  （内存/job 约束一类）。
- **处理决定（2026-11-17）**：本分支不再追根因，留到下一个专门分支处理；
  本分支只用上面的 workaround 完成验证。下次在普通 VS 终端用默认配置复测一次。
- **复测结果（2026-11-18，后续分支）**：在 VS 环境（`vsenv.sh`）、**独立 target 目录**、
  **默认 profile（不关调试信息）** 下执行 `cargo check -p flowy-net`（33.52s，含 `ring`）
  与 `cargo check -p flowy-app --locked`（7m29s，含 `cc`/`ring`/`gpui`），**均未复现 D8050**。
  结论：当前机器 + Rust 1.100 工具链下默认配置可用，workaround 仍保留在本文件备用，
  但不应固化为仓库默认；换机器或 VS 升级后若复现，按上面的方式 B 处理。
  复测命令：`CARGO_TARGET_DIR=<repo>/flowy18_wip/build-test/d8050-target cargo check -p flowy-app --locked`
  （注意：仓库若配置了 `[build] build-dir`，实际产物仍可能落在共享 build 目录，不要据此判断隔离成功）。
- 副作用：第三方依赖无调试信息；切换 profile 会触发一次重编。

## 四、运行测试

1. 直接跑测试会大面积 `PermissionDenied (os error 5)`：默认 `std::env::temp_dir()` 在当前进程下不可写。
   把 `TEMP/TMP` 指到**工作区内**目录再跑：

```bash
mkdir -p build-test/rebase-validation-tmp
export TEMP=<repo>/flowy18_wip/build-test/rebase-validation-tmp
export TMP="$TEMP"   # 必须分成两条：同一行 export 里的 $TMP 会取旧值
cargo test -p flowy-app -- --test-threads=1
```

> 2026-11-18 复测补充：`export TEMP=... TMP=$TEMP` 写在**同一条** export 里时，
> `$TEMP` 在赋值前展开，`TMP` 仍是旧的用户 Temp，测试目录会落到 `C:\Users\...\Temp`
> 并被当前沙箱拒绝（表现为 `PermissionDenied (os error 5)`）。务必分两条 export。

2. 2026-11-17 记录：`flowy-app` 共 21 个测试，**21 passed / 0 failed**
   （2026-11-18 收尾分支新增 1 个刷新门禁测试后为 22 passed）；
   其中 `failed_new_task_is_not_hidden_by_navigation` 已按
   [session-index-storage-error-test.md](session-index-storage-error-test.md) 重写为
   「隔离损坏索引 + 从 history 重建 + 导航不清提示」的断言，不再依赖旧磁盘状态。
3. 测试需串行（`--test-threads=1`），避免共享临时目录互相干扰。

## 五、下次照抄的流程

1. 备份并 stash：

   ```bash
   git branch backup/<name>-pre-main-rebase-$(date +%Y%m%d) HEAD
   git stash push -u -m "pre-main-rebase wip"
   ```

2. 拉取并 rebase：

   ```bash
   "/d/Git/cmd/git.exe" -c http.sslBackend=openssl -c http.proxy= -c https.proxy= fetch origin --prune
   git rebase origin/main
   ```

3. 解决冲突（策略见第一节），`git add` 后 `git rebase --continue`。

4. 恢复 wip：`git stash pop`；冲突按需修，确认无误后 `git stash drop`。

5. 验证（进入方式见第三节；方式 B 为本轮实测）：
   - 在已导入 vcvars64 的 shell 里编译测试：

     ```bash
     cargo test --offline --ignore-rust-version \
       --config 'profile.dev.package."*".debug=false' \
       -p flowy-app --no-run
     ```

   - 运行测试二进制（TEMP/TMP 必须指向工作区内可写目录，否则大面积 PermissionDenied）：

     ```bash
     export TEMP=<repo>/flowy18_wip/build-test/rebase-validation-tmp
     export TMP=$TEMP
     ./build.noindex/debug/deps/flowy_app-*.exe --test-threads=1
     ```

6. 格式化只改动的文件（`cargo fmt --all` 在本机会因路径长度失败）：

   ```bash
   rustfmt --edition 2024 crates/flowy-studio/src/{panel,studio}.rs
   ```

## 六、启动 smoke（2026-11-17 已验证）

构建桌面入口并直接运行，窗口正常出现，说明 rebase 后应用可启动：

```bash
cargo build --offline --ignore-rust-version \
  --config 'profile.dev.package."*".debug=false' \
  -p flowy-desktop

./build.noindex/debug/flowy-app.exe
```

实测：窗口标题 `Flowy`；已登录账号下模型 chip 显示云端目录里的模型名
（`Deepseek-v4.1-flash`），说明 `catalog → panel → composer` 的模型名同步链路
在真实账号下工作。注意这只是 2026-11-17 的启动 smoke，不是 UI 回归；当时的
`dx serve` 热补丁尚未打通（**2026-11-26 已修复并实机验证通过，见下文「六（补充）」**）。

## 六（补充）、Windows 热补丁（`dx serve --hot-patch`，2026-11-26 已实测跑通）

其他 Windows 开发者或 Agent 迭代 `flowy18_wip` 界面时，不再需要每次改动都整包重编重拉（~1 分钟+）；可直接通过 `scripts/dev-hotpatch.ps1` 原位热补丁运行中窗口。

### 1. 前置环境（每台 Windows 机器一次性准备）

- **Rust 工具链**：仓库 `rust-toolchain.toml` 锁定的 `1.100`（自带 `lld-link.exe`，位于 `<sysroot>\lib\rustlib\x86_64-pc-windows-msvc\bin\gcc-ld\lld-link.exe`）。
- **`dx 0.7.10`（精确版本，与 `gpui-pre` 的 `subsecond 0.7.10` 匹配）**：

  ```powershell
  cargo install dioxus-cli --version 0.7.10 --locked
  ```

- **Visual Studio Build Tools（MSVC x64）+ Windows 10/11 SDK**（含 `vcruntime.lib`、`ucrt.lib`、`advapi32.lib`；脚本会通过 `vswhere.exe` 自动发现，**无需**提前打开 VS Developer PowerShell）。

### 2. 启动命令与实测表现

在 `flowy18_wip/` 目录（普通 Windows PowerShell 5.1 或 PowerShell 7 均可）执行：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\dev-hotpatch.ps1
```

- **自动覆盖的热补丁 UI crate**：`flowy-desktop`、`flowy-app`、`flowy-studio`、`flowy-ui`、`flowy-i18n`、`flowy-shell`、`flowy-obs`。
- **耗时预期**：
  - 首次冷启动或增删 `flowy-desktop` 依赖时：需要编译 `desktop-dev` 依赖并生成一次约 857 MB 的 fat archive（`target\...\libdeps-*.a`），耗时较长。
  - 会话建立后改代码热补丁：实测修改 `crates/flowy-app/src/app/titlebar.rs`（~18s）与传递 UI crate `crates/flowy-studio/src/home.rs`（~15s）均可原位热补丁生效（日志出现 `GPUI hot reload applied`），**保持同一 PID、窗口几何与云端登录会话不重启**。
- **不适用热补丁的改动**：增删 struct 字段、修改静态初始化器或变更 `Cargo.toml` 依赖时，在 `dx serve` 终端按 `r` 重建或重启会话。

### 3. 已修复的 5 处 `dx 0.7.10` Windows 缺陷（切勿误删相关代码）

1. **响应文件空格路径未加引号（`LNK1104: Files\Microsoft.obj`）**：`scripts/dx-capture-wrapper/src/link_wrapper.rs` 将含空白行重写到临时 UTF-16LE BOM `.rsp`。
2. **`DX_LINK_CUSTOM_LINKER` 绕过 `NoLink` + 捕获环境回灌**：`src/main.rs` 过滤 `RUSTC_WRAPPER` / `RUSTC_WORKSPACE_WRAPPER` / `DX_RUSTC` / `DX_LINK*` / `CARGO_MAKEFLAGS`；`src/link_wrapper.rs` 在 `DX_WRAPPER_NO_LINK=1` + `DX_LINK=1` + `@<DX_LINK_ARGS_FILE>` 时写出 20 字节 COFF 占位桩。
3. **MSVC `link.exe` 隐式 `/INCREMENTAL` `@ILT` 偏移崩溃（`0xc0000409`）与 `LNK4003`**：`dev-hotpatch.ps1` 优先选用 rustup 自带 `lld-link.exe` 作为 `DX_REAL_LINKER`，缺失时才回退 MSVC `link.exe` 并由 `dx-link-wrapper` 追加 `/INCREMENTAL:NO`。
4. **x64 `stub.o` 踩毁 `__chkstk` 的 `RAX` 入参导致栈溢出（`0xc000041d`）**：`dx-link-wrapper` 在 `/DLL` 补丁链接时复制 `stub.o` 并将 `__chkstk` / `_chkstk` / `___chkstk_ms` / `__alloca_probe` 降为 `STATIC`，改由 `msvcrt.lib` 原生 `chkstk.obj` 解析。
5. **1-hop 文件监听漏掉传递 UI crate**：`crates/flowy-desktop/Cargo.toml` 直接声明 `flowy-i18n` / `flowy-studio` / `flowy-ui`（均已是 `flowy-app` 传递依赖，不引入新外部包），使 `dx` 原生监听规范路径并走完整拓扑级联重放。

### 4. Windows 热补丁常见排查速查

| 现象 | 原因与处理 |
| --- | --- |
| 脚本提示 `Flowy is already running` | 关闭已有 Flowy 窗口或停止旧 `dx serve` 会话（`dx` 启动的进程名为 `flowy-app-<hash>.exe`） |
| 报 `need dx 0.7.10` 或选到 `mise\shims\dx` | 安装 `dioxus-cli 0.7.10`；脚本已自动跳过 `mise\shims` 并筛选真实 `dx.exe` |
| 删掉/重编了 `dx-capture-wrapper.exe` 后变慢 | Cargo 把 `RUSTC_WRAPPER` 哈希计入指纹，重编 wrapper 会触发一次 `desktop-dev` 依赖重编，平时勿手动清理 `scripts/dx-capture-wrapper/target/` |
| 直接手敲 `dx serve --hot-patch` 仍报 `LNK1104` | 必须通过 `.\scripts\dev-hotpatch.ps1` 启动，不要绕过脚本手敲 `dx serve`（脚本负责清 `sccache`、挂双 wrapper、配 `LIB` 与 `lld-link.exe`） |
| PS 5.1 下编 wrapper 报 `--config` TOML 错误 | 脚本已改用临时 `CARGO_BUILD_BUILD_DIR` 并在 `finally` 还原，切勿改回裸 `--config 'build.build-dir="..."'` |

## 七、别做的事

- 不要把 `flowy18_wip/.cargo/config.toml`、`profile.dev.package."*".debug=false` 当作长期修复提交进仓库。
- 不要在 `workspace-write` 沙箱下直接 rebase/fetch（写不了 `.git`）。
- 不要并行跑 cargo 构建/测试（工作区大、`build.noindex` 会互相抢锁）。
- 不要为了通过测试而保留损坏的 `sessions/index.json`（产品行为以隔离+重建为准）。
- macOS/Linux 的启动脚本、target 条件依赖、共享构建配置本轮未改；后续要动它们前先确认三端差异。
