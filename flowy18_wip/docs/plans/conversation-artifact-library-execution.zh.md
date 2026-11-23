# 首页对话文件交付与资料库：实施与验收记录

实施依据：[完整计划](conversation-artifact-library.zh.md)。2026-11-21 启动，2026-11-22 持续验证。

## 交付边界

本轮实施 M0–M5 的首版链路与 M6 自动化验证，M7 不在本轮范围。真实模型请求、原生桌面人工交互、三端安装包资源核验必须各自有证据，不能用脚本模型或 renderer fixture 替代。未提交 Git、未推送、未签名或发布版本。

**本轮停止点**：首版链路、225 项自动化测试、workspace check、桌面构建、UI contracts、格式检查与 9 张原生布局截图回读已完成。M6 尚未关闭：许可/打包资源补丁等待已发出的独立授权确认；真实模型首页请求、生产系统保存/外部打开交互、Windows 链接特权与 Unix 分支、性能基线、完整强杀恢复和安装包解包矩阵仍保留未验收状态。

## M0 基线与保留

- 原 HEAD：`c7f4a60200ad4fe6a53c136bbd981c5bf6f7fe5f`；原分支 `fix/flowy18-project-modal-blank-task`。
- 备份分支：`backup/conversation-artifact-library-m0-20261121`。
- 原始 44 项 WIP 完整存入 `conversation-artifact-library-m0-20261121` stash；apply 后仍保留该 stash。原有预览、字体声明、fixtures 和 plan 未被丢弃。
- 当前功能分支：`codex/conversation-artifact-library`，从 `origin/main` 的 `2e5ac1d6dcadfb4b9a6b3f095d0808a7d64f8645` 建立。HTTPS fetch 连接重置；随后 `git fetch github-ssh --prune` 成功，并核实两个 remote 的 main 同指此提交。
- 环境：Windows x64、Rust 1.100、VS 2022 MSVC 14.44.35207；采用现有 `link.exe`，按本机 SDK 配置 LIB；TEMP/TMP 指向隔离工作区目录。Cargo 构建与测试串行。
- 仓库内已有 `library.json` 只有隔离 example fixture；生产不再读写旧 `library.json`，不扫描用户目录查找或迁移文件。
- 原生首帧和 0/100/1,000 条资料的窗口性能基线尚未采集。后台恢复与文件操作已接入；不能据此宣称实测性能门槛通过。

## 已实施的代码链路

1. `flowy-agent-core` 保存 Home/Project/Unknown 来源、独立 workspace、稳定 submission、可信 ArtifactReceipt 与完整 DeliveryCommit。旧历史的可选字段有 serde 默认值。
2. 专用 `publish_file` 走现有 ToolRegistry/ToolGuard，只在明确 Home 会话、本轮 output root 和已持久 submission 下注册；不解析 Markdown、不自动收录普通 write_file、shell 或上传文件。
3. `ArtifactStore` 持有 staging、不可变 payload、独立 manifest/整轮 marker、收藏/回收站偏好、DeletePending/Deleted 墓碑、来源删除证明和查看副本。UI DTO 不含磁盘路径。
4. 发布从固定源句柄分块核验和复制，拒绝越界、链接/重解析点、目录、空文件和已知内容头不匹配。Windows 固定父目录句柄并拒绝源的共享写/删除；Unix 使用 rooted openat/NOFOLLOW，当前机器未运行 Unix 分支。
5. 一轮成功且 history 与完整 DeliveryCommit 落盘后，才提交独立 marker；marker 完成后再更新聊天和资料库。失败、取消、历史写入失败的 prepared 没有成功卡片资格。线性化点之后的取消不回滚已持久交付，收尾显示 SavingFiles。
6. 已提交、暂存、查看副本和元数据有有界预算；永久删除按实际删除结果释放额度。无成功凭证的 prepared 保留 7 天后，仅在历史可读、启动无运行轮次且完整预检通过时清理；损坏/未知版本保留并报告。
7. 对话卡片与资料库使用相同 artifact_id；支持来源标题搜索、真实时间排序、收藏筛选、回收站、撤销/恢复、确认永久删除和实际用量。来源已删除与暂不可用分别展示。
8. 图片/PDF/PPTX 共用宿主预览弹层和 bounded renderer；其余格式是元数据详情与文件操作，不声明已经有专业生成器。关闭弹层恢复触发焦点。
9. 另存通过系统保存对话框，取消不创建文件，后台复制同一快照；默认打开仅对安全格式白名单创建保留扩展名的查看副本。副本计额；用户明确确认关闭外部应用后可清理，Windows 占用失败保留额度与重试提示。
10. 来源跳转后台选择会话，再按 artifact_id 解析虚拟列表行与焦点；Back 复用原 LibraryPanel 的搜索、筛选、视图和滚动状态。会话删除先物化完整交付组；历史/索引失败回滚，独立文件保留。
11. 生产启动的 history/artifact 恢复在后台进行，加载完成前不显示空库。发送落盘和 Stop 也移到后台，使用活动会话、上下文代际、提交 ID 与 UI 请求代际防止迟到结果修改另一会话。

## 生成能力与验证材料

| 能力 | 本轮证据 | 不代表的能力 |
| --- | --- | --- |
| CSV | 隔离本地脚本 SSE 驱动真实 AgentHandle → Engine → 实际 write_file → publish_file，核对字节与同 ID 卡片 | 真实云模型或人工首页请求尚未验收 |
| PDF/PPTX | 自有多页 fixture 通过同一发布、快照与恢复路径；既有 renderer 测试核对页数与非空像素 | 没有接入 Office 专业生成器 |
| 图片 | 既有自有图片 fixture 的尺寸、像素与有界解码测试；同一共享预览入口 | 没有接入新的图片生成 API |

脚本 SSE 测试最初存在 Windows accepted socket 继承 nonblocking 的竞态；修复为 accepted socket blocking + 有界超时后，连续 5 次 focused 测试通过。此修复仅在测试服务，不是生产网络回退。

## 验证记录

2026-11-22 冻结矩阵：`flowy-agent` 69、`flowy-agent-core` 50、`flowy-app` 41、`flowy-i18n` 6、`flowy-studio` 59，共 **225 passed / 0 failed / 3 ignored**。3 个显式 ignored 是原有手动性能基线与两个平台 shell 测试；另有两项 Windows 链接夹具在测试体中因系统错误 1314 明确记录环境跳过，不能计作已执行链接拒绝验收。完整日志：`.scratch/artifact-tests-final.log`。

`cargo check --workspace --offline --locked`、`cargo build --offline --locked -p flowy-desktop`、隔离 `library` example 构建、`bun scripts/check-ui-contracts.mjs`、改动文件 rustfmt 和 `git diff --check` 均已通过；末次小幅正确性/格式调整后再次复核关键检查。`impeccable detect` 返回空结果，只作为机械检查，不替代原生截图。

Core、Agent、i18n 的 focused Clippy 已通过（`.scratch/artifact-clippy-core-agent.log`）。完整相关 crate Clippy 仍被既有未改代码阻挡：Studio `projects.rs:119` 的 assigning_clones；App 的 dock 辅助函数 too_many_arguments/trivially_copy_pass_by_ref，projects 的 map_identity/ignored_unit_patterns/semicolon/auto_deref，skills 的 semicolon，共 15 项错误。最后日志分别是 `.scratch/artifact-clippy-final.log` 与 `.scratch/artifact-clippy-app-final.log`，已没有本轮新增文件的错误；没有为了通过 lint 修改无关功能或加入 allow。

原生渲染 9/9 张已生成并逐图回读：中文浅色资料列表、英文深色 800×600、中文真空库、英文筛选空态、中文深色回收站、英文资料异常、中文加载态、英文三页 PDF、中文深色小窗三页 PPTX。正式大窗口 PNG 为 2376×1473（1584×982 @150%），小窗口为 1200×900（800×600 @150%）。未发现按钮越界、正文截断或模态圆角漏裁切。报告位于 `.scratch/conversation-artifacts-native-final/visual-readback.md`，含截图尺寸与批次进程退出码观测边界；这只验证隔离原生 renderer/layout，不是生产真实模型、系统保存/默认打开或鼠标操作的验收。

本节表格汇总冻结后的结果和证据边界：

| 检查 | 当前结论 |
| --- | --- |
| core/agent focused 单测与服务集成 | Core 50、Agent 69 通过；脚本 provider 链路不替代真实模型 |
| App/Studio/i18n | App 41、Studio 59、i18n 6 通过 |
| UI contracts | 多轮通过；最终随冻结矩阵复核 |
| 格式/空白 | 只检查本次 Rust 文件，已多轮通过 |
| Clippy | Core/Agent/i18n 通过；相关全链有 15 项既有未改代码错误，已记录 |
| cargo check --workspace / desktop build | 均通过，末次调整后再次复核 |
| 原生窗口交互 | CUA 两次返回 GetCursorPos 拒绝访问 0x80070005，未完成 |
| Windows 符号链接夹具 | 两项因错误 1314 无链接特权而明确环境跳过；普通文件、共享写/重命名锁测试独立通过 |
| 真实模型 | 进程未配置 FLOWY_LLM_API_KEY/MODEL/BASE_URL；尚无隔离真实模型验收证据 |

## TC-01–35 证据索引

“服务/自动化”不等于全部原生人工验收；未达到完整输入条件的用例保留待验收。

| 用例 | 已有证据与剩余范围 |
| --- | --- |
| TC-01 | 脚本 provider 驱动真实 CSV 工具链；真实首页模型请求待验收 |
| TC-02 | 真实 PDF/PPTX fixture 发布与 renderer 页数/像素；两入口原生回读待验收 |
| TC-03/04 | 可信工具能力检查、伪造回执负向、缺失/空/目录/内容头负向测试 |
| TC-05/06/07/08 | 同槽位幂等/冲突、跨轮跨会话同名独立 ID、源修改不改快照 |
| TC-09 | 资料、卡片、来源和收藏/回收站重启服务测试 |
| TC-10 | 历史失败回滚、失败模型无 commit；逐个故障窗口的进程强杀仍需扩展验收 |
| TC-11/30 | 模拟部分组移动后恢复、marker 失败；完整进程强杀矩阵待验收 |
| TC-12 | RunFence 取消前/后提交、迟到事件、UI draft/selection 代际测试 |
| TC-13 | core 并发会话测试及跨 session 同名 Store 测试；原生并发两个窗口待验收 |
| TC-14 | rooted 打开、路径/ADS/DOS 名称、共享写/删除锁；链接特权与 Unix 待环境验收 |
| TC-15 | 单文件/轮次/总量与原始暂存计额、元数据/未知版本有界处理；完整上限压力矩阵待验收 |
| TC-16 | 未支持格式不请求 renderer，元数据与安全打开白名单测试 |
| TC-17 | 4 项真实宿主 picker 接收器自动化通过：取消、同字节保存、已确认目标覆盖、目录/占用失败；真实 OS 覆盖提示待人工验收 |
| TC-18 | host 删除来源后文件/偏好仍在、确认删除证明与失败回滚；菜单已接线 |
| TC-19 | 收藏/回收站持久化、恢复与 Undo 请求测试 |
| TC-20/21 | 千行虚拟历史稳定 artifact 锚点/UIA 聚焦、成功工具折叠后卡片可见、Library 滚动保留 |
| TC-22 | 真空库/过滤空态/loading/error 区分，真实新首页流程与失败状态测试 |
| TC-23/24 | 中英文 key 与 UIA、窄内容工具条、Enter/Tab/焦点测试，9 个原生布局截图回读通过；生产窗口真实交互仍待验收 |
| TC-25 | future/corrupt manifest/marker 保留原件、健康组可用与 strict cleanup/write 测试 |
| TC-26 | AgentUpdate::Artifacts 独立于页面，后台更新宿主查询与投影 |
| TC-27 | 生产源码不存在 FLOWY_LIBRARY_DATA/AddFilesRequested/local_path/Catalog 导入路径；example 明确隔离 |
| TC-28 | 当前来源标题优先、删除后 snapshot 回退、来源不可用独立映射测试 |
| TC-29 | 模型失败不形成 DeliveryCommit、旧 prepared 不从后续轮成功复活的提交屏障 |
| TC-31 | 查看副本与主快照字节隔离、计额及确认清理/Windows 占用重试；外部程序真实保存待人工验收 |
| TC-32 | 新 Home origin/workspace 从 core 重建，不以损坏 UI index 为来源权限 |
| TC-33/34 | DeletePending→Deleted、重放防复活、实际删除计额及 Windows 占用测试 |
| TC-35 | source_available/history_unavailable 与持久 source_deleted 分开；损坏历史不显示成确认删除 |

## 许可与发行门禁

已准备可审查的许可原文补齐与打包资源提案，但未获本轮独立授权前不修改 Dioxus.toml、scripts/package-macos.sh 或新增发行声明。当前提案为 20 份原文，共 64,866 bytes，生产缺少 14 份；逐字 SHA-256 已核对，包含精确上游 commit 的 OXML/RPPTX 根 MIT 原文。

提案、可检查补丁与来源 inventory 目前在 .scratch/artifact-license-proposal.md、artifact-license-packaging.patch、artifact-license-inventory.json。实际 MSI/macOS 安装包解包核对没有完成。此门禁未关闭，不能宣称已经具备发行资格。

## 历史范围

本记录补充原计划，不改写或删除计划中的原始用户需求、M7 后续范围、旧预览实现证据与历史附录。既有本地导入/生产注入路线已被专用交付入口取代；原 library.json 和用户源文件原样保留。
