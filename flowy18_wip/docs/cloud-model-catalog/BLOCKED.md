# 验证限制及历史阻塞

## 2026-11-17 更新
用户授权全部必要修复并提交。会话路由阻塞已修复：AgentOptions由Mutex保护、公开set_options、下一请求读取最新路由；apply_config更新全部既有agent且不替换session。回归updated_route_preserves_session_and_history通过，发送已恢复。以下第4—6轮描述为历史记录，不代表当前发送状态。
当前限制：真实云端推理、其他平台未实测；dsh-rs不带unstable feature的既有integration tests报E0603私有模块错误，库编译及本次--lib路由测试通过；原有auth/agent临时目录测试失败仍单独记录。

## 首要阻塞：会话路由不随 apply_config 更新

第4、5、6轮核查同一阻塞持续存在：
- flowy-agent/src/handle.rs:497 在 spawn_session 时将 provider/model 固化到 AgentOptions；send:273复用session.agent。
- apply_live_config:788只更新适配器与booted.config，不更新现存会话的AgentOptions。
- dsh-rs/src/llm/adapters/openai.rs:81直接把options.model写入请求；配置中的default_model不覆盖它。
- 当前公开接口未发现可保持历史并重配已有会话的方法。根治需要扩大修改边界至handle.rs，必要时涉及dsh公开重配API；两者均不在授权白名单内，因此未改。
- 当前选择保存后cloud_ready=false，禁止发送，展示已有notice.model_later，避免错误模型/mock假成功。此限制意味着本次目标未完成，不能发布或合并。
- 不采用新建空会话丢失上下文、跨模块绕私有状态等方式规避边界。

## 其他限制

- 原始源码基线在修改前未成功跑到测试；后续修正MSVC包装后，既有失败与用户描述一致，但不是修改前独立基线证据。
- 最后auth全量：10 passed/1 failed，session::tests::save_load_clear_roundtrip报Store。
- 最后agent全量：10 passed/6 failed/1 ignored，六个原存储测试均PermissionDenied/os error5。
- 上层含tests检查已通过，最近退出0、3.29s；依赖镜像故障已通过官方源补锁定缓存解决，未改配置/依赖/锁文件，下载逐个校验SHA256。
- fixture为固定JSON结构样本：首条来自用户已知样本，其余11条合成；不是完整云端真实目录。指定id变异红→绿已验证。
- 仓库既有云端推理实现使用session access token作为Bearer（transport.rs:195/246），但本功能未进行真实请求验证。
- 未改locale；刷新入口为↻，可访问性/空态文案完整体验未验收。
- 未做其他平台构建或UI真实登录验证。

## 交付状态

独立分支feat/account-model-catalog。无提交、无推送。保留局部实现供审查，不声明完成。若继续实现，需要明确允许修复会话路由所属文件，并增加同一会话切换模型且历史不丢失的真实请求/适配器集成回归。
