# 账号模型目录执行进度

## 后续分支收尾（2026-11-18，PR #458 合并后）

- PR #458 已合并（merge commit `86fcd6c32`）；合并前修复了 Windows job 的格式门禁与 1.100 clippy 门禁（格式 24 文件；clippy 全量清零）。
- 后续分支 `feat/flowy18-wip-followups`：① 历史工具结果容错——`to_agent_message` 跳过缺 `MessageSource::Tool` 的条目，`repair_tool_call_history` 在请求前补齐配对（缺 result 的 call 补 `is_error=true` 占位、无 call 的 result 丢弃、坏 JSON arguments 降级空对象）；② 失败状态在 working indicator 可见（本地化、脱敏、aria_live=Polite，重启首帧也刷新）；③ 刷新门禁抽纯函数并补单测；④ D8050 默认 profile 复测（未复现，workaround 保留备用）；⑤ 文档同步（BLOCKED / 踩坑文档 / plans 索引 / 本文件）。
- 验证（Rust 1.100）：`cargo fmt --check`=0；`cargo clippy -p flowy-ui -p flowy-studio -p flowy-app -p flowy-i18n --all-targets --locked -- -D warnings`=0；测试 flowy-app 22、flowy-studio 7、flowy-agent-core 35（2 ignored）、flowy-agent 24（1 ignored）、flowy-auth 11、flowy-core 33、flowy-net 3、flowy-ota 23、flowy-obs 6 全绿。


## 收尾状态（2026-11-17 晚）

- 分支已 rebase 到最新 `origin/main`（`0cccfec79`），4 个提交线性重放，仅 `app/mod.rs` 的 `sign_out` 冲突（保留 main 的 `pub(super)` + 本分支的目录清理）。
- 代码审查收尾：修复「未入库的 pi.rs 被已暂存代码引用」与「错误提示当模型名展示」两项 Required；补齐空目录提示、`apply_config` 失败路径、StorageError 去重、刷新入口 i18n/禁用态、品牌 chip 语义；清理死代码 `sync_agent_llm`。
- 验证：`flowy-app` 21 passed、`flowy-studio` 5 passed、`flowy-agent-core` 24 passed/1 ignored、`flowy-agent` 34 passed/2 ignored、`flowy-auth` 11 passed；Windows 桌面入口实跑可启动。
- 遗留：真实云端推理、macOS/Linux、Windows MSVC D8050 根因（下个分支）。

## 最新状态（2026-11-17，覆盖下方历史状态）
用户明确授权全部修复并提交。多轮复核撤销未证实的跨账号目录问题；确认并修复已有会话路由不更新。修改范围增补handle.rs及dsh-rs的services/agent/loop_driver。通过Mutex options原地更新，保留session身份及历史；先验证adapter再替换，在线切换拒绝busy，断开取消所有agents。cloud_ready已恢复成功后开放。新增路由回归1/1通过，auth目录5/5、agent目录3/3通过；相关库check通过，最终上层含tests check退出0(5.43s)。旧21个test函数逐字一致。独立复审最终确认两项Required（替换原子性、退出失败路径）均闭环，无新增必修项：运行时新增同锁replace_adapter，断开路径无注册失败分支。最终dsh --lib 6/6绿，上层含tests check退出0(5.42s)。本轮完成已授权提交，不推送；真实云调用和其他平台仍未验证。

1. 目标：账号 cloud 目录驱动选择与固定 provider 配置，保留旧 Model 兼容，不新增依赖。
2. 顺序：基线 → 纯解析/状态 → 生命周期与 UI → 验收。
3. 风险：MSVC 初始化失败；推理凭证契约与完整脱敏 fixture 未确认。
4. 基线分支 feat/flowy18-wip-windows-launch；实现分支 feat/account-model-catalog，无提交。
5. auth/agent 核心初稿已导出；auth 4个测试绿，缺失cloud测试实测红3/1→绿4/0。agent两个新增测试绿。prefs新增model_id/model_account且保留旧字段；panel新增目录/ready/事件和send门禁；studio菜单已初步改目录（旧辅助方法未删）；composer标签改目录。上层尚未编译，app事件未接，当前工作区非交付状态。
6. auth 基线三次尝试尚未进入测试：权限拒绝、cmd 引号错误、未初始化 cl.exe。
7. MSVC 包装修正成功：临时脚本 C:/Users/38788/AppData/Local/Temp/flowy-catalog-test.cmd 调用 vcvars64 后 cargo %* --ignore-rust-version。auth 实际 8 passed/1 failed，agent 9 passed/6 failed/1 ignored；新增3+2测试通过，失败均为原文所列存储/临时目录权限错误。原始源基线未在修改前跑通，不能把当前结果称作未修改基线。下一步补边界测试与上层接入。
8. 现有 cloud-provider-thin-plan.md 为早期范围记录，以本会话已批准计划为准。
9. 原有未跟踪 .cargo/.scratch/build.noindex/docs 内容不修改；不触碰旧测试/locale/依赖表。
10. 上层检查命令已实际运行：cargo check -p flowy-app -p flowy-studio --tests --ignore-rust-version，退出101，下载 atomic/0.5.3 时本机镜像 http://127.0.0.1:18778 返回 Empty reply from server；尚未编译到上层。job bash-701a52cf29464d64 已收集。
11. 第1轮续做：app已新增catalog_task/generation，启动恢复和登录成功拉目录，退出清目录/停止agent/清配置；刷新结果按generation丢弃，选中后固定flowy provider配置JWT+endpoint+id并保存model_account/model_id。移除生产prefs.llm_config及sync_agent_llm调用。auth按账号API同源HTTPS过滤endpoint。composer副标签改Flowy。上层编译仍被atomic下载阻断，尚未验证，严禁以当前状态交付完成。所有已启动后台作业均收集完成。
12. 第2轮：官方static.crates.io可达。下载atomic/bit-set后，临时Python脚本根据Cargo.lock补齐余下16个缓存包并逐个校验锁定SHA256，未改依赖/镜像配置。上层 cargo check -p flowy-app -p flowy-studio --tests --locked --offline --ignore-rust-version 已退出0（2m09s），含既有测试编译通过；job bash-d386980fb9684446已收集。fixture已固定JSON并明确仅首条真实；目录去重保持首条。catalog默认provider和Result刷新已修正；刷新期间cloud_ready=false且catalog_loading阻止重复刷新/切换，失败恢复ready。
13. 仓库既有推理凭证证据：nomifun-cloud llm_chat.rs:137→media.rs:643→transport.rs:246/195，用session access_token写Authorization Bearer，且有legacy token头。当前dsh适配器Bearer路线尚需手工真实验证。
14. 第3轮：指定fixture id单字符变异红3/1→恢复绿4/0已完成；catalog新增有效选择/空目录测试，3/3绿；app cloud_config_tests 1/1绿，2m40s构建完成。prefs新增纯序列化兼容测试，运行中。发送按钮视觉跟随cloud_ready；空token不准选择；不可信endpoint整次报错保留快照。证据见VERIFICATION.md。
15. 第4轮：新增save_checked保留旧save签名，云模型选择保存失败展示存储错误。未知user_id刷新不再清当前内存目录。auth全量10passed/1failed（同session Store），agent10passed/6failed/1ignored（同六个权限错误），含新测试均绿。上层含tests再次check退出0(4.71s)。所有job收集完。
16. 首要新阻塞：handle.rs的spawn_session固化AgentOptions，apply_live_config不更新已存在session；send复用旧agent。因此当前真实切换未闭环，不能交付完成。详见BLOCKED.md。下一轮核实dsh适配器是否有运行时model覆盖或白名单内公共接口安全恢复方案，否则保持发送关闭并报告需要扩大handle.rs边界。未修改handle.rs或dsh。
17. 第5轮确认：openai.rs:81明确body model=options.model，default_model不覆盖；白名单内没有现有公共重配API。选择后cloud_ready保持false并用notice.model_later说明，避免错误mock/旧模型发送。该状态不是功能完成，无法发布。
18. 第5轮检查：上层含tests check退出0(3.29s)；git diff --check退出0；tracked修改8个均白名单；Cargo.toml/Cargo.lock/locales diff为空；脚本比较修改文件中原有21个test函数块与HEAD逐字一致，退出0。所有job收集完成。
19. 第6轮收束：会话路由白名单阻塞在第4/5/6轮持续。未跟踪文件已核验：两个新源模块+本任务记录；cloud-provider-thin-plan.md与.cargo/.scratch/build.noindex是开工前已有内容。BLOCKED/VERIFICATION已整合最终结果。无运行中的后台job。目标按blocked结束，非完成；发送保持关闭，不可发布。
20. 继续工作前：需明确放开handle.rs（必要时dsh公共重配接口）边界，先补同会话切换保持历史的回归再实现。当前只保留目录/偏好/安全门禁的可审查局部实现。所有旧测试/locale/依赖表未改，无提交推送。
