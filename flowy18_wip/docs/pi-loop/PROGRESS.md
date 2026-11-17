# 进度

## 第二阶段审查（最新）
用户要求继续方案规划/审查并开始验证。独立分支experiment/pi-loop-contracts；先归档stage1-checkpoint.zip和checkpoint-sha256.json。只新增contract_attacks.rs，2项负向测试覆盖8种坏流及不支持能力；原4项仍绿，合计6passed/0failed/0ignored，退出0。生产git diff为空。MIGRATION-REVIEW.md给出needs-attention结论及协议/权限/历史→集成→清理计划。未实现完整生产迁移；不提交推送。全部后台job已收集。


## 完成状态
离线最小实验完成：最终4/4绿、0ignored；桥接变异红0/1→恢复全4/4绿。真实loop验证工具续跑、取消及晚到文本、run间模型切换和单文本块旧协议转换。所有job已收集。结论与下一阶段边界见REPORT.md；生产文件git diff为空，未提交推送。完整协议/真实联网/跨平台未验，不扩大结论。以下是执行历史。

- 基线e5524a21b，独立分支experiment/pi-loop-minimal。
- 目标：验证低层loop及薄适配成本；不迁移生产。
- 顺序：锁定依赖→五类离线行为→依赖审计与结论。
- 最大风险：Git依赖网络、上游事件契约转换、取消语义。
- 已创建独立workspace与固定SHA manifest；cargo metadata退出0，锁定177 packages，确认拉取指定SHA。cargo tree -e normal --locked退出0，dependency-tree.txt已保存。metadata.json忽略，生产manifest/锁文件未改。
- 首次cargo test --manifest-path experiments/pi-loop/Cargo.toml --locked退出0，39.30s；integration测试1passed/0failed，验证文本增量/唯一终态/同历史model-a→model-b。日志test-output.txt。下一步工具/取消/事件转换及变异测试。
- 第1轮：增显式async-trait/tokio-util（均已在依赖图）；真实echo工具续跑通过，取消token传播及终态后迟到输出被忽略通过。cargo test --offline最终3passed/0failed/0ignored，cancel-test-output.txt。已读现有dsh StreamChunk定义（types/llm.rs:202），下一步薄转换测试；无需引入完整旧内核依赖，优先按实际序列化协议fixture解码。
- 上游本地只读源码：C:/Users/38788/.cargo/git/checkouts/pi-rust-ba8012a5f17fe953/300c1f3。tests/common/mod.rs给出低层config构造示例，下一步独立编写行为测试，不复制整个测试套件。
