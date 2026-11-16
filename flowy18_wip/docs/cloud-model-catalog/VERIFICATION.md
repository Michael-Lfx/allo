# 验证证据

## 2026-11-17 原子替换最终验证
- 独立第二轮指出先卸旧adapter再注册的失败窗口；改为LlmRuntimeApi::replace_adapter在同一map锁内remove+insert，无注册冲突失败路径；构造replacement失败发生在任何状态变更前。
- cargo test -p dsh-rs --lib --locked --offline --ignore-rust-version：6 passed/0 failed，包含同会话历史保留、旧路由移除/目标覆盖测试，退出0。
- cargo check -p flowy-agent -p flowy-app -p flowy-studio --tests --locked --offline --ignore-rust-version：退出0，5.42s。

## 2026-11-17 路由修复后的验证
- cargo test -p dsh-rs --lib routing_tests --locked --offline --ignore-rust-version：updated_route_preserves_session_and_history ... ok；1 passed/0 failed，退出0。
- cargo test -p flowy-auth model_catalog --locked --offline --ignore-rust-version：5 passed/0 failed，退出0。
- cargo test -p flowy-agent catalog --locked --offline --ignore-rust-version：3 passed/0 failed，退出0。
- cargo check -p dsh-rs -p flowy-agent -p flowy-app -p flowy-studio --locked --offline --ignore-rust-version：退出0，36.23s。
- 最后 cargo check -p flowy-agent -p flowy-app -p flowy-studio --tests --locked --offline --ignore-rust-version：退出0，5.43s。
- 同时把dsh-rs加入--tests的检查失败：既有integration tests引用私有core/llm/session模块导致E0603；未修改旧测试，不声明全套integration通过。
- 原有测试函数块对比：21 unchanged，退出0。人类Git身份hoye，仓库归属检查钩子已启用。
- 同会话测试验证请求配置解析的新model/provider以及session对象、id、历史保持，不是实际云端推理证据。


## 编译环境
Windows MSVC 1.97.1，通过临时cmd调用vcvars64，CC/CXX=cl.exe、CARGO_PROFILE_DEV_DEBUG=0。所有Cargo命令附--ignore-rust-version。未验证声明的Rust最低版本与其他平台。

## 第2轮上层检查
命令：cargo check -p flowy-app -p flowy-studio --tests --locked --offline --ignore-rust-version
输出：Finished `dev` profile [optimized] target(s) in 2m 09s
退出码：0。之后新增边界测试和少量门禁修改，最终还需复跑。

## 第3轮指定fixture变异
命令：cargo test -p flowy-auth model_catalog --ignore-rust-version
仅修改新fixture首条id末字符h→g，断言未变。
输出：
```
parses_cloud_models_and_extra_fields ... FAILED
assertion `left == right` failed
 left: "AIPC-deepseek-v4-flasg"
right: "AIPC-deepseek-v4-flash"
test result: FAILED. 3 passed; 1 failed; 0 ignored; 6 filtered out
```
退出101。恢复fixture后同命令：
```
test result: ok. 4 passed; 0 failed; 0 ignored; 6 filtered out
```
退出0。

## 第3轮目录边界
命令：cargo test -p flowy-agent catalog --ignore-rust-version
```
failed_refresh_keeps_previous_snapshot ... ok
invalid_selection_falls_back_to_first ... ok
refresh_preserves_valid_selection_and_empty_success_clears_it ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 14 filtered out
```
退出0。

## 第3轮app定向测试
cargo test -p flowy-app cloud_config_tests --locked --offline --ignore-rust-version
结果：selected_cloud_model_sets_fixed_provider_and_account_credential ... ok；1 passed/0 failed；退出0。
cargo test -p flowy-app legacy_tiers_do_not_become_cloud_ids --locked --offline --ignore-rust-version
结果：legacy_tiers_do_not_become_cloud_ids_and_new_selection_roundtrips ... ok；1 passed/0 failed；退出0。

## 第4/5轮回归与边界核验
- cargo test -p flowy-auth --ignore-rust-version：10 passed/1 failed/0 ignored；退出101；失败session::tests::save_load_clear_roundtrip，Err(Store)。
- cargo test -p flowy-agent --ignore-rust-version：10 passed/6 failed/1 ignored；退出101；失败均os error5：index::tests::concurrent_saves_do_not_share_temporary_files、tests::corrupt_index_is_preserved_and_cannot_be_overwritten、tests::failed_index_save_emits_a_storage_error、tests::interrupted_session_can_be_used_after_restart、tests::restoring_history_does_not_project_replayed_events_twice、tests::restoring_rich_history_preserves_rows_without_replaying_them_twice。
- cargo check -p flowy-app -p flowy-studio --tests --locked --offline --ignore-rust-version：退出0，最后3.29s。
- git diff --check：无输出，退出0。
- 原有测试函数块与HEAD逐字比较：Original test functions unchanged: 21，退出0。
- git diff --numstat -- '**/Cargo.toml' '**/Cargo.lock' '**/locales/*'：无输出，退出0。
- git diff --name-only：8个tracked文件均在源文件白名单内；新源文件只有auth/model_catalog.rs及agent/catalog.rs。新任务记录不包含在源白名单计数中，单独列为交付记录。

以上编译/单测不能证明运行时会话切换正确；该项已证实受白名单阻塞，发送保持关闭，不得判为整体完成。

## 早期全套测试（非最终冻结结果）
Auth：8 passed / 1 failed，session::tests::save_load_clear_roundtrip返回Store。
Agent：9 passed / 6 failed / 1 ignored，六个原有存储测试PermissionDenied/os error5。
以上在新增后续测试前取得，最终需要重新记录。
