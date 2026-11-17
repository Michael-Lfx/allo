# 最小内核迁移审查与验证

## 结论 (Verdict)
needs-attention — 单pi loop方案可行；当前只具备离线实验基础，生产切换须先完成流协议、工具权限和历史三条边界验证，不直接删除dsh-rs。

## 事实清单 (Irreducible Facts)
- 模型工具调用必须与结果按call id匹配，历史必须保留结构化内容 — physics（协议约束）。
- 流式网络和工具任务必须响应取消，否则停UI不等于停执行 — physics。
- UI/账号目录无需感知具体loop，保持现有外部接口可降低改动 — economics。
- Cordis/AgentRegistry、旧队列和插件hooks是当前实现选择，不是完成LLM→工具→回答的必需结构 — convention。
- 上游rpi-agent低层调用已实测文本/工具续跑/取消/下一run模型切换，未用harness/durable/CLI — evidence。

## 攻击点 (Attack Points, by severity)
1. P1 evidence：仅文本桥接会丢reasoning、usage、工具参数增量及错误语义。当前src/lib.rs明确拒绝这些事件，生产不能直接用它。证伪实验：对截断、乱序、错误终态、未知事件进行负向检查；本轮contract_attacks两项测试通过，但完整转换仍未实现。
2. P1 evidence：旧工具不是七个裸函数。builtin.rs默认注册bash/read_file/write_file/edit_file/glob/grep/todo_write；registry.rs:152-188执行policy waterfall与guards，todo_write依赖SessionService。直接移除注册层会丢权限或任务状态。证伪实验：调用前deny、非法参数、取消时不得进入工具主体；下一阶段必须在宿主工具adapter上验证，不能只测echo。
3. P1 evidence：现有持久化保存投影lines并flush结构化session，不能仅从UI文本重建上下文。证伪实验：保存/恢复带多个tool call、错误result、reasoning的历史，检查消息序列/id/参数一致、无重复执行；当前未验。

## 幸存结论 (What Survives)
- 保留AgentHandle对外接口，生产只运行一个loop。
- 账号模型、凭证、endpoint信任规则归宿主；每run快照，运行中不切模型。
- pi只使用rpi-agent/rpi-ai和必要传递依赖，首期不fork。
- 工具实现可复用，权限、参数校验和取消不可删除；不引入rpi-tools整套工具。
- 存储与UI继续由客户端维护，不引入第二套durable/harness。

## 修改建议 (Changes)
### 分阶段执行范围
A. 当前验证（已执行）：独立分支experiment/pi-loop-contracts；stage1-checkpoint.zip及SHA256保存7个原实验文件；新增contract_attacks测试，不改生产。
B. 协议adapter：只在实验中扩展文本/reasoning/usage/tool增量及finish映射；保留raw arguments字符串直到调用结束，合法JSON才执行；多call按id关联；非法/未知显式失败。不得丢失usage或把error变stop。
C. 宿主工具adapter：一个AgentTool包装现有受控execute入口，先复用policy/guards而非绕开；todo_write显式接宿主会话状态。首版串行，不迁移插件系统；做deny/错误/取消测试。
D. 历史验证：从结构化会话事件转换，不从ChatLine显示文本猜测；验证多工具/恢复/取消后续聊。失败则不切生产。
E. 集成：仅flowy-agent内部接pi run；StreamFn复用现有传输能力，不能启动旧loop嵌套新loop。生产请求只能一个loop驱动。按现有功能门槛运行验证后再删旧loop引用。
F. 清理：按cargo tree与实际引用删除不再依赖的旧调度模块；不按名称批量删dsh-rs。保留工具实现/存储能力，迁移完成前不删旧历史文件。

### 参数保留表
必须：model/endpoint/凭证、system/history/input、tool schema/call id/result、cancel、timeout、max steps、错误终态。
条件透传：max output tokens、reasoning、temperature、usage（有值不可静默丢）。
首期不接：steering/follow-up队列、并行工具、自动压缩、长期记忆、provider自定义、多内核选择菜单、动态插件加载。

### 通过门槛
协议负向测试+旧工具权限测试+结构化历史往返全绿，且单次run有上限/唯一终态/无晚到写入；生产依赖不出现rpi高层应用crate。Windows本机通过只能报告Windows，其他平台另列。

## 未解疑问 (Open Questions)
- 旧七工具有哪些当前产品实际使用频率？缺少使用证据，迁移时先保持行为不删能力；不能按无调用统计臆断无人使用。
- 真实云端SSE是否存在未覆盖事件？需脱敏录制或本地HTTP脚本服务器验证；fixture不能代替真实网关验收。

## 本轮实际证据
cargo test --manifest-path experiments/pi-loop/Cargo.toml --locked --offline：contract_attacks 2 passed/0 failed；loop_probe 4 passed/0 failed；0ignored，退出0，2.66s构建。见contract-test-output.txt。
工具源码只读检查：flowy18_wip/crates/dsh-rs/src/tools/builtin.rs:19、:64及registry.rs:152。生产文件未修改，不提交推送。
