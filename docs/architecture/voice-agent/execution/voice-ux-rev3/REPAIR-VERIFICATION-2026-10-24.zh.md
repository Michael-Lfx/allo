# rev3 独立复审修复与验证事实

> **本地提交回执（2026-10-24）**：本轮代码、测试及原文档已提交为 `8f62353ea386d483fa26f911b62000191d9e3d5a`；署名门禁通过，未推送。后续按 [下一步验证修复计划](NEXT-VALIDATION-PLAN-2026-10-24.zh.md) 的 P0–P4 执行，该计划尚未运行。以下“未提交/无提交授权”为代码提交前快照，保留作历史，不代表当前 Git 状态。

> 2026-10-24；分支 `feat/voice-next`；接管基准 `4aca4cd11333a0d4151d03fb8941186c1017462b`。修复与原实施改动均在工作树，未提交、未推送。本文覆盖本目录历史“全部通过”声明；旧报告原文保留作历史参考，不能用于当前签核。

## 1. 本轮结论与边界

已修复独立审查发现的创建失败、生命周期竞态、清理未接线、持久身份冲突、迟到快照复活活动行、状态误报和侧栏交互缺口。进一步补齐首页模型/预设冻结、有效输入后发布自动复用指针，以及晚到回复经过另一条 SQL 投影路径时必须保留会话的遗漏。

本轮复用 Conversation、现有 SQLite extra、启动仲裁、journal 和恢复 tick；没有新增 migration、权限模式、依赖或 agent harness。正式消息仍走共享 Conversation 历史。未使用占位采用逻辑关闭，保留 creation key 作为终态身份，不能通过公开 PATCH 重新打开。

**不能签核“24 项全部完成”**：组件静态渲染与源码检查不是浏览器点击证据，代码编译不是原生透明首帧或物理声学证明。完整 Conversation 回归有三项技能加载测试未结束，另有全仓 TypeScript/dead-css 门禁失败，详见第 4 节。

## 2. 修复映射

| 审查项 | 修复与最小合同 |
| --- | --- |
| F1 项目创建触发公开字段保护 | 公开创建仍拒绝保留字段；内部 `create_inner` 在校验之后注入可信 workspace 与创建身份。显式新建对默认、项目均分配独立 request 身份。 |
| F2 关闭与写入竞争 | SQLite 同一写事务中检查终态、保留生命周期并插入消息；关闭条件在事务中重新检查消息、active turn 与 accepted receipts。补齐 dialogue pair 的独立原生 SQL 写入路径。 |
| F3 清理为不可达接口 | 删除无调用方的 `/session/discard_empty`、handler 与前端 helper。宿主恢复 tick 根据可信 terminal、matching session/request、主窗口页面和任务保护集合执行 ended/cleanup。 |
| F4 读回执保留空占位、改名不保留 | 读取不再改变生命周期；用户编辑在成功写入事务中保留。失败写入不制造 retained。 |
| F5 公开 extra 可覆盖终态 | 生命周期/创建身份为后端保留字段；普通 repo 更新保留不可变身份和 retained 状态；clone 移除实例生命周期身份。 |
| F6 128 项缓存不足以保证幂等 | creation key 按 user + request 持久化；重试比对 canonical workspace、requested mode、model、preset；ended/discarded 请求不能重建。owner 从已有 Conversation 读取。 |
| F7 多订阅迟到 seed 复活活动行 | 共享单调快照消费者维护活动归属与 retired instance；慢订阅也交付共享最新快照。pending request 仍按精确身份结算。 |
| F8 null/断线/终态误报倾听 | null 显示 syncing；离线显示不可用；结束态显示未通话；无声学 readiness 时显示文字继续。 |
| F9 More 不可聚焦、开菜单重挂载 | 保留同一根节点；opacity 控制可见性而非 display:none；focus-within、Enter/Space、ContextMenu/Shift-F10、Escape 恢复焦点；语音行 hovercard 与操作互斥。 |
| F10 活动通话仍可删除 | 行菜单禁用、共享批量删除入口校验；Conversation 服务安装宿主弱引用 guard，保护统一删除路径。挂断与取消任务保持独立。 |
| F11 仍有麦克风/跑马灯/错误 aria | 所有语音类型采用被动 AudioLines；活动连接点与动效持续保留；语音标题 ellipsis，active/idle 中英可访问文案分开；减少动效时静止。 |
| F12 反例仅源码字符串断言 | 正式加入真实 SQLite 生命周期与 recorder 集成回归；前端改为真实组件静态渲染和异步订阅反例。浏览器交互探针已编写，但没有执行成功，不能作为通过证据。 |

## 3. 接线与数据事实

### 3.1 新建、冻结与保留

首页发送 `new_voice_conversation`，冻结 workspace、模型引用、preset 引用和显式模式。preset 继续由既有 Conversation 创建服务权威解析；不接收客户端 snapshot，不静默丢弃失效 preset，不能将非 Nomi 预设切入该语音执行入口。快聊模型选择机制不在此次改变范围内。

`provisional → retained` 的原子保留点是有效消息/接纳写入或成功的用户编辑；不是读取回执，也不等待 LLM 成功。`ignore` 仍不产生正式问答。显式占位尚未保留时不会替换既有自动目标；保留后将默认/项目指针发布至既有 workspace registry。默认槽使用空字符串内部键，合法 canonical 项目绝不会使用该键；项目 30 分钟规则及默认工作区既有复用规则保持不变。

显式目标解析与迟到投影发布指针共用既有 journal 锁，串行保护 registry 的读取、修改和原子文件替换，避免另一工作区的 map 更新被覆盖。不是另建一份复用状态。

### 3.2 结束与离页

现有 1 秒恢复 tick 只在 connected 且身份可核实时处理终态。`ended` 的持久标记与空占位关闭分开：后台任务未结束也可记录通话已结束，但任务、reservation、当前活动 binding、未投影 journal 输入和仍选中的页面阻止清理。

桌面主窗口通过 `report_voice_context(..., view_only)` 上报“正在看哪个页面”，与自动语音目标候选分开；进入设置可记录已离页，但不清空此前选择的工作区。没有可信 view 上报则暂不清理。断线/unknown/崩溃本身不证明结束，不按短超时猜测删除。

### 3.3 UI

首页 icon-only；输入框左上角胶囊承载阶段；侧栏左声波只表示类型/活动归属，右 More 固定槽承载操作。普通胶囊没有空详情，审批、逐项队列取消、unknown 核对和历史重试保留。CSS 节奏动效不是实际音量或全双工承诺。当前生产 AEC 未在本轮接入。

## 4. 本轮验证

原始日志位于忽略目录 `build.noindex/voice-workspace-display-review/`，不是可移植的签核附件。以下正式测试文件随代码保留，可复跑。

| 检查 | 当前证据 |
| --- | --- |
| `cargo test -p nomifun-conversation --test voice_placeholder_lifecycle` | **11 passed**，含 24 次真实 close/append 竞争、改名、公开保留字段、ended 旧请求、共享删除 guard。无模型网络调用。 |
| `cargo test -p nomifun-app --test voice_dialogue_integration --no-default-features` | **30 passed**；真实 service/SQLite/recorder，新增终态/离页/重建 recorder 恢复、迟到 pair、首页模型冻结与 preset 不静默忽略、默认/项目指针发布。 |
| `cargo test -p nomifun-app --lib voice_ --no-default-features` | **90 passed、3 ignored**；忽略项不计通过。 |
| 前端既有定向命令（21 文件，含 `rev3Probes.test.tsx`） | **164 passed、0 failed、1240 assertions**；静态组件渲染、适配器异步事件/请求验证。不是 164 项 DOM 点击。 |
| `cargo check -p Flowy` | 最终 DTO/宿主/桌面桥接增量编译通过，exit 0，2m03s；未启动真实桌面窗口。 |
| `bun run build:ui` | 最终状态/共享快照改动后生产打包通过，exit 0。 |
| TypeScript | 最终直接 `tsc --noEmit` 为 61 条；与修复前诊断集合差分新增 0、移除 0（统一工作目录路径前缀后比较）。不能把全仓 typecheck 记为通过。 |
| i18n/theme/icons/button-layout/motion/css-scope/架构边界 | 本轮定向脚本已通过；i18n 11764 keys。 |
| dead-css | **失败**；报告指向本轮未改动的 codeBlock、Wallpaper、HerdsmanProviderNotice、CloudLogin、技能消息、eval、视频等文件及旧基线条目；没有修改基线掩盖失败。 |
| DB/Conversation lib 回归 | DB **458 passed**。Conversation 完整运行中共 3 项技能加载测试长期不结束，已终止该次测试进程；单独重跑 `send_message_allows_large_explicit_skill_snapshot` 45 秒仍超时并清理 owned child。显式跳过这 3 项后 **644 passed、3 filtered out**；过滤不是通过。尚未证明卡住与本轮无关，不能标成“已知基线通过”。 |
| 浏览器/原生 | 浏览器交互探针已编译，但运行未得到有效结果；工具拒绝本地文件加载后停止，不绕过策略。原生透明/DPI/首帧、真实声音仍未签核。 |

正式测试入口：

- `crates/backend/nomifun-conversation/tests/voice_placeholder_lifecycle.rs`
- `crates/backend/nomifun-app/tests/voice_dialogue_integration.rs`
- `ui/src/renderer/pages/voiceSession/rev3Probes.test.tsx`
- 未执行成功的交互探针：`ui/src/renderer/pages/voiceSession/tests/voiceComposerRev3.browser-probe.tsx`（不被生产导入）。

## 5. 24 项验收的准确解释

| 用例 | 当前可证明范围 / 剩余 |
| --- | --- |
| LC-01/02 | 默认和项目独立创建、相同身份复用、配置改变拒绝有真实服务证据；没有首页两次点击的浏览器录像。 |
| LC-03/04 | 30 项 recorder 集成及 90 项宿主测试覆盖路由冻结/单通话/unknown；本轮未操作真实 KWS。 |
| LC-05 | 冻结目标和现有跨会话隔离测试通过；真实迟到 ASR 设备未测。 |
| LC-06/07 | 终态、离页保护、重建 recorder、晚到消息、改名保留有 SQLite 测试；宿主 tick/主窗口 view 上报接线已编译。 |
| LC-08 | 既有 ignore/FIFO/审批恢复回归保持；无声学硬件结论。 |
| LC-09 | 原子 close/append 竞争和迟到 pair 有实测；不夸大为所有 send/task 组合压力测试。 |
| LC-10/11 | 持久终态/公开保护、旧会话不清理、指针发布有测试；未做整个进程断电恢复演练及所有分页组合穷举。 |
| LC-12 | 显式模型冻结、变化冲突和失效 preset 不丢弃有真实集成测试；有效 preset 完整目录解析沿既有服务，本轮没有另建全套预设 fixture。 |
| UI-01/04/05/07/09 | 组件渲染/异步快照/代码边界证据；不等于所有虚拟列表/入口的实际点击验收。 |
| UI-02/03/08/11 | 布局、焦点与 reduced-motion 代码已修；中英静态渲染通过。浏览器宽度/hover/焦点、明暗视觉与 DPI 尚未签核。 |
| UI-06 | 共享服务删除防线及宿主回归通过；挂断与取消任务独立沿用既有测试。 |
| UI-10 | 审批/队列/unknown/历史重试原有定向组件及适配器回归通过，未取得浏览器交互回执。 |
| UI-12 | 沿用既有桌宠 owner/耳机/临时显示逻辑；原生窗口与桌宠姿态未签核，不重复声明通过。 |

## 6. 接续执行顺序

1. 先阅读本文及当前 diff；不得根据历史 RESULTS 的勾选重新宣布全绿。
2. 保留语音定向测试的 green 状态，补查 3 项技能加载测试卡住的具体调用边界，并区分 HEAD 基线与修复回归。不要为绕过测试修改业务接纳合同。
3. 在允许的交互验证环境执行已有探针：More 的真实 click/keyboard/Escape、菜单互斥、固定槽宽度、中英/明暗/reduced-motion。没有实际回执的项目保持未签核。
4. 原生验证仅对透明首帧、正确 Bolt/owner、保存位置和 DPI 作验收；不以 Web 测试替代。声学能力未变，不宣称接入 AEC。
5. 当前没有提交/推送授权；本轮不修改 Git 历史或分支。

## 附录：历史证据

同目录 `PROGRESS.md`、`RESULTS.zh.md`、`BLOCKED.md` 的旧正文完整保留。其“160 项”“24 项全部完成”“discard_empty 已调用”“全部质量门禁通过”等属于原执行报告，已被本文逐项纠正，不是本轮复审结论。
