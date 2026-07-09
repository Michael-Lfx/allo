# Flowy 智能体能力优化方案

> 基于 Hermes-Agent 对标的深度源码排查，针对记忆系统、进化引擎、自动技能学习、后台复盘四大模块的优化建议。
>
> 排查日期：2026-07-09 · 排查范围：nomifun-companion / nomifun-ai-agent / nomi-memory / nomi-insights-core / nomifun-cron

---

## 一、现状总结

### 1.1 双系统分离架构

Flowy 采用**双系统分离**设计：

| 能力 | 普通对话路径 | 桌面伙伴路径 |
|------|-------------|-------------|
| 记忆存储 | 文件级（.md + MEMORY.md 索引） | SQLite（memory.db，6 类 kind + strength + scope） |
| 记忆提取 | 会话结束 LLM 蒸馏（ENV 门控默认 OFF） | 定时 tick（60s）Learner 读采集事件蒸馏 |
| 进化/技能学习 | 无 | EvolutionEngine：挖矿→起草→评审→物化→自动激活 |
| 事件采集 | 是数据源（Collector 采集 8 类事件） | 消费方（伙伴对话仅挣 XP，不喂 Learner） |
| 后台复盘 | session-end pipeline（POI + Resolution + 工作包） | Archiver 会话窗口归档 + Learner/Evolution tick |
| 外部贡献 | nomi-insights-core（脱敏工作包上传） | 无（伙伴数据不外传） |

### 1.2 核心问题

**数据流单向**：普通对话 → Collector → 伙伴系统（学习/进化），但伙伴系统的产出（记忆、技能）**不能回流到普通对话**。

关键代码证据：
- `nomi.rs#L548-L563`：`companion_sink` 和 `companion_skill_sink` 仅在 `overrides.companion == true` 时注入
- `agent.rs#L197-L207`：`companion_sink.is_some()` → `distill_dir = None`（伙伴红线）
- `skill_sink.rs#L31-L37`：伙伴技能存储为 `SkillScope::Companion(owner)`，非 Shared

---

## 二、优化方案

### 优化 1：打通双系统——让普通对话直接参与进化

#### 问题
进化引擎（EvolutionEngine + Learner）只运行在伙伴系统中。普通对话虽作为数据源被采集，但不能直接产出技能建议。

#### 方案
在普通对话的 session-end pipeline 中接入 EvolutionEngine 的挖矿结果。

#### 涉及模块
- `nomi-insights-core/src/work_session/pipeline.rs`
- `nomifun-companion/src/evolution/engine.rs`
- `nomifun-companion/src/evolution/miner.rs`

#### 实现步骤
1. 在 `spawn_session_end_pipeline` 中增加一个可选的「技能挖矿」阶段
2. 复用 `miner.rs` 的 `mine_candidates` 函数，对当前会话的工具调用序列进行挖矿
3. 挖矿结果以「技能建议」工件形式广播（类似 `skill_suggest.rs` 的 `broadcast_artifact`）
4. 用户可在普通对话侧边栏看到建议，采纳后写入 Shared scope 技能

#### 代码变更概要
```rust
// pipeline.rs — spawn_session_end_pipeline 增加挖矿阶段
if insights_cfg.enabled && insights_cfg.skill_mining_enabled {
    let mine_result = mine_session_tools(&messages).await;
    if let Some(candidate) = mine_result {
        broadcast_skill_candidate(&broadcaster, &session_id, candidate).await;
    }
}
```

#### 预期收益
- 普通对话不再仅是数据源，也能直接产出技能建议
- 用户在普通对话中也能看到「检测到重复工具调用模式，建议固化为技能」

---

### 优化 2：引入 per-turn 后台审查

#### 问题
普通对话的记忆蒸馏仅在会话结束时触发（session-end），时效性不如 Hermes 的 per-turn fork 审查。

#### 方案
借鉴 Hermes 的 `background_review` 模式，在每轮后 fork 一个轻量 LLM 审查「是否应更新记忆/技能」。

#### 涉及模块
- `nomifun-ai-agent/src/manager/nomi/agent.rs`（PostTurnHook 扩展）
- `nomifun-ai-agent/src/capability/session_lifecycle.rs`（新增 hook 类型）
- 新增 `nomifun-ai-agent/src/capability/turn_review.rs`

#### 实现步骤
1. 新增 `PostTurnReviewHook` trait，在每轮结束后异步触发
2. 复用 `distill.rs` 的 `DISTILL_SYSTEM` prompt，但改为轻量版（max_tokens=1024，仅提取 1-2 条高信号记忆）
3. 同模型时复用 warm cache（不额外 cold-write），异模型时用 compact digest
4. 审查结果写入文件级记忆（与 session-end distill 共享目录）

#### 代码变更概要
```rust
// session_lifecycle.rs — 新增 hook
#[async_trait]
pub trait PostTurnReviewHook: Send + Sync {
    async fn on_post_turn_review(&self, ctx: &TurnContext<'_>, reply: &str, messages: &[Value]);
}

// turn_review.rs — 轻量蒸馏
pub struct LightweightTurnReviewer {
    cfg: Arc<Config>,
    memory_dir: PathBuf,
}

impl PostTurnReviewHook for LightweightTurnReviewer {
    async fn on_post_turn_review(&self, ctx: &TurnContext<'_>, reply: &str, messages: &[Value]) {
        // 仅对人类发起的轮次触发
        if !ctx.origin_is_human { return; }
        // 轻量蒸馏：max_tokens=1024，最多 2 条记忆
        tokio::spawn(run_lightweight_distill(self.cfg.clone(), self.memory_dir.clone(), transcript));
    }
}
```

#### 预期收益
- 记忆提取从会话级提升到轮次级，时效性大幅提升
- 同模型复用 warm cache，额外成本仅为输出 tokens

---

### 优化 3：复盘产出闭环化——Resolution 信号驱动进化

#### 问题
Resolution Verdict Engine 产出高质量的会话质量信号（9 维信号 + 6 级 verdict + 4 级 tier），但这些信号仅用于外部贡献工作包，不反馈到本地进化系统。

#### 方案
将 Resolution 信号反馈到 Learner 和 EvolutionEngine，让会话质量驱动进化优先级。

#### 涉及模块
- `nomi-insights-core/src/work_session/resolution.rs`（信号产出方）
- `nomifun-companion/src/evolution/engine.rs`（消费方）
- `nomifun-companion/src/learner.rs`（消费方）

#### 实现步骤
1. 在 `ResolutionPayload` 中增加 `session_skill_signals` 序列化输出
2. 将 Resolution 信号写入 `collected_events` 表（新事件类型 `conversation_lifecycle.resolution`）
3. EvolutionEngine 的挖矿优先级受 verdict 影响：
   - `failed` 会话 → 工具调用模式优先挖矿（可能是反复试错的模式）
   - `solved_confirmed` 会话 → 工具调用模式高置信度挖矿
   - `correction_loops > 0` → 降低挖矿置信度（用户纠正过的模式）
4. Learner 的蒸馏权重受 verdict 影响：
   - `solved_confirmed` → 事件权重 ×1.5
   - `failed` → 事件权重 ×0.5（避免从失败中学习错误模式）

#### 代码变更概要
```rust
// engine.rs — process_candidate 增加 verdict 加权
fn confidence_boost(verdict: &str, correction_loops: u32) -> f64 {
    match verdict {
        "solved_confirmed" => 1.5,
        "solved_inferred" => 1.0,
        "failed" if correction_loops > 0 => 0.3, // 用户纠正过的模式降低置信度
        "failed" => 0.5,
        _ => 1.0,
    }
}
```

#### 预期收益
- 失败会话的工具调用模式不被错误地固化为技能
- 成功会话的高效工具序列被优先挖掘
- Resolution 信号不再仅用于外部贡献，也驱动本地进化

---

### 优化 4：伙伴技能提升到 Shared scope

#### 问题
EvolutionEngine 创建的技能存储为 `SkillScope::Companion(owner)`，普通对话无法访问。即使伙伴学到了有用的技能，普通对话也不能使用。

#### 方案
增加「技能提升」机制：允许将高置信度、高使用频率的伙伴技能提升到 Shared scope，使其对所有对话可用。

#### 涉及模块
- `nomifun-companion/src/evolution/engine.rs`（提升触发）
- `nomifun-companion/src/store.rs`（scope 变更）
- `nomifun-companion/src/skill_sink.rs`（Shared scope 回退已支持）
- `nomifun-extension/src/skill_service.rs`（文件迁移）

#### 实现步骤
1. 在 `decay_skills` 或 EvolutionEngine tick 中检查提升条件：
   - `source == "mined"` && `status == "active"` && `usage_count >= 5` && `strength >= 0.8`
2. 将技能文件从 `SkillScope::Companion(owner)` 复制到 `SkillScope::Shared`
3. 更新 DB 行：`scope_kind = "user"`, `scope_companion_id = ""`
4. 保留原记录的 provenance（标注来源伙伴）

#### 代码变更概要
```rust
// store.rs — 新增 promote_to_shared
pub async fn promote_skill_to_shared(&self, companion_id: &str, name: &str) -> Result<(), AppError> {
    // 1. 检查提升条件
    let skill = self.get_skill(companion_id, name).await?;
    if skill.usage_count < 5 || skill.strength < 0.8 { return Ok(()); }
    
    // 2. 文件迁移：Companion scope → Shared scope
    let from_scope = SkillScope::Companion(companion_id.to_owned());
    let to_scope = SkillScope::Shared;
    skill_service::copy_skill(&self.skill_paths, &from_scope, &to_scope, name).await?;
    
    // 3. DB 更新
    sqlx::query("UPDATE companion_skills SET scope_kind='user', scope_companion_id='' WHERE ...")
        .execute(&self.pool).await?;
    Ok(())
}
```

#### 预期收益
- 高频使用的伙伴技能自动提升为全局技能
- 普通对话也能受益于伙伴自进化的成果
- 打通「伙伴学习 → 全局复用」的数据回流

---

### 优化 5：distill 从 ENV 门控改为配置项

#### 问题
普通对话的记忆蒸馏由环境变量 `NOMIFUN_MEMORY_DISTILL` 控制，默认 OFF。这导致该功能实际几乎不运行——用户不知道需要设置环境变量。

#### 方案
将 distill 门控从环境变量迁移到 `nomi-config` 的配置项，默认 ON（或提供 UI 开关）。

#### 涉及模块
- `nomi-config/src/config.rs`（新增 memory 配置段）
- `nomifun-ai-agent/src/manager/nomi/distill.rs`（读取配置而非 ENV）
- `nomifun-ai-agent/src/manager/nomi/agent.rs`（传递配置）

#### 实现步骤
1. 在 `nomi-config` 的 `Config` 中新增 `memory` 配置段：
   ```toml
   [memory]
   distill_enabled = true       # 默认 ON
   distill_max_tokens = 2048
   distill_provider_id = ""     # 可选：指定蒸馏用的模型
   distill_model = ""
   ```
2. `distill.rs` 的 `distill_enabled()` 改为读取配置
3. 前端 System Settings 增加「会话记忆蒸馏」开关

#### 代码变更概要
```rust
// distill.rs — 改为读取配置
pub fn distill_enabled(cfg: &Config) -> bool {
    cfg.memory.distill_enabled
    // 兼容：ENV 仍可覆盖
    || std::env::var("NOMIFUN_MEMORY_DISTILL").map(|v| v == "1").unwrap_or(false)
}
```

#### 预期收益
- 普通对话记忆蒸馏默认可用
- 用户可通过 UI 控制开关，无需设置环境变量

---

### 优化 6：技能支撑文件分层

#### 问题
EvolutionEngine 产出的技能仅有一个 SKILL.md 文件，缺少 Hermes 的 references/templates/scripts 三层支撑结构。

#### 方案
为伙伴技能草稿增加支撑文件目录结构。

#### 涉及模块
- `nomifun-companion/src/evolution/engine.rs`（草稿生成时创建目录）
- `nomifun-extension/src/skill_service.rs`（目录结构支持）
- `nomifun-companion/src/evolution/prompt.rs`（DRAFT_SYSTEM prompt 增加 references 指令）

#### 实现步骤
1. DRAFT_SYSTEM prompt 增加指令：在 SKILL.md 中声明 `references` / `templates` / `scripts`
2. `create_skill` 时自动创建三个子目录
3. CRITIC_SYSTEM 评审时检查支撑文件是否存在且合理
4. `CompanionSkillStoreSink.load_skill_body` 支持加载引用文件

#### 预期收益
- 技能可携带示例文件、模板、可执行脚本
- 技能复用性大幅提升

---

### 优化 7：学习图谱可视化

#### 问题
伙伴学到的技能和记忆没有可视化展示，用户难以理解伙伴学到了什么。

#### 方案
借鉴 Hermes 的 Learning Graph，将伙伴技能 + 记忆的关联关系可视化。

#### 涉及模块
- 新增 `ui/src/renderer/pages/nomi/tabs/LearningGraphTab.tsx`
- `nomifun-companion/src/service.rs`（新增图谱数据 API）

#### 实现步骤
1. 后端新增 `/api/companions/:id/learning-graph` 端点
2. 返回技能节点（name/source/strength/usage_count/last_used）+ 记忆节点（kind/content/strength/created_at）
3. 边的派生：
   - 技能→技能：同名技能在不同 source 间的进化链
   - 记忆→技能：记忆 content 与技能 name/description 的词汇重叠
4. 前端使用 D3.js 或 @antv/G6 渲染力导向图

#### 预期收益
- 用户可直观看到伙伴学到了什么
- 技能进化链可视化（draft → active → superseded）
- 记忆与技能的关联关系清晰

---

### 优化 8：本地使用洞察

#### 问题
Flowy 缺少面向用户的本地 Token/成本/工具使用趋势分析。insights 模块仅用于外部贡献。

#### 方案
借鉴 Hermes 的 `insights.py`，在现有贡献管道之外增加面向用户的本地分析。

#### 涉及模块
- 新增 `nomifun-insights/src/local_analytics.rs`
- `ui/src/renderer/pages/nomi/tabs/AnalyticsTab.tsx`

#### 实现步骤
1. 从 `conversations` 表 + `companion_skills` 表 + `companion_memories` 表聚合数据
2. 生成报告：
   - 会话维度：每日会话数、平均轮次、平均工具调用数
   - 技能维度：技能使用 Top-N、技能创建/归档趋势
   - 记忆维度：记忆增长趋势、按 kind 分布
   - 进化维度：EvolutionEngine tick 频率、草稿→active 转化率
3. 前端展示为图表（折线图 + 饼图 + 柱状图）

#### 预期收益
- 用户可看到伙伴进化的投入产出比
- 帮助用户决定哪些采集源值得开启

---

### 优化 9：知识库作为双系统桥接

#### 问题
知识库系统是唯一能跨系统共享的结构化知识载体，但目前伙伴创建的知识库绑定到 `kind="companion"`，不自动对普通对话可用。

#### 方案
增加「知识库推荐」机制：当伙伴通过 Learner 产出 `create_skill` 或 `insight` 建议时，同时推荐创建知识库并绑定到 workpath scope。

#### 涉及模块
- `nomifun-companion/src/learner.rs`（建议产出时附带知识库推荐）
- `nomifun-companion/src/service.rs`（知识库创建 API）
- `nomifun-knowledge/src/binding.rs`（workpath scope 绑定）

#### 实现步骤
1. Learner 的 `create_skill` 建议增加 `knowledge_base` 可选字段
2. 用户采纳建议时，自动创建知识库并绑定到当前 workpath
3. 普通对话在相同 workpath 下自动加载该知识库
4. 伙伴后续的 Learner 蒸馏结果可回写（knowledge_write）到该知识库

#### 预期收益
- 伙伴学到的知识可通过知识库回流到普通对话
- 不破坏双系统分离架构（通过显式的知识库绑定实现）
- 用户可控：可选择不绑定

---

### 优化 10：Collector 采集源智能推荐

#### 问题
Collector 的 8 个事件源默认全 OFF（仅 companion_dialogues ON），用户不知道该开启哪些。

#### 方案
基于 Learner 的学习效果反馈，智能推荐开启采集源。

#### 涉及模块
- `nomifun-companion/src/collector.rs`（采集源效果统计）
- `nomifun-companion/src/service.rs`（推荐 API）
- `ui/src/renderer/pages/nomi/tabs/CollectorTab.tsx`

#### 实现步骤
1. 每个 Learner tick 记录「本次学习产出的记忆/建议来自哪些事件源」
2. 聚合统计：每个事件源的贡献度（产出记忆数 / 产出建议数 / reinforce 命中率）
3. 前端展示采集源效果仪表盘
4. 当某事件源未开启但其对应的工作类型频繁发生时，弹出推荐提示

#### 预期收益
- 用户可数据驱动地决定开启哪些采集源
- 避免全开导致的噪声和 token 成本

---

## 三、实施优先级

| 优先级 | 优化项 | 难度 | 预期收益 | 依赖 |
|--------|--------|------|---------|------|
| P0 | 优化 5：distill 改为配置项 | 低 | 高 | 无 |
| P0 | 优化 4：伙伴技能提升到 Shared | 中 | 高 | 无 |
| P1 | 优化 3：Resolution 信号驱动进化 | 中 | 高 | 无 |
| P1 | 优化 1：普通对话接入挖矿 | 中 | 高 | 优化 4 |
| P1 | 优化 2：per-turn 后台审查 | 高 | 高 | 优化 5 |
| P2 | 优化 9：知识库桥接 | 中 | 中 | 无 |
| P2 | 优化 6：技能支撑文件分层 | 中 | 中 | 无 |
| P2 | 优化 10：采集源智能推荐 | 低 | 中 | 无 |
| P3 | 优化 7：学习图谱可视化 | 中 | 中 | 无 |
| P3 | 优化 8：本地使用洞察 | 中 | 中 | 无 |

---

## 四、风险与约束

### 4.1 隐私红线不可逾越
- 工具调用仅记录 name + param SHAPE，不记录值——此约束在任何优化中不可放宽
- distill 的双重 redact 门（transcript + 每字段）必须保留
- Collector 的防自我强化红线（排除 agent 驱动轮次）不可移除

### 4.2 性能约束
- per-turn 审查（优化 2）必须异步 fire-and-forget，不得阻塞主对话流
- EvolutionEngine tick（60s）不得因新增逻辑而显著延长
- session-end pipeline 的新增阶段（优化 1）必须可配置关闭

### 4.3 兼容性
- 优化 5（distill 配置化）需兼容旧的 ENV 门控
- 优化 4（技能提升）需处理已存在的 Companion scope 技能
- 所有优化不得破坏现有的 companion red line（伙伴会话不 distill）

---

## 五、附录：源码排查索引

| 模块 | 关键文件 | 行数 | 核心功能 |
|------|---------|------|---------|
| EvolutionEngine | `nomifun-companion/src/evolution/engine.rs` | 859 | tick→衰减→挖矿→起草→评审→物化→自动激活 |
| Miner | `nomifun-companion/src/evolution/miner.rs` | 360 | 确定性挖矿：工具名序列滑窗[2-5步]→跨会话去重 |
| Evolution Prompt | `nomifun-companion/src/evolution/prompt.rs` | 177 | DRAFT_SYSTEM / CRITIC_SYSTEM / MERGE_SYSTEM |
| Learner | `nomifun-companion/src/learner.rs` | 494 | tick→读事件→LLM蒸馏→记忆/建议/情绪/日记/XP |
| Learn Prompt | `nomifun-companion/src/prompt.rs` | 312 | LEARN_SYSTEM（6类记忆+7类建议）/ ARCHIVE_SYSTEM |
| Collector | `nomifun-companion/src/collector.rs` | 1277 | 8事件源采集+防自我强化+工具调用隐私 |
| Archiver | `nomifun-companion/src/archiver.rs` | 435 | 会话窗口归档→日摘要→注入新窗口 |
| Skill Sink | `nomifun-companion/src/skill_sink.rs` | 81 | 伙伴技能→agent引擎自动使用 |
| Companion Config | `nomifun-companion/src/config.rs` | 244 | 采集源配置+学习配置+外观+人格 |
| Gamify | `nomifun-companion/src/gamify.rs` | 24 | XP→Level(√曲线) |
| Companion Store | `nomifun-companion/src/store.rs` | 2495 | memory.db：记忆+建议+技能+状态+窗口 |
| Companion Prompt | `nomifun-companion/src/companion.rs` | 1166 | build_companion_system_prompt+记忆注入+日摘要注入 |
| Companion Service | `nomifun-companion/src/service.rs` | 1200+ | 技能审批+状态查询+memory_sink/skill_sink构建 |
| Distill (orchestration) | `nomifun-ai-agent/src/manager/nomi/distill.rs` | 115 | ENV门控+双重redact+LLM调用+写入 |
| Distill (pure) | `nomi-memory/src/distill.rs` | 487 | DISTILL_SYSTEM prompt+解析+写入+去重 |
| Nomi Agent Manager | `nomifun-ai-agent/src/manager/nomi/agent.rs` | 1816 | companion red line+工具注册+send_message |
| Agent Factory | `nomifun-ai-agent/src/factory/nomi.rs` | 2310 | companion门控+系统提示+sink注入 |
| Factory Mod | `nomifun-ai-agent/src/factory/mod.rs` | 274 | AgentFactoryDeps+CompanionPromptProvider |
| Session Lifecycle | `nomifun-ai-agent/src/capability/session_lifecycle.rs` | 208 | PreTurnHook+PostTurnHook+SessionEndHook |
| Session-End Pipeline | `nomi-insights-core/src/work_session/pipeline.rs` | 316 | POI→Resolution→工作包 |
| Resolution Engine | `nomi-insights-core/src/work_session/resolution.rs` | 562 | 9维信号→fuse_verdict→hybrid合并→skill boost |
| Skill Maturity | `nomi-insights-core/src/maturity.rs` | 106 | first_seen+content_hash→贡献资格 |
| Skill Suggest | `nomifun-cron/src/skill_suggest.rs` | 362 | SKILL_SUGGEST.md检测+hash去重+广播 |
| Memory Store | `nomi-memory/src/store.rs` | 802 | 文件读写+frontmatter解析+usage回流 |
| Memory Index | `nomi-memory/src/index.rs` | 514 | MEMORY.md索引+截断(200行/25KB) |
| Memory Prompt | `nomi-memory/src/prompt.rs` | 185+ | 记忆访问/推荐前验证/记忆vs其他持久化 |
