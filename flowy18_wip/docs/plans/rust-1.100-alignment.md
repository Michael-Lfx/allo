# Rust 1.100 工具链对齐计划

状态：**声明已对齐（2026-11-18）**；剩余为验证与可选项。
起点：本机工具链从 1.97.1 升到 1.100.0（`mise use -g rust@1.100.0`），
`rustc 1.100.0` / `cargo 1.100.0` / `clippy 0.1.100` / `rustfmt 1.11.0`。
1.100.0 是当前最新稳定版（2026-11-12 发布）。

## 已执行

1. `flowy18_wip/Cargo.toml`：`rust-version` `1.99` → `1.100`（MSRV 与工具链一致）。
2. `.github/workflows/flowy18-ui.yml`：`dtolnay/rust-toolchain@master` 的
   `toolchain: "1.99"` → `"1.100"`，组件仍是 rustfmt + clippy。
3. 验证：`cargo metadata` 报 `rust_version = 1.100`；`cargo test -p flowy-i18n --locked`
   退出 0；`cargo fmt --all -- --check` 退出 0（rustfmt 1.11 无格式漂移）。
4. 清理 Cargo 1.100 的 `unused_dependencies`（20 条 → 0，2026-11-18）：
   - workspace 段删除 `anyhow`、`sha2`、`rfd`、`flowy-chat`（后三者只在 workspace 声明，无成员继承）；
   - `crates/flowy-app/Cargo.toml` 删除 `flowy-chat`、`gpui_platform`（该 crate 未使用）。
   - 删 `flowy-app` 的 `flowy-chat` 后，workspace 那条会变成新的未使用项，故一并删除。
   - `Cargo.lock` 仅少两条依赖边（flowy-chat、gpui_ce_platform 从 flowy-app 节点移除）。
   - 复跑 CI 同口径：flowy-ui 18 / flowy-i18n 4 / flowy-studio 11 / flowy-app 23 全绿，
     flowy-desktop check、fmt、clippy `-D warnings` 全绿，`unused dependency` 告警 0。

## 现在的版本口径

- 本机：`rustc/cargo 1.100.0`（mise 全局 `rust = "1.100.0"`）。
- `flowy18_wip`：`rust-toolchain.toml` pin `1.100` + MSRV `1.100`。
- wip 的 CI job：`1.100`。
- 根仓库（Tauri 主工程）：`Cargo.toml` 无 `rust-version`，CI 用 `dtolnay/rust-toolchain@stable`（当前等同 1.100）。
- Docker：`rust:1-slim-trixie`（跟随 1.x，现为 1.100）。

## 待办（按需取舍）

1. `flowy-chat` crate 现在没有任何成员依赖（只剩 `members`/`default-members` 里的登记）。
   是否退役（移出成员列表或删目录）需要单独决定，本轮未动。
2. 与本次无关的旧告警：`[profile.release.package.gpui-pre-macros]`（`Cargo.toml:159`）
   的包不在依赖图里（`Cargo.lock` 无此项），每次构建打印 6 条 warning；要清就删这段。
3. 根仓库要不要显式锁版本：把各 workflow 的 `@stable` 换成 `@master` + `toolchain: 1.100`
   （锁死、不再跟随 stable），或在根加 `rust-toolchain.toml`（注意会与 `@stable` 步骤重复安装）。
4. 借 1.100 复测 Windows MSVC D8050 现象（现有 workaround 保留，不固化）。

## 回退

- 工具链：`mise use -g rust@1.97.1`（mise 的 rust 安装目录是指向 `~/.cargo/bin` 的软链，秒切）。
- 仓库：本轮只改两处声明行，单独 revert 即可，不涉及运行时代码。

## 未验证

- 根仓库（`allo` 主工程）未在 1.100 下跑校验。
- wip 的 clippy `-D warnings` 与 `flowy-ui/story/app` 测试未在 1.100 下本地重跑（见待办 1）。
