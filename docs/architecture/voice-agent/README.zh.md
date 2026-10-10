# voice-agent 文档目录 (AI Mate Voice Agent Docs)

AI Mate 无头伴侣主机语音交互方案的权威文档集。两层结构：**一份架构基线 + 两份子规范**，外加一份历史底稿归档。

| 文档 | 定位 |
| :--- | :--- |
| [interaction.zh.md](interaction.zh.md) | **架构基线（唯一生效标准）**：背景与痛点、五模块边界、无锁声学管线、双状态机、单次四路路由契约、工具策略安全红线、ADR 矩阵与 Phase 0-4 验收门禁 |
| [model-management.zh.md](model-management.zh.md) | 子规范：模型资产统一目录拓扑、双重强校验、双源探针降级链、`ModelManager` 生命周期状态机与桌面/无屏/离线三场景分发 |
| [ui-motion.zh.md](ui-motion.zh.md) | 子规范：动效令牌总线、设置页行契约与状态视觉映射、无屏配对页契约、无障碍降级与反模式红线 |
| [interaction.archive.zh.md](interaction.archive.zh.md) | 历史全量底稿归档（仅供追溯审计，不构成执行依据） |
| [execution/](execution/README.zh.md) | **执行计划**：骨架先行五波次拆分、多 Agent 并行分工、TDD 纵向测试集与 bsk 浏览器验证规程（面向执行，不定义验收指标） |

阅读顺序：先读基线建立全局，再按需深入子规范；测试用例（TC-AM-xxx）与阶段门禁（Gate S ~ Gate 4）统一收敛在基线 §5，子规范只做映射引用；落地执行按 execution/ 波次推进。
