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
- 副作用：第三方依赖无调试信息；切换 profile 会触发一次重编。

## 四、运行测试

1. 直接跑测试会大面积 `PermissionDenied (os error 5)`：默认 `std::env::temp_dir()` 在当前进程下不可写。
   把 `TEMP/TMP` 指到**工作区内**目录再跑：

```bash
mkdir -p build-test/rebase-validation-tmp
TEMP=<repo>/flowy18_wip/build-test/rebase-validation-tmp \
TMP=<repo>/flowy18_wip/build-test/rebase-validation-tmp \
  ./build.noindex/debug/deps/flowy_app-*.exe --test-threads=1
```

2. 本次最终结果：`flowy-app` 共 21 个测试，**21 passed / 0 failed**；
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
在真实账号下工作。注意这只是启动 smoke，不是 UI 回归；`dx serve` 热补丁未验证。

## 七、别做的事

- 不要把 `flowy18_wip/.cargo/config.toml`、`profile.dev.package."*".debug=false` 当作长期修复提交进仓库。
- 不要在 `workspace-write` 沙箱下直接 rebase/fetch（写不了 `.git`）。
- 不要并行跑 cargo 构建/测试（工作区大、`build.noindex` 会互相抢锁）。
- 不要为了通过测试而保留损坏的 `sessions/index.json`（产品行为以隔离+重建为准）。
- macOS/Linux 的启动脚本、target 条件依赖、共享构建配置本轮未改；后续要动它们前先确认三端差异。
