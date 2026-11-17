# 删除 dsh-rs、只保留 pi-rust 的计划

状态：待确认。本文只做规划，不删除任何文件。

## 一、结论

`dsh-rs` 已经没有任何依赖方，可以从工作区移除并删除目录；删除不会改变 `flowy-agent` 的行为（它已切到 `flowy-agent-core` + pi-rust）。建议分两步走：先解除工作区引用并验证，再单独提交删除目录，便于回滚。

## 二、现状证据

1. 反向依赖：`cargo tree -i dsh-rs --workspace` 只输出 `dsh-rs` 自身。
2. `cordis-rust` 的唯一依赖方也是 `dsh-rs`；删除后两者一起离开构建图。
3. `flowy18_wip/Cargo.toml` 仍有三处引用：`members` 第 18-20 行、`default-members` 未包含它、`[workspace.dependencies]` 第 64 行，以及第 22-23 行关于 clippy 的注释。
4. `flowy18_wip/AGENTS.md` 第 16 行仍把 `dsh-rs` 列为 Agent harness。
5. `Cargo.lock` 仍保留 `dsh-rs`、`dsh-plugin-contract`、`dsh-plugin-hello` 三个条目。
6. 目录规模：61 个受版本控制文件，约 556 KB；`git status` 对该目录干净，没有未提交修改。
7. 无 CI／脚本／brand 引用：`.github`、`scripts`、`brands` 中未发现 `dsh` 相关配置。
8. 其集成测试此前存在私模块 `E0603` 失败（既有问题）；删除后这些红目标不再进入工作区，但不应把“消除红测试”当作删除理由。
9. 仓库内其他引用仅剩历史文档与 core 里两处 MIT 归属注释（“ported from dsh-rs”），注释应保留。

## 三、范围

1. 删除 `flowy18_wip/crates/dsh-rs` 整个目录，包括 `crates/dsh-plugin-contract` 与 `crates/dsh-plugin-hello`。
2. 从 workspace `members` 移除三项。
3. 从 `[workspace.dependencies]` 移除 `dsh-rs`。
4. 删除第 22-23 行的 dsh clippy 注释。
5. 更新 `flowy18_wip/AGENTS.md` 的 crate 表：移除 dsh-rs 行，补 pi-rust 行；`flowy-agent` 描述从 “UI↔dsh 端口” 改为 “UI↔core/pi 端口”。
6. 保留历史文档（迁移记录、cloud-* 记录）中的 dsh 描述；在迁移文档里补一句“dsh-rs 已删除，见本计划”。
7. 保留 core 中两处 MIT 归属注释。

## 四、前置：可回滚 checkpoint

当前工作区有大量未提交的迁移改动，直接删目录会让 diff 变大且无法二分。删除前需要：

1. 把当前迁移改动提交为 checkpoint（需用户明确同意提交）。
2. 打标签 `archive/dsh-rs-<日期>` 指向删除前提交，或至少在计划文档记录删除前的 commit SHA。
3. 确认该标签/提交不会被意外清理。

若用户不希望提交，则只执行“解除引用”一步，暂不删除目录；目录留着不影响构建。

## 五、执行步骤

P1 解除引用（可独立编译，目录仍在）：

1. 编辑 `flowy18_wip/Cargo.toml`：删除三个 member、workspace 依赖、注释。
2. 编辑 `flowy18_wip/AGENTS.md`：按第三节更新表项。
3. 运行 `cargo metadata --no-deps`，确认 crate 列表不含 dsh。
4. 运行 `cargo check -p flowy-agent -p flowy-agent-core`（串行），确认无引用错误。
5. 运行 `cargo tree -i dsh-rs --workspace`，预期报 “package not found”。

P2 删除目录（单独提交）：

1. `git rm -r flowy18_wip/crates/dsh-rs`。
2. 运行 `cargo metadata` 更新 `Cargo.lock`，确认三个条目消失。
3. 全仓搜索 `dsh[_-]rs|dsh_rs`（排除历史文档与归属注释），预期只剩文档。
4. 运行 `cargo check --workspace`（串行；dsh 移出后范围变小）。
5. 运行 `cargo test -p flowy-agent-core` 与 `cargo test -p flowy-agent`（TEMP 重定向到可写目录）。
6. 运行 `cargo build -p flowy-desktop`，确认可执行文件仍能产出。

P3 收尾：

1. 更新迁移文档与本计划的状态为“已完成”。
2. 可选清理 `build.noindex/debug/incremental/dsh_rs-*` 等旧产物（不属版本控制，空间回收用 `cargo sweep` 或手动删除）。
3. 确认 `Cargo.lock` 无 `dsh`、`cordis-rust` 条目。

## 六、验收标准

1. `cargo metadata --no-deps` 的包列表不含 `dsh-rs`、`dsh-plugin-contract`、`dsh-plugin-hello`。
2. `cargo tree -i dsh-rs --workspace` 返回 “package not found”。
3. 全仓搜索只命中历史文档与 core 归属注释，无代码/脚本引用。
4. `cargo check --workspace` 退出码 0（记录警告数量）。
5. `flowy-agent-core` 与 `flowy-agent` 测试保持全绿。
6. `flowy-desktop` 构建成功，`target/debug/flowy-app.exe` 可启动。
7. `Cargo.lock` 中 `dsh`、`cordis-rust` 条目消失。

## 七、风险与回滚

1. 其他分支仍在使用 `dsh-rs`：删除只影响当前分支；需要时从标签/历史恢复。
2. 未提交改动被混入删除提交：通过 P0 checkpoint 隔离。
3. 历史文档仍在描述 dsh：保留为历史记录，避免改写既有证据。
4. 回滚方式：`git checkout <tag> -- flowy18_wip/crates/dsh-rs` 并恢复 manifest，或 revert 删除提交。

## 八、工作量

1. P1：约 15 分钟 + 一次增量 check。
2. P2：约 10 分钟 + 一次 workspace check、两次测试、一次桌面构建（受缓存影响）。
3. P3：约 10 分钟。

总计半天内可完成，主要时间在编译验证。

## 九、待确认

1. 是否先提交当前迁移改动作为 checkpoint（需要明确授权提交）？
2. 删除前是否打 `archive/dsh-rs-<日期>` 标签？
3. 历史文档是否保持原样（建议保持）？
