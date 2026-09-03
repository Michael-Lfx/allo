//! Two-loop concept-graph agent: the draft is pre-created and INJECTED into
//! the opening user message (no "start" tool — the model never spends a
//! round bootstrapping), the `lg_*` tools (`lg_scope` .. `lg_finish`) drive
//! generation, then audit-gated repair rounds drive publishing.
//!
//! Same layering as `LiveLearningCompleter`: the learning crate holds only
//! [`LearningGraphAgentEngine`]; this crate provides the provider-backed
//! implementation, and the app layer wires it via
//! `LearningService::set_learning_graph_engine`.
//!
//! Why a dedicated loop instead of `run_one_shot_turn`? The one-shot entry
//! caps tool rounds at 8 and returns only the final text; generation needs
//! up to 30 rounds and must observe the draft/publish side effects
//! ([`LoopContext`] slots). The tool loop below is therefore the one-shot
//! core with the round cap, the token budget and the message window
//! parameterized — same fail-closed whitelist execution (an unknown tool
//! name gets an error result and never reaches any other surface).
//!
//! The deterministic audit gate holds the decision power: the generation
//! loop builds, the repair loop fixes exactly what the audit reports, and
//! only `lg_finish` (which re-runs the audit as a hard gate) publishes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use nomi_providers::{LlmProvider, ProviderError, create_provider};
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomifun_common::{AppError, LearningCourseId, ProviderId, UserId};
use nomifun_learning::{
    LearningGraphAgentEngine, LearningGraphRecord, GraphOp, LearningService, NodeQuery,
    SubgraphDirection,
};
use tokio::sync::mpsc;

use crate::factory::provider_config::resolve_provider_config_with_output_limit;
use crate::knowledge_completer::resolve_default_model;
use crate::loop_core::{
    LoopEventSink, REPAIR_LOOP_LIMIT, REPAIR_MAX_ROUNDS, json_compact, log_text, run_agent_loop,
};
use crate::one_shot::{OneShotDeps, OneShotTool, one_shot_handler};

/// 生成循环的轮次上限：学习图本地覆盖共享默认（loop_core 的 50）——200+
/// 节点的构建每轮只落一小批 patch，复杂目标 50 轮必然中途断头。共享常量
/// 不动（course_outline / lesson_content 循环按各自默认值运行）。
const GENERATE_MAX_ROUNDS: usize = 100;

/// 学习图循环自己的每轮 token 预算：构建是长程规划任务，大批量 patch JSON
/// 需要大输出预算，取共享默认（`AGENT_MAX_TOKENS`）的 4 倍。绝不改共享常
/// 量——course_outline / lesson_content 循环按各自默认值运行；该预算在解
/// 析时还会收敛到模型声明的输出上限（见
/// `resolve_provider_config_with_output_limit`）。
const ROUND_TOKEN_BUDGET: u32 = 32768;

/// 本循环的总时长预算（生成 + 修复 + 审计门禁）：200 节点规模的构建要
/// 跑多轮生成与修复循环，共享默认 600s 必然撞墙。超时不是模型可解决的
/// 错误，而是预算墙——本地声明而非改共享常量（调用方本地覆盖，见
/// loop_core 的 `TOTAL_TIMEOUT_SECS`）。超时后草稿与轮次日志存活，续建
/// 接着建。
const GENERATE_TIMEOUT_SECS: u64 = 1800;

/// 取消提示语：以 `LlmEvent::Error` / `ProviderError::Api` 注入。循环的
/// 错误分支不会把它当作可同轮重试的错误（只匹配 malformed JSON），整个
/// 生成以"已取消"失败收场。取消不保留草稿（重试即全新生成）；真实失
/// 败仍保留草稿供续建。学习图与课程大纲两个生成循环共用。
pub(crate) const CANCEL_MESSAGE: &str = "生成已被用户取消";

/// 生成循环共用的 provider 包装：每次 LLM 请求开始前与流转发途中轮询
/// 取消旗标（旗标挂在 [`LearningService`] 的生成注册上，取消端点置位）。
/// 取消在流边界即刻生效——请求前直接拒绝；流中把取消作为 Error 事件注入
/// 并停止转发（下游 receiver 被 drop 后上游发送失败，HTTP 流随之终止）。
pub(crate) struct CancellableProvider {
    pub(crate) inner: Arc<dyn LlmProvider>,
    pub(crate) cancel: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl LlmProvider for CancellableProvider {
    async fn stream(
        &self,
        request: &LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Api {
                status: 499,
                message: CANCEL_MESSAGE.to_owned(),
            });
        }
        let mut rx = self.inner.stream(request).await?;
        let cancel = Arc::clone(&self.cancel);
        let (tx, out) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if cancel.load(Ordering::Relaxed) {
                    let _ = tx.send(LlmEvent::Error(CANCEL_MESSAGE.to_owned())).await;
                    break;
                }
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });
        Ok(out)
    }
}

/// 第一轮 user 消息：任务主题 + 预创建草稿的视图与范围参考。取代旧的
/// `lg_start` / `lg_scope` 两个纯读轮次——同一份信息零成本落进上下文。
/// `previous_rounds` 是上次中断会话的轮次日志（续建时非空），恢复模型对
/// 构建进度的认知。
fn compose_opening_user_text(
    topic: &str,
    draft_view_json: &str,
    scope_reference: Option<&str>,
    previous_rounds: &[String],
) -> String {
    let mut text = format!("{topic}\n\n{draft_view_json}");
    match scope_reference {
        Some(reference) => {
            text.push_str("\n\n【范围参考】\n");
            text.push_str(reference);
        }
        None => text.push_str(
            "\n\n【范围参考】不可用（范围分析降级）；以 lg_audit 的覆盖检查为准",
        ),
    }
    if !previous_rounds.is_empty() {
        text.push_str(&format!(
            "\n\n【上次会话构建日志（{} 轮后中断；接着此进度继续，不要重复已建单元）】",
            previous_rounds.len()
        ));
        for line in previous_rounds {
            text.push_str("\n- ");
            text.push_str(line);
        }
        text.push_str(
            "\n\n建议：先 lg_inspect(full=true) 一次性通读当前全图核对进度，再继续构建。",
        );
    }
    text
}

/// Generation-loop system prompt. The model builds the whole network via
/// the draft tools; the audit gate still has the last word at `lg_finish`.
const GENERATE_AGENT_SYSTEM: &str = r#"你是一名具有教育领域专业知识的专家，任务目标是根据要求通过工具逐步构建出一张学习单元网络图。
- 认知学习单元不一定是原子概念，过度解构原子概念不适合作为人类单次学习行为单元，但也可能有些原子概念本身就足够复杂这种情况可以作为单独节点。不能是太复合的概念，过于复合的概念没有意义不是单次学习行为能够解决的问题，应该拆分成多个节点
- 对每个无依赖节点，自问：一个完全零基础的学习者能否在 30 分钟内学会？若不能，则必须为其补充前置节点或将其拆解
- 要有整体规划，学习线路应该从易到难，不能有过突兀过大的难度曲线变化导致不可控的认知负荷，难度本身有提高的必要但应是渐进的
- 生成的学习单元网络一定要完整，宁愿节点过多不要过少-节点关系没有必要冗余，但应该是无环的复杂网络，如果节点的连接过少大概率是认知切分方式的或连接存在问题
- 复杂场景目标节点数通常 >200，但优先保证认知合理性
- 真实依赖关系优先于难度曲线；当二者冲突时，先保证依赖正确，再通过增加铺垫节点或调整切分来缓解难度跳跃。
【工具使用】
1. 先使用lg_scope，这就是你的参考清单，除了覆盖清单你也可以发挥主观能动性，一切以完成用户目标为标准。
2. 常用 lg_inspect 掌握全局；操作图动手前可以用 lg_query / lg_subgraph 查清单元名的精确写法——patch 里的引用必须与图中名称完全一致，否则整个操作被拒。
3. 分批构建：每批小于 25 个操作，宁可多批，不要超长批次。
4. 整张图全部构建完成后调用 lg_audit 自查

【单元命名】（与单次生成管线同标准）
- 节点大多数 name 是动作句（有必要可以例外）：包含 解/求/证明/推导/比较/判定/构造/区分/计算/应用/理解/辨析/建立/验证/化简/变形/转化/估计/近似/检验/分类/归纳/抽象/训练 等等词以及它们之间的组合存在。  
    - 人对概念的单次学习行为应该是种动作                  
- 螺旋式学习合法：同一主题在不同深度以不同视角出现，完整名称不得完全相同；允许共享主题关键词，但必须通过动作词或限定语体现认知层级差异。
    - 正例： “计算简单导数” / “应用导数解决优化问题” / “证明导数中值定理” 是合法螺旋；
    - “理解导数” / “学习导数” 属于近似且层级不清，应避免。
- 单元是通常 30 分钟内可完成的一个学习会话；极困难的综合课最多 60 分钟。

【依赖契约】
- 充分性（SUFFICIENCY）：pre 是完整的直接前置集合——可以存在可选项但不能因为过少导致无法理解后续内容；不要为了缩短回复而裁剪 pre。
- 收敛：真实知识是 DAG 不是树
- 除真正的无需基础的入门单元外，pre 不得为空。
- 连通性：整个网络必须是一个连通结构；真实依赖处必须交叉链接

【覆盖与规模】
- lg_scope 返回的「最终目标」与「用户起点」是边界：零依赖起点单元必须落在 baseline 之内——baseline 是零基状态时，起点从最原始、最日常可及的概念起步，不得凭空拔高入口难度。
- min 是学习分钟预算：常规 5-30 分钟（软上限，尽量不超过 30），极困难单课最多 60 分钟（硬上限）；超过 60 会被拒。
- 引用名必须恰好等于图中已存在的单元名；拿不准时先 lg_query。

【上交前自查】（调用 lg_audit 自查之前，逐条过一遍）
上交前先用 lg_inspect(full=true) 通读全图，再用 lg_scope / lg_query 以图的当前真实状态重新核对。
1. 起点基础性：无依赖入口是否真的足够基础？尤其当无依赖入口只有一个、或目标本身复杂而全图节点数少于 200 时，着重检查——不要拿一个宏大抽象的概念当可用起点，宁可先补一层更简单的铺垫单元。
2. 覆盖完整性：从用户视角反思覆盖是否完整——scope 清单是下限不是上限，用户目标隐含的子领域、必备的常识性概念即使清单没列也要补上；不要只局限于工具给出的范围。
3. 切分与衔接：反思节点划分是否足够细致、相连节点难度是否跳跃过大（过大跳跃意味着中间缺了铺垫单元）；

【结束条件】
- 只有当你确认图已完整覆盖 scope 且 lg_audit 基本健康时，才调用 lg_finish；否则继续构建。"#;

/// Repair-loop system prompt: the audit report is the ONLY repair basis;
/// the model patches locally and never rewrites the graph wholesale.
const REPAIR_AGENT_SYSTEM: &str = r#"你是一名学习图修复代理：基于确定性审计报告，精确修复图中被指出的问题。

【修复原则】
1. 审计报告是主要的修复依据：逐条处理 danger 级 findings，按报告给出的证据（节点名、缺失的大块概念名、孤立组件）精确操作；
2. 不做过大的重构、不删无关节点。（add/link/unlink/set_pre/reverse/split/merge/update/delete）。
3. 动手前可以用 lg_query / lg_subgraph 查清引用名的精确写法；引用不一致会被拒绝。
4. 修复动作分批提交每批 25 个操作以内，每批后用 lg_audit 复查该条 finding 是否消除。
5. 全部 danger 消除后调用 lg_finish 发布。

【常见修复动作对照】
- missing_block_coverage：按报告列出的大块概念名，add 对应单元（把概念改写为动作句）。
- coverage：对照 scope 清单，补齐缺失大块概念的单元。
- disconnected_components：用 link 或在新单元的 pre 中引用，把孤立组件接入主结构。
- orphaned_units：把失去唯一前置的单元重新 link 到它真正的前置；旧的前置边确实画错时，用 set_pre 整体替换该单元的前置（或先 unlink 移除错误边再 link）。
- tree_structure：为确实需要两条以上前线的单元补 link，制造收敛。
- unit_overload：split 超过 60 分钟硬上限的单元；30-60 分钟的偏重单元若可拆也建议拆。

【结束条件】
- 审计无 danger 时调用 lg_finish；若 lg_finish 被拒绝，认真阅读返回的阻塞 findings 并继续修复。
- 禁止空手结束：每一轮都必须调用 lg_patch 执行修复动作（或 lg_audit 复查、lg_finish 尝试发布）；只输出文字而不调用任何工具，会被判定为拒绝修复，整个生成以失败告终。
- 回复使用中文。"#;

/// Provider-backed engine for the two-loop learning-graph pipeline.
pub struct LiveLearningGraphAgentEngine {
    pub service: Arc<LearningService>,
    pub deps: OneShotDeps,
    /// 中断会话的轮次日志（draft_id → 每轮意图行）。resume 时取出注入开
    /// 场消息恢复认知；发布成功后清除。内存态，与草稿同生命周期（重启即
    /// 失）。
    pub round_logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

impl LiveLearningGraphAgentEngine {
    /// Resolve the model pair (explicit override, else the default
    /// resolution) into a live provider handle plus the model's declared
    /// output ceiling — shared by generate/resume.
    async fn resolve_provider(
        &self,
        model_override: Option<(&str, &str)>,
    ) -> Result<(ProviderId, String, Arc<dyn LlmProvider>, Option<u32>), AppError> {
        let (provider_id, model) = match model_override {
            Some((provider_id, model)) => (provider_id.to_owned(), model.to_owned()),
            None => resolve_default_model(&self.deps.provider_repo, &self.deps.provider_model_repo)
                .await
                .ok_or_else(|| {
                    AppError::Conflict(
                        "learning graph generation unavailable: no enabled provider/model is configured"
                            .into(),
                    )
                })?,
        };
        let provider_id: ProviderId = ProviderId::parse(provider_id)
            .map_err(|error| AppError::BadRequest(format!("invalid provider id: {error}")))?;
        let (cfg, output_limit) = resolve_provider_config_with_output_limit(
            &self.deps.provider_repo,
            &self.deps.provider_model_repo,
            &self.deps.encryption_key,
            provider_id.as_str(),
            &model,
            &self.deps.workspace,
        )
        .await?;
        let provider: Arc<dyn LlmProvider> = create_provider(&cfg);
        Ok((provider_id, model, provider, output_limit))
    }

    /// Shared generation body: the draft is created (or located, on resume)
    /// BEFORE the loop and injected into the opening user message together
    /// with the scope reference — the model never spends a round
    /// bootstrapping. Everything else — the timeout shell, the provenance
    /// write and the failure diagnostics — is identical for both entry
    /// points.
    async fn run_generation(
        &self,
        user_id: &UserId,
        topic: &str,
        model_override: Option<(&str, &str)>,
        resume_draft: Option<String>,
    ) -> Result<LearningGraphRecord, AppError> {
        let (provider_id, model, provider, output_limit) =
            self.resolve_provider(model_override).await?;
        // 请求预算用本循环的 ROUND_TOKEN_BUDGET（4×），但绝不越过模型声明
        // 的输出上限——目录/模型行给出更小的 output_limit 时自动降档到该
        // 上限（严格网关会对超限请求直接 400）。思考模式禁用（实验中；开
        // 启版本为 round_budget/2 预算的 Enabled，见 dad18d00d）。
        let round_budget =
            output_limit.map_or(ROUND_TOKEN_BUDGET, |ceiling| ROUND_TOKEN_BUDGET.min(ceiling));
        let thinking = ThinkingConfig::Disabled;
        // 取消旗标：挂在服务端生成注册上，取消端点置位；包装后的 provider
        // 在每次 LLM 请求边界轮询（见 `CancellableProvider`）。
        let provider: Arc<dyn LlmProvider> = Arc::new(CancellableProvider {
            inner: provider,
            cancel: self.service.generation_cancel_flag(),
        });
        tracing::info!(
            topic,
            provider = provider_id.as_str(),
            model = %model,
            resumed = resume_draft.is_some(),
            "learning graph generation start"
        );

        // ── 草稿注入 ── 循环开始前创建（或续建时定位）草稿，把草稿视图与
        // 范围参考直接拼进第一轮 user 消息：模型不再花整轮调用“启动”工具
        // （思考开启下每轮都是几十秒的实打实开销），“忘记建草稿”这一故
        // 障模式随之消失。lg_scope 保留为只读工具，供建图后期（上下文衰减
        // 时）重取参考清单。
        // 注入含一次范围分析 one-shot 调用，在总时长壳之外——单独加 60s
        // 兜底：分析挂住时不能让 HTTP 永久悬空、连超时错误都给不出。
        let (draft_id, draft_view_json, scope_reference, previous_rounds) =
            tokio::time::timeout(
                std::time::Duration::from_secs(60),
                async {
                    let (draft_id, draft_view_json) = match resume_draft.as_ref() {
                        Some(draft_id) => {
                            let view =
                                self.service.inspect_learning_graph_draft(draft_id)?;
                            let mut draft_json = json_compact(&view);
                            // 续建附全图清单（行式紧凑文本）：概览只有节点
                            // 数等数字，模型看不到具体单元无从接续，会把大
                            // 图误判为待从零构建。
                            if let Ok(dump) = self.service.dump_learning_graph_draft(draft_id)
                            {
                                if !dump.is_empty() {
                                    draft_json.push_str(
                                        "\n\n【当前全图清单】名称 [分钟] <- 前置：\n",
                                    );
                                    draft_json.push_str(&dump);
                                }
                            }
                            (draft_id.clone(), draft_json)
                        }
                        None => {
                            let view = self
                                .service
                                .create_learning_graph_draft(
                                    topic,
                                    None,
                                    Some((&provider_id, model.as_str())),
                                )
                                .await?;
                            (view.draft_id.clone(), json_compact(&view))
                        }
                    };
                    let scope_reference = self
                        .service
                        .scope_reference_learning_graph_draft(&draft_id)
                        .ok()
                        .flatten();
                    // 续建时取出上次会话的轮次日志（中断前的计划轨迹）注入
                    // 开场，恢复模型对「做到哪了、接下来干什么」的认知。
                    let previous_rounds = resume_draft.as_ref().and_then(|draft_id| {
                        self.round_logs
                            .lock()
                            .ok()
                            .and_then(|mut logs| logs.remove(draft_id))
                    });
                    Ok::<_, AppError>((
                        draft_id,
                        draft_view_json,
                        scope_reference,
                        previous_rounds,
                    ))
                },
            )
            .await
            .map_err(|_| {
                AppError::Internal("草稿准备超时：范围分析未在 60 秒内完成，请重试".into())
            })??;
        let user_text = compose_opening_user_text(
            topic,
            &draft_view_json,
            scope_reference.as_deref(),
            previous_rounds.as_deref().unwrap_or(&[]),
        );

        // The two slots are shared with the tool handlers: the draft injected
        // above and the record `lg_finish` published. On timeout they also
        // carry the diagnostics (draft id -> live audit report).
        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&self.service),
            user_id: user_id.clone(),
            model_override: Some((provider_id, model.clone())),
            draft_slot: Arc::new(Mutex::new(Some(draft_id))),
            published_slot: Arc::new(Mutex::new(None)),
            round_log: Mutex::new(Vec::new()),
        });

        // 注入完成即广播一条可见事件：第一轮 LLM（思考开启）要 1-2 分钟才
        // 产生首个 round 事件，此前进度面板不能是一片空白。
        ctx.log(
            "draft_ready",
            serde_json::json!({
                "phase": "started",
                "text": "草稿与范围参考已就绪，开始构建学习单元网络…",
            }),
        );

        match tokio::time::timeout(
            std::time::Duration::from_secs(GENERATE_TIMEOUT_SECS),
            self.run_loops(
                provider,
                &model,
                &user_text,
                Arc::clone(&ctx),
                round_budget,
                thinking,
            ),
        )
        .await
        {
            Ok(Ok(record)) => {
                ctx.log("session_end", serde_json::json!({
                    "ok": true,
                    "phase": "completed",
                    "record_id": record.id,
                    "nodes": record.graph.nodes.len(),
                    "edges": record.graph.edges.len(),
                }));
                // Persist the generation provenance next to the audit
                // snapshot (best-effort: diagnostics only).
                if let Ok(course_id) = parse_course_id(&record.id) {
                    let _ = self
                        .service
                        .record_learning_graph_generation(
                            &course_id,
                            ctx.model_override.as_ref().map(|(p, _)| p.as_str()).unwrap_or(""),
                            &model,
                        )
                        .await;
                }
                // 发布成功：会话结束，清掉可能残留的轮次日志。
                if let Some(draft_id) = ctx
                    .draft_slot
                    .lock()
                    .ok()
                    .and_then(|slot| slot.clone())
                {
                    if let Ok(mut logs) = self.round_logs.lock() {
                        logs.remove(&draft_id);
                    }
                }
                Ok(record)
            }
            Ok(Err(error)) => {
                // Attach the surviving draft's context (the timeout branch's
                // behavior): a mid-flight failure may leave dozens of built
                // units behind — the message must say they are still alive.
                // 用户取消例外：只留一句中性终态消息。
                let error = attach_draft_context(&self.service, &ctx, error);
                let draft_id = ctx.draft_slot.lock().ok().and_then(|slot| slot.clone());
                // 取消不保留草稿：草稿与轮次日志一并丢弃，重试即全新生成；
                // 其余失败仍把轮次日志归档到草稿名下，续建时注入开场恢复
                // 认知。
                let cancelled = self.service.generation_cancel_requested()
                    || matches!(&error, AppError::BadGateway(message) if message.contains(CANCEL_MESSAGE));
                match (cancelled, draft_id) {
                    (true, Some(draft_id)) => {
                        // 草稿已在 attach_draft_context 中丢弃，这里只清轮次日志。
                        if let Ok(mut logs) = self.round_logs.lock() {
                            logs.remove(&draft_id);
                        }
                    }
                    (false, Some(draft_id)) => {
                        // 轮次日志归档到草稿名下：续建时注入开场恢复认知。
                        if let (Ok(mut logs), Ok(round_log)) =
                            (self.round_logs.lock(), ctx.round_log.lock())
                        {
                            logs.insert(draft_id, round_log.clone());
                        }
                    }
                    _ => {}
                }
                ctx.log("session_end", serde_json::json!({
                    "ok": false,
                    "phase": "failed",
                    "error": error.to_string(),
                }));
                Err(error)
            }
            Err(_) => {
                let mut message = format!(
                    "生成进行到 {GENERATE_TIMEOUT_SECS}s 达到本次会话的时长预算——这不是图的问题，是会话预算墙"
                );
                if let Some(draft_id) = ctx
                    .draft_slot
                    .lock()
                    .ok()
                    .and_then(|slot| slot.clone())
                {
                    // 轮次日志归档：续建时接着此进度继续。
                    if let (Ok(mut logs), Ok(round_log)) =
                        (self.round_logs.lock(), ctx.round_log.lock())
                    {
                        logs.insert(draft_id.clone(), round_log.clone());
                    }
                    match self.service.audit_learning_graph_draft(&draft_id) {
                        Ok(audit) => {
                            message.push_str(&format!(
                                "\ndraft {draft_id} survives（可续建，重试即接续本进度）; its audit state:\n{audit}"
                            ));
                        }
                        Err(_) => {
                            message.push_str(&format!(
                                "\ndraft {draft_id} survives（可续建，重试即接续本进度）; audit unavailable"
                            ));
                        }
                    }
                }
                ctx.log("session_end", serde_json::json!({
                    "ok": false,
                    "phase": "failed",
                    "error": "timeout",
                }));
                Err(AppError::Internal(message))
            }
        }
    }
}

#[async_trait::async_trait]
impl LearningGraphAgentEngine for LiveLearningGraphAgentEngine {
    async fn generate(
        &self,
        user_id: &UserId,
        topic: &str,
        model_override: Option<(&str, &str)>,
    ) -> Result<LearningGraphRecord, AppError> {
        self.run_generation(user_id, topic, model_override, None).await
    }

    async fn resume(
        &self,
        user_id: &UserId,
        draft_id: &str,
        topic: &str,
        model_override: Option<(&str, &str)>,
    ) -> Result<LearningGraphRecord, AppError> {
        self.run_generation(user_id, topic, model_override, Some(draft_id.to_owned()))
            .await
    }
}

impl LiveLearningGraphAgentEngine {
    /// Generation loop, then audit-gated repair loops. `provider` is
    /// injected so tests stub the LLM here (same seam as
    /// `run_one_shot_turn_with_provider`). `max_tokens`/`thinking` are the
    /// resolved per-round budget and thinking policy — the learning-graph
    /// loop reasons and clamps its budget to the model's output ceiling.
    async fn run_loops(
        &self,
        provider: Arc<dyn LlmProvider>,
        model: &str,
        user_text: &str,
        ctx: Arc<LoopContext>,
        max_tokens: u32,
        thinking: ThinkingConfig,
    ) -> Result<LearningGraphRecord, AppError> {
        // ── Generation loop: full tool set, the injected opening message
        // (topic + draft view + scope reference) as the user turn ──
        ctx.log("generate_loop_start", serde_json::json!({
            "phase": "generating",
            "max_rounds": GENERATE_MAX_ROUNDS,
            "tool_count": learning_graph_tools(Arc::clone(&ctx)).len(),
        }));
        let generate_tools = learning_graph_tools(Arc::clone(&ctx));
        let loop_result = run_agent_loop(
            provider.clone(),
            model,
            GENERATE_AGENT_SYSTEM,
            user_text,
            &generate_tools,
            GENERATE_MAX_ROUNDS,
            max_tokens,
            thinking.clone(),
            "generate",
            Some(ctx.as_ref()),
        )
        .await;
        // lg_finish 可能在循环的最后一轮成功发布（预算恰好在发布轮耗尽）
        // ——发布优先于任何循环错误：已入库的成果不能被预算墙吞掉。
        if let Some(record) = take_published(&ctx) {
            ctx.log("publish_ok", serde_json::json!({
                "phase": "publishing",
                "loop": "generate",
                "record_id": record.id,
            }));
            return Ok(record);
        }
        let final_text = loop_result
            .map_err(|error| attach_draft_context(&self.service, &ctx, error))?;
        let draft_id = ctx
            .draft_slot
            .lock()
            .map_err(|_| AppError::Internal("learning graph draft slot poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                ctx.log("generate_loop_end", serde_json::json!({
                    "ok": false,
                    "reason": "no draft was ever created (the pre-loop draft injection went missing)",
                    "text": log_text(&final_text),
                }));
                AppError::Internal(
                    "learning graph agent finished without creating a draft (the pre-loop draft injection went missing — internal error)"
                        .into(),
                )
            })?;
        ctx.log("generate_loop_end", serde_json::json!({
            "ok": true,
            "draft_id": draft_id,
            "text": log_text(&final_text),
        }));

        // ── Repair loops: the audit gate has the last word ──────────────
        // `finish_learning_graph_draft` IS the deterministic gate: success
        // means the graph cleared it, UnprocessableEntity means danger
        // findings remain and the repair loop gets the full report.
        // `idle_nudge` carries a warning into the next loop when the model
        // ended a repair round without touching the draft (revision
        // unchanged) — models may otherwise "reply, not repair".
        let mut idle_nudge: Option<String> = None;
        for round in 0..REPAIR_LOOP_LIMIT {
            ctx.log("repair_loop_start", serde_json::json!({
                "phase": "repairing",
                "round": round + 1,
                "draft_id": draft_id,
            }));
            match ctx
                .service
                .finish_learning_graph_draft(&ctx.user_id, &draft_id)
                .await
            {
                Ok(record) => {
                    ctx.log("publish_ok", serde_json::json!({
                        "phase": "publishing",
                        "loop": "repair",
                        "round": round + 1,
                        "record_id": record.id,
                    }));
                    return Ok(record);
                }
                Err(AppError::UnprocessableEntity(_)) => {
                    ctx.log("finish_blocked", serde_json::json!({
                        "round": round + 1,
                        "draft_id": draft_id,
                    }));
                }
                Err(error) => return Err(error),
            }
            let audit = ctx.service.audit_learning_graph_draft(&draft_id)?;
            let repair_user = match &idle_nudge {
                Some(nudge) => format!("{nudge}\n\n{audit}"),
                None => audit,
            };
            let repair_tools = learning_graph_tools(Arc::clone(&ctx));
            let revision_before = ctx
                .service
                .inspect_learning_graph_draft(&draft_id)?
                .revision;
            let loop_result = run_agent_loop(
                provider.clone(),
                model,
                REPAIR_AGENT_SYSTEM,
                &repair_user,
                &repair_tools,
                REPAIR_MAX_ROUNDS,
                max_tokens,
                thinking.clone(),
                "repair",
                Some(ctx.as_ref()),
            )
            .await;
            // 发布优先于循环错误（同 generate 循环）。
            if let Some(record) = take_published(&ctx) {
                ctx.log("publish_ok", serde_json::json!({
                    "phase": "publishing",
                    "loop": "repair",
                    "round": round + 1,
                    "record_id": record.id,
                }));
                return Ok(record);
            }
            let final_text = loop_result
                .map_err(|error| attach_draft_context(&self.service, &ctx, error))?;
            let revision_after = ctx
                .service
                .inspect_learning_graph_draft(&draft_id)?
                .revision;
            if revision_after == revision_before {
                ctx.log("repair_loop_idle", serde_json::json!({
                    "round": round + 1,
                    "revision": revision_after,
                    "text": log_text(&final_text),
                }));
                idle_nudge = Some(format!(
                    "警告：你上一轮没有对草稿做任何修改（revision 仍是 {revision_after}），只回复了文字。\
                     禁止空手结束：本轮必须调用 lg_patch 执行修复动作，或以 lg_finish 尝试发布；\
                     只输出文字而不调用任何工具，会被判定为拒绝修复，整个生成将以失败告终。"
                ));
            } else {
                idle_nudge = None;
            }
            ctx.log("repair_loop_end", serde_json::json!({
                "round": round + 1,
                "draft_id": draft_id,
                "revision": revision_after,
                "text": log_text(&final_text),
            }));
        }

        // Budget exhausted: report honestly with the surviving findings
        // (the draft is kept, so a human or a later run can continue).
        let audit = ctx.service.audit_learning_graph_draft(&draft_id)?;
        ctx.log("repair_budget_exhausted", serde_json::json!({
            "draft_id": draft_id,
        }));
        Err(AppError::UnprocessableEntity(format!(
            "learning graph agent exhausted {REPAIR_LOOP_LIMIT} repair loops; the draft survives with these blocking findings:\n{audit}"
        )))
    }
}

// ── Shared loop state ──────────────────────────────────────────────────────

/// Everything the tool handlers need, captured once per generation. The two
/// slots are the only mutable cross-round state: which draft is active and
/// which record (if any) was published by `lg_finish`.
struct LoopContext {
    service: Arc<LearningService>,
    user_id: UserId,
    model_override: Option<(ProviderId, String)>,
    draft_slot: Arc<Mutex<Option<String>>>,
    published_slot: Arc<Mutex<Option<LearningGraphRecord>>>,
    /// 轮次日志：每轮的意图文本与工具摘要（`emit_progress` 的 agent_round
    /// 分支追加）。对话历史无法跨会话保留，这些计划轨迹在续建时注入开场
    /// 消息，恢复模型对「做到哪了、接下来干什么」的认知。
    round_log: Mutex<Vec<String>>,
}

impl LoopContext {
    /// Mirror loop events onto the `learning.course-generation` WebSocket
    /// stream (the shared progress channel — no session files). Best-effort
    /// and never fails the caller.
    fn log(&self, event: &str, fields: serde_json::Value) {
        self.emit_progress(event, &fields);
    }

    /// Translate loop-core log events into `learning.course-generation`
    /// frames, tagged `kind: "learning_graph"`. `agent_round` carries the
    /// loop-core shape (loop/round/text/tool_calls) and is reshaped; events
    /// with a `phase` field pass through with the tag added; anything else
    /// is loop-internal and stays off the wire.
    fn emit_progress(&self, event: &str, fields: &serde_json::Value) {
        let payload = match event {
            "agent_round" => {
                let repair =
                    fields.get("loop").and_then(serde_json::Value::as_str) != Some("generate");
                // 轮次日志：记录每轮意图（工具摘要 + 计划文本），续建时注
                // 入开场消息恢复认知（见 `round_log` 字段注释）。
                let tools_text = fields
                    .get("tool_calls")
                    .and_then(serde_json::Value::as_array)
                    .map(|calls| {
                        calls
                            .iter()
                            .map(|call| {
                                let name =
                                    call.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                                let failed = call
                                    .get("is_error")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                format!("{name}{}", if failed { "✗" } else { "✓" })
                            })
                            .collect::<Vec<_>>()
                            .join(" · ")
                    })
                    .unwrap_or_default();
                let text = fields
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .trim();
                let mut line = format!(
                    "第{}轮({}): {}",
                    fields
                        .get("round")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    if repair { "修复" } else { "构建" },
                    tools_text,
                );
                if !text.is_empty() {
                    let char_count = text.chars().count();
                    let mut brief: String = text.chars().take(120).collect();
                    if char_count > 120 {
                        brief.push('…');
                    }
                    line.push(' ');
                    line.push_str(&brief);
                }
                if let Ok(mut log) = self.round_log.lock() {
                    log.push(line);
                }
                serde_json::json!({
                    "kind": "learning_graph",
                    "phase": "round",
                    "loop": fields.get("loop"),
                    "round": fields.get("round"),
                    "max_rounds": if repair { REPAIR_MAX_ROUNDS } else { GENERATE_MAX_ROUNDS },
                    "tools": fields.get("tool_calls").cloned().unwrap_or_default(),
                    "text": fields.get("text").cloned().unwrap_or_default(),
                })
            }
            "round_feedback" => {
                // 损坏降级上 WS：否则用户会看到轮次凭空从 1 跳到 2，不知道
                // 中间发生过一次传输损坏与自动恢复。复用 round 行渲染。
                serde_json::json!({
                    "kind": "learning_graph",
                    "phase": "round",
                    "loop": fields.get("loop"),
                    "round": fields.get("round"),
                    "tools": [],
                    "text": format!(
                        "工具调用参数损坏，本轮操作未执行——已自动要求模型重新提交（第 {} 次）",
                        fields.get("feedbacks_used").and_then(serde_json::Value::as_u64).unwrap_or(0),
                    ),
                })
            }
            _other if fields.get("phase").is_some() => {
                let mut payload = fields.clone();
                if let Some(object) = payload.as_object_mut() {
                    object.insert("kind".to_owned(), serde_json::json!("learning_graph"));
                }
                payload
            }
            _ => return,
        };
        self.service.emit_course_event(payload);
    }

    fn require_draft(&self) -> Result<String, String> {
        self.draft_slot
            .lock()
            .map_err(|_| "internal draft slot lock failed".to_owned())?
            .clone()
            .ok_or_else(|| "没有活动的草稿（草稿应在循环开始前注入，缺失属于内部错误）".to_owned())
    }
}

impl LoopEventSink for LoopContext {
    fn log(&self, event: &str, fields: serde_json::Value) {
        LoopContext::log(self, event, fields);
    }
}

/// Take the published record out of the slot (once) — called after every
/// loop, because the model may legitimately `lg_finish` from either loop.
fn take_published(ctx: &LoopContext) -> Option<LearningGraphRecord> {
    ctx.published_slot
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

/// Parse the published record id (a `learning_courses.course_id` string)
/// into the typed id for the provenance write. The slot only ever holds a
/// record produced by `finish_learning_graph_draft`, so failure here is
/// impossible in practice — the caller treats it as best-effort regardless.
fn parse_course_id(raw: &str) -> Result<LearningCourseId, String> {
    LearningCourseId::parse(raw).map_err(|error| error.to_string())
}

/// Failure-path diagnostics: append the surviving draft's context to the
/// error message (same as the timeout branch). A mid-flight failure can
/// leave dozens of built units behind; the message must say the draft is
/// still alive and what state it is in, or a resume has no entry point.
/// `UnprocessableEntity` (repair budget exhausted) already carries the full
/// blocking report — it passes through untouched.
///
/// 用户取消走单独出口：那是中性终态而非故障，且取消不保留草稿——就地
/// 丢弃（幂等，覆盖 run_loops 内层与 run_generation 尾部所有调用点），
/// 消息只有一句，无诊断附文。轮次日志的清理由 run_generation 尾部负责。
fn attach_draft_context(
    service: &LearningService,
    ctx: &LoopContext,
    error: AppError,
) -> AppError {
    if service.generation_cancel_requested()
        || matches!(&error, AppError::BadGateway(message) if message.contains(CANCEL_MESSAGE))
    {
        if let Some(draft_id) = ctx.draft_slot.lock().ok().and_then(|slot| slot.clone()) {
            service.discard_learning_graph_draft(&draft_id);
        }
        return AppError::BadGateway(CANCEL_MESSAGE.to_owned());
    }
    let Some(draft_id) = ctx
        .draft_slot
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
    else {
        return error;
    };
    // 幂等：run_loops 内层与 run_generation 外层都会调用本函数，避免同一段
    // 审计转储在用户可见的错误消息里出现两遍。
    if matches!(&error,
        AppError::BadGateway(message) | AppError::Internal(message)
            if message.contains("可续建"))
    {
        return error;
    }
    let note = match service.audit_learning_graph_draft(&draft_id) {
        Ok(audit) => format!(
            "\n\n草稿 {draft_id} 仍在(TTL 1 小时,可续建)——当前审计状态:\n{audit}"
        ),
        Err(_) => format!("\n\n草稿 {draft_id} 仍在(TTL 1 小时,可续建),审计不可用"),
    };
    match error {
        AppError::BadGateway(message) => AppError::BadGateway(format!("{message}{note}")),
        AppError::Internal(message) => AppError::Internal(format!("{message}{note}")),
        other => other,
    }
}

// ── Tool set ───────────────────────────────────────────────────────────────

/// The `lg_*` whitelist. The draft is pre-created before the loop starts
/// (injected into the opening user message), so there is no "start" tool;
/// `lg_scope` is read-only, and an unlisted tool name fails closed at the
/// loop level.
fn learning_graph_tools(ctx: Arc<LoopContext>) -> Vec<OneShotTool> {
    vec![
        lg_inspect(Arc::clone(&ctx)),
        lg_query(Arc::clone(&ctx)),
        lg_subgraph(Arc::clone(&ctx)),
        lg_audit(Arc::clone(&ctx)),
        lg_scope(Arc::clone(&ctx)),
        lg_patch(Arc::clone(&ctx)),
        lg_finish(Arc::clone(&ctx)),
    ]
}

/// Compact JSON without \uXXXX escapes (the default serializer already
/// keeps non-ASCII; this is the single formatting seam for tool replies) and
/// the truncation/stop-reason helpers live in `loop_core` — re-imported here.

fn lg_inspect(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "lg_inspect".into(),
        description: "当前草稿全局概览：节点/边数、总分钟预算、各范围块的单元分布、入口/终点单元、审计摘要。每次 lg_patch 前后调用，保持全局认知；full=true 时附全部单元的紧凑清单（每行一个：名称 [分钟] <- 前置），一次性通读全图——续建恢复进度与上交前自查都用。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "full": { "type": "boolean", "description": "为 true 时在概览后附全部单元与前置的紧凑清单（一次性读取全图；缺省只返回概览）" }
            }
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let view = ctx
                    .service
                    .inspect_learning_graph_draft(&draft_id)
                    .map_err(|error| error.to_string())?;
                if !input
                    .get("full")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                {
                    return Ok(json_compact(&view));
                }
                let dump = ctx
                    .service
                    .dump_learning_graph_draft(&draft_id)
                    .map_err(|error| error.to_string())?;
                Ok(format!("{}\n\n【全图清单】\n{dump}", json_compact(&view)))
            }
        }),
    }
}

fn lg_query(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "lg_query".into(),
        description: "按名称子串 / 重叠词过滤列出单元：返回 id、分钟预算、直接前置与直接后继。任何 lg_patch 之前先查询，确保引用的名字与图中完全一致。limit 上限 200，防止上下文爆炸。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "matcher": { "type": "string", "description": "单元名子串（可选，空=不过滤）" },
                "keyword": { "type": "string", "description": "名称重叠过滤词（如某个大块概念名）；只保留名称与之重叠 >=2 个连续字符的单元（可选）" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "返回条数上限，默认 50" }
            }
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let query = NodeQuery {
                    matcher: input
                        .get("matcher")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned),
                    keyword: input
                        .get("keyword")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned),
                    limit: input
                        .get("limit")
                        .and_then(|value| value.as_u64())
                        .map(|limit| limit as usize)
                        .unwrap_or(50)
                        .clamp(1, 200),
                };
                let list = ctx
                    .service
                    .query_learning_graph_draft(&draft_id, query)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&list))
            }
        }),
    }
}

fn lg_subgraph(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "lg_subgraph".into(),
        description: "查看某区域的依赖局部图：围绕给定单元展开前置（ancestors）/ 后继（descendants）闭包，返回局部 DAG 的节点与边。编辑某区域前先看清它的依赖结构。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "nodes": { "type": "array", "items": { "type": "string" }, "description": "中心单元名（必须是图中精确存在的名字）" },
                "direction": { "type": "string", "enum": ["ancestors", "descendants", "both"], "description": "ancestors=全部前置闭包；descendants=全部后继闭包；both=双向" },
                "depth": { "type": "integer", "minimum": 1, "description": "游走层数上限（可选；缺省=完整闭包）" }
            },
            "required": ["nodes", "direction"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let nodes: Vec<String> = input
                    .get("nodes")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                if nodes.is_empty() {
                    return Err("lg_subgraph: nodes 必须是至少一个单元名的数组".into());
                }
                let direction = match input
                    .get("direction")
                    .and_then(|value| value.as_str())
                {
                    Some("ancestors") => SubgraphDirection::Ancestors,
                    Some("descendants") => SubgraphDirection::Descendants,
                    Some("both") => SubgraphDirection::Both,
                    _ => {
                        return Err(
                            "lg_subgraph: direction 必须是 ancestors / descendants / both".into()
                        );
                    }
                };
                let depth = input
                    .get("depth")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as usize);
                let view = ctx
                    .service
                    .subgraph_learning_graph_draft(&draft_id, nodes, direction, depth)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&view))
            }
        }),
    }
}

fn lg_audit(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "lg_audit".into(),
        description: "完整确定性审计报告：每条 finding 的级别、类型、证据与修复提示，以及 scope 对照覆盖情况（大块概念未落地）。修复 loop 的主要输入；发布前的最终自查也用它。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                ctx.service
                    .audit_learning_graph_draft(&draft_id)
                    .map_err(|error| error.to_string())
            }
        }),
    }
}

fn lg_scope(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "lg_scope".into(),
        description: "返回 scope 范围参考全文：大块概念清单——生成阶段的严格完备覆盖自查表。构建前先取回它，逐项核对你的计划，确保每个大块概念都不遗漏。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                match ctx
                    .service
                    .scope_reference_learning_graph_draft(&draft_id)
                    .map_err(|error| error.to_string())?
                {
                    Some(reference) => Ok(reference),
                    None => Ok("本草稿没有 scope 参考（范围分析不可用时降级）；以 lg_audit 的覆盖检查为准".into()),
                }
            }
        }),
    }
}

fn lg_patch(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "lg_patch".into(),
        description: "批量应用图操作（add/link/unlink/set_pre/reverse/split/merge/update/delete），一次调用就是一个批次。操作按数组顺序执行，后面的操作可以引用本批前面 add 的名字。返回每个操作的成功/拒绝明细 + 最新审计摘要。\n\n各操作语义：\n- add：新增单元，pre 是直接前置名列表，min 是分钟预算。\n- link：在两个已有单元之间补一条缺失的依赖边 from -> to。\n- unlink：仅移除一条依赖边 from -> to，两个单元都保留——发现关系画错（而非方向画反）时用它；reverse 只适合改方向。\n- set_pre：把 target 的前置集合整体替换为 pre（完整集合语义），一步完成'删错边+接对边'；前置画错或多余时优先用它。\n- reverse：把已有边 from -> to 翻转为 to -> from。\n- split / merge / update / delete：拆分、合并、改名或改预算；delete 会删除单元及其触及的所有边。\n\n调用示例：\n{\"operations\": [\n  {\"op\": \"add\", \"name\": \"用几何概括法临摹静物\", \"pre\": [], \"min\": 20},\n  {\"op\": \"add\", \"name\": \"使用一点透视绘制空间\", \"pre\": [\"用几何概括法临摹静物\"], \"min\": 25},\n  {\"op\": \"unlink\", \"from\": \"使用一点透视绘制空间\", \"to\": \"画布与画笔准备\"},\n  {\"op\": \"link\", \"from\": \"画布与画笔准备\", \"to\": \"用几何概括法临摹静物\"},\n  {\"op\": \"set_pre\", \"target\": \"使用一点透视绘制空间\", \"pre\": [\"用几何概括法临摹静物\", \"画布与画笔准备\"]}\n]}\n\n引用规则：add 的 pre、set_pre 的 pre 和 link/unlink/reverse 的 from/to 必须与图中已有名字（或本批前面 add 的名字）完全一致，不一致的操作会被拒绝并附最近似的名字提示。每批 5-15 个操作，宁多批勿超长。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "operations": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 15,
                    "description": "图操作数组（按顺序执行，可引用本批前面 add 创建的名字）；每个元素是带 op 标签的对象，见下方 items 的 oneOf 定义",
                    "items": {
                        "oneOf": [
                            { "type": "object", "properties": { "op": { "const": "add" }, "name": { "type": "string", "description": "新单元名——动作句，通常 30 分钟内可完成的一个学习会话（极困难最多 60 分钟）" }, "pre": { "type": "array", "items": { "type": "string" }, "description": "直接前置单元名，必须与图中已有名字（或本批前面 add 的名字）完全一致；入门单元可为空" }, "min": { "type": "integer", "minimum": 1, "maximum": 60, "description": "学习分钟预算：常规 5-30（软上限），极困难单课最多 60（硬上限）" } }, "required": ["op", "name"] },
                            { "type": "object", "properties": { "op": { "const": "link" }, "from": { "type": "string", "description": "已存在单元名（前置）" }, "to": { "type": "string", "description": "已存在单元名（后继）" } }, "required": ["op", "from", "to"] },
                            { "type": "object", "properties": { "op": { "const": "unlink" }, "from": { "type": "string", "description": "已存在单元名（当前前置）" }, "to": { "type": "string", "description": "已存在单元名（当前后继）" } }, "required": ["op", "from", "to"] },
                            { "type": "object", "properties": { "op": { "const": "reverse" }, "from": { "type": "string", "description": "已存在单元名（新前置）" }, "to": { "type": "string", "description": "已存在单元名（新后继）" } }, "required": ["op", "from", "to"] },
                            { "type": "object", "properties": { "op": { "const": "set_pre" }, "target": { "type": "string", "description": "要改前置的已存在单元名" }, "pre": { "type": "array", "items": { "type": "string" }, "description": "完整的新直接前置名列表（必须与图中已有名字或本批前面 add 的名字完全一致）；空数组=清空前置使其成为入门单元（是否合规由审计判定）；重复项自动去重" } }, "required": ["op", "target", "pre"] },
                            { "type": "object", "properties": { "op": { "const": "split" }, "target": { "type": "string", "description": "要拆分的已存在单元名" }, "into": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string", "description": "拆分后的新单元名（动作句）" }, "pre": { "type": "array", "items": { "type": "string" }, "description": "可选的直接前置名（已有单元或本批其它拆分）；留空则继承被拆单元的前置" }, "min": { "type": "integer", "minimum": 1, "maximum": 60 } }, "required": ["name"] } } }, "required": ["op", "target", "into"] },
                            { "type": "object", "properties": { "op": { "const": "merge" }, "into": { "type": "string", "description": "保留的已存在单元名" }, "targets": { "type": "array", "items": { "type": "string" }, "description": "要并入的已存在单元名列表" } }, "required": ["op", "into", "targets"] },
                            { "type": "object", "properties": { "op": { "const": "update" }, "target": { "type": "string", "description": "要改的已存在单元名" }, "name": { "type": "string", "description": "新名字（可选）" }, "min": { "type": "integer", "minimum": 1, "maximum": 60, "description": "新分钟预算（可选）" } }, "required": ["op", "target"] },
                            { "type": "object", "properties": { "op": { "const": "delete" }, "target": { "type": "string", "description": "要删除的已存在单元名" } }, "required": ["op", "target"] }
                        ]
                    }
                }
            },
            "required": ["operations"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let operations =
                    input.get("operations").cloned().unwrap_or(serde_json::Value::Null);
                let ops: Vec<GraphOp> = serde_json::from_value(operations)
                    .map_err(|error| format!("lg_patch: operations 必须是图操作数组——{error}"))?;
                if ops.is_empty() {
                    return Err("lg_patch: operations 不能为空".into());
                }
                let report = ctx
                    .service
                    .patch_learning_graph_draft(&draft_id, ops)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&report))
            }
        }),
    }
}

fn lg_finish(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "lg_finish".into(),
        description: "发布草稿为正式学习图记录。确定性审计门禁有最终决定权：存在 danger 级 findings 时发布被阻塞，返回完整阻塞报告（草稿保留，可继续修复）。只有 lg_audit 确认无 danger 时才调用。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                match ctx
                    .service
                    .finish_learning_graph_draft(&ctx.user_id, &draft_id)
                    .await
                {
                    Ok(record) => {
                        *ctx.published_slot
                            .lock()
                            .map_err(|_| "lg_finish: 内部锁故障".to_owned())? =
                            Some(record.clone());
                        Ok(format!(
                            "学习图已发布：id={}，主题={}，{} 个单元 / {} 条边。",
                            record.id,
                            record.topic,
                            record.graph.nodes.len(),
                            record.graph.edges.len()
                        ))
                    }
                    Err(error) => Err(error.to_string()),
                }
            }
        }),
    }
}

// ── The tool loop (one-shot core, parameterized) ───────────────────────────
//
// `run_agent_loop` lives in `loop_core` — shared verbatim by the
// course-outline and lesson-content loops. The fail-closed whitelist
// execution contract is documented there.

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_providers::ProviderError;
    use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
    use nomi_types::message::{ContentBlock, Message, StopReason, TokenUsage};
    use nomifun_learning::LearningCompleter;
    use tokio::sync::mpsc;

    use crate::knowledge_completer::tests::{ListOnlyModelRepo, ListOnlyRepo};
    use crate::loop_core::AGENT_MAX_TOKENS;

    /// Scripted fake provider: each `stream` call pops the next script
    /// entry; every observed request (tool names + messages) is recorded.
    struct ScriptedProvider {
        script: Mutex<Vec<Vec<LlmEvent>>>,
        seen_tool_names: Mutex<Vec<Vec<String>>>,
        seen_messages: Mutex<Vec<Vec<Message>>>,
        seen_thinking: Mutex<Vec<Option<ThinkingConfig>>>,
    }

    impl ScriptedProvider {
        fn new(script: Vec<Vec<LlmEvent>>) -> Arc<Self> {
            Arc::new(Self {
                script: Mutex::new(script),
                seen_tool_names: Mutex::new(Vec::new()),
                seen_messages: Mutex::new(Vec::new()),
                seen_thinking: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn stream(
            &self,
            request: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            self.seen_tool_names
                .lock()
                .unwrap()
                .push(request.tools.iter().map(|tool| tool.name.clone()).collect());
            self.seen_messages.lock().unwrap().push(request.messages.clone());
            self.seen_thinking.lock().unwrap().push(request.thinking.clone());
            let mut script = self.script.lock().unwrap();
            if script.is_empty() {
                return Err(ProviderError::Connection("script exhausted".into()));
            }
            let events = script.remove(0);
            let (tx, rx) = mpsc::channel(events.len().max(1));
            tokio::spawn(async move {
                for event in events {
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
            });
            Ok(rx)
        }
    }

    fn done(stop_reason: StopReason) -> LlmEvent {
        LlmEvent::Done { stop_reason, usage: TokenUsage::default() }
    }

    fn tool_use(name: &str, input: serde_json::Value) -> LlmEvent {
        LlmEvent::ToolUse {
            id: format!("call_{name}"),
            name: name.into(),
            input,
            extra: None,
        }
    }

    fn add_op(name: &str, pre: &[&str]) -> serde_json::Value {
        serde_json::json!({ "op": "add", "name": name, "pre": pre })
    }

    /// Completer whose reply never parses as a scope reference: every draft
    /// starts scope-free, keeping the deterministic audit fully structural.
    struct FakeCompleter;

    #[async_trait::async_trait]
    impl LearningCompleter for FakeCompleter {
        async fn complete(
            &self,
            _model_override: Option<(&str, &str)>,
            _system: &str,
            _user: &str,
            _max_tokens: u32,
        ) -> Result<String, AppError> {
            Ok("not a scope json".into())
        }
    }

    #[derive(Default)]
    struct NoopBroadcaster;

    impl nomifun_realtime::UserEventSink for NoopBroadcaster {
        fn send_to_user(
            &self,
            _user_id: &str,
            _event: nomifun_api_types::WebSocketMessage<serde_json::Value>,
        ) {
        }
    }

    /// A service wired with the fake completer and a scratch knowledge dir;
    /// the temp dir stays alive for the test's duration.
    async fn test_service() -> (Arc<LearningService>, tempfile::TempDir) {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let knowledge_service = Arc::new(nomifun_knowledge::KnowledgeService::new(
            Arc::new(nomifun_db::SqliteKnowledgeRepository::new(
                database.pool().clone(),
            )),
            dir.path(),
            nomifun_knowledge::KnowledgeEventEmitter::new(
                Arc::new(NoopBroadcaster),
                Arc::from(owner_id),
            ),
        ));
        let service = Arc::new(LearningService::new(database.pool().clone()));
        service.set_generation_dependencies(knowledge_service, Arc::new(FakeCompleter));
        (service, dir)
    }

    fn engine(service: Arc<LearningService>) -> LiveLearningGraphAgentEngine {
        LiveLearningGraphAgentEngine {
            service,
            round_logs: Arc::new(Mutex::new(HashMap::new())),
            deps: OneShotDeps {
                provider_repo: Arc::new(ListOnlyRepo(Vec::new())),
                provider_model_repo: Arc::new(ListOnlyModelRepo(Vec::new())),
                encryption_key: [0u8; 32],
                workspace: std::env::temp_dir(),
            },
        }
    }

    fn context(
        service: Arc<LearningService>,
    ) -> (
        Arc<LoopContext>,
        Arc<Mutex<Option<String>>>,
        Arc<Mutex<Option<LearningGraphRecord>>>,
    ) {
        let draft_slot = Arc::new(Mutex::new(None));
        let published_slot = Arc::new(Mutex::new(None));
        let ctx = Arc::new(LoopContext {
            service,
            user_id: UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
            model_override: None,
            draft_slot: Arc::clone(&draft_slot),
            published_slot: Arc::clone(&published_slot),
            round_log: Mutex::new(Vec::new()),
        });
        (ctx, draft_slot, published_slot)
    }

    /// 与 [`context`] 相同，但预创建草稿并预填 slot——注入式架构下生产
    /// 路径（`run_generation`）在循环开始前就持有草稿；脚本直接从构建动
    /// 作开始的测试用这个入口。
    async fn seeded_context(
        service: Arc<LearningService>,
    ) -> (
        Arc<LoopContext>,
        Arc<Mutex<Option<String>>>,
        Arc<Mutex<Option<LearningGraphRecord>>>,
    ) {
        let view = service
            .create_learning_graph_draft("数学基础", None, None)
            .await
            .unwrap();
        let draft_slot = Arc::new(Mutex::new(Some(view.draft_id)));
        let published_slot = Arc::new(Mutex::new(None));
        let ctx = Arc::new(LoopContext {
            service,
            user_id: UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
            model_override: None,
            draft_slot: Arc::clone(&draft_slot),
            published_slot: Arc::clone(&published_slot),
            round_log: Mutex::new(Vec::new()),
        });
        (ctx, draft_slot, published_slot)
    }

    /// 安全不变量：发给模型的工具注册面恰等于构造的工具集——每一轮都如此，
    /// 且生成 loop 携带全部 7 个工具（草稿注入后不再有“启动”工具）。
    #[tokio::test]
    async fn generation_loop_exposes_exactly_the_whitelist() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service);
        let tools = learning_graph_tools(Arc::clone(&ctx));
        let names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "lg_inspect", "lg_query", "lg_subgraph", "lg_audit", "lg_scope",
                "lg_patch", "lg_finish"
            ]
        );

        // Round 1: the model asks lg_inspect before any draft exists — the
        // handler must answer with a guidance error, never panic.
        let provider = ScriptedProvider::new(vec![
            vec![
                tool_use("lg_inspect", serde_json::json!({})),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("understood".into()), done(StopReason::EndTurn)],
        ]);
        run_agent_loop(
            provider.clone(),
            "test-model",
            GENERATE_AGENT_SYSTEM,
            "数学基础",
            &tools,
            GENERATE_MAX_ROUNDS,
            ROUND_TOKEN_BUDGET,
            ThinkingConfig::Disabled,
            "generate",
            Some(ctx.as_ref()),
        )
        .await
        .unwrap();

        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 2, "two model rounds");
        for round in seen.iter() {
            assert_eq!(round, &names, "tool registry must be exactly the whitelist on every round");
        }
        let thinking = provider.seen_thinking.lock().unwrap();
        assert!(
            thinking.iter().all(|t| matches!(t, Some(ThinkingConfig::Disabled))),
            "every round must explicitly disable thinking: {thinking:?}"
        );
        let rounds = provider.seen_messages.lock().unwrap();
        let followup = &rounds[1];
        let last = followup.last().unwrap();
        assert!(matches!(
            &last.content[0],
            ContentBlock::ToolResult { is_error: true, content, .. }
                if content.contains("没有活动的草稿")
        ));
    }

    /// 轮次上限：脚本无限工具调用时，loop 在 max_rounds 处终止并报错。
    #[tokio::test]
    async fn agent_loop_round_cap_returns_error() {
        let tool = OneShotTool {
            name: "ping".into(),
            description: "test".into(),
            input_schema: serde_json::json!({}),
            handler: one_shot_handler(|_| async { Ok("pong".to_owned()) }),
        };
        let provider = ScriptedProvider::new(vec![
            vec![tool_use("ping", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![tool_use("ping", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![tool_use("ping", serde_json::json!({})), done(StopReason::ToolUse)],
        ]);
        let error = run_agent_loop(
            provider,
            "test-model",
            "system",
            "user",
            std::slice::from_ref(&tool),
            2,
            AGENT_MAX_TOKENS,
            ThinkingConfig::Disabled,
            "test",
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(&error, AppError::BadGateway(message) if message.contains("exceeded 2 tool rounds")));
    }

    /// 空图必须被审计门禁拦截：生成 loop 里模型过早 lg_finish（一个单元
    /// 都没建）→ 发布被拒（empty_graph danger）→ 修复 loop 补建完整网络
    /// → 第二轮门禁通过并发布。绝不能以空图收尾（前端会渲染成空白图）。
    #[tokio::test]
    async fn empty_graph_is_blocked_until_repaired_and_published() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = seeded_context(Arc::clone(&service)).await;

        // Repair batch: n2/n3 branch off n1; n4/n5 converge two threads
        // each; n6 converges n4+n5; n7..n30 depend on n6 — 30 nodes, 3
        // multi-parent nodes (>= 10%), one component, depth 4.
        let mut operations = vec![
            add_op("n1", &[]),
            add_op("n2", &["n1"]),
            add_op("n3", &["n1"]),
            add_op("n4", &["n1", "n2", "n3"]),
            add_op("n5", &["n1", "n2"]),
            add_op("n6", &["n4", "n5"]),
        ];
        for index in 7..=30 {
            operations.push(add_op(&format!("n{index}"), &["n6"]));
        }

        let provider = ScriptedProvider::new(vec![
            // ── generation loop: a premature lg_finish on the injected draft ──
            vec![tool_use("lg_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                LlmEvent::TextDelta("发布被拒，需要先构建单元".into()),
                done(StopReason::EndTurn),
            ],
            // ── repair loop 1: build the whole network in one patch ──
            vec![
                tool_use(
                    "lg_patch",
                    serde_json::json!({ "operations": operations }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("已构建完整网络".into()), done(StopReason::EndTurn)],
        ]);
        let record = engine(Arc::clone(&service))
            .run_loops(
                provider.clone(),
                "test-model",
                "数学基础",
                ctx,
                ROUND_TOKEN_BUDGET,
                ThinkingConfig::Disabled,
            )
            .await
            .unwrap();
        assert_eq!(record.topic, "数学基础");
        assert_eq!(record.graph.nodes.len(), 30, "repaired graph publishes");
        // The published record id is the graph course's id.
        assert!(nomifun_common::LearningCourseId::parse(&record.id).is_ok());
        // The premature lg_finish was rejected with the empty_graph finding.
        let rounds = provider.seen_messages.lock().unwrap();
        let rejected = rounds.iter().flat_map(|round| round.iter()).any(|message| {
            matches!(
                &message.content[0],
                ContentBlock::ToolResult { is_error: true, content, .. }
                    if content.contains("empty_graph")
            )
        });
        assert!(rejected, "an empty graph must be rejected by the audit gate");
        // The published graph lives in the database (the SQLite publish
        // replaced the legacy JSON directory) — exactly one learning-graph
        // course with the full repaired node set.
        let published = service.list_learning_graphs().await.unwrap();
        assert_eq!(published.len(), 1, "exactly one published record");
        assert_eq!(published[0].node_count, 30, "the repaired graph publishes");
    }

    /// 预算恰好在发布轮耗尽：lg_finish 在最后一轮成功发布后循环才撞上
    /// 轮次墙——发布必须优先于循环错误，已入库的成果不能被吞掉。
    #[tokio::test]
    async fn publish_on_final_round_survives_round_budget() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = seeded_context(Arc::clone(&service)).await;

        // 无 scope（FakeCompleter）下 coverage 门是固定 min_units——用与
        // empty_graph 测试同款的 30 节点收敛网，确保 lg_finish 过门禁。
        let mut operations = vec![
            add_op("n1", &[]),
            add_op("n2", &["n1"]),
            add_op("n3", &["n1"]),
            add_op("n4", &["n1", "n2", "n3"]),
            add_op("n5", &["n1", "n2"]),
            add_op("n6", &["n4", "n5"]),
        ];
        for index in 7..=30 {
            operations.push(add_op(&format!("n{index}"), &["n6"]));
        }
        let mut script: Vec<Vec<LlmEvent>> = (0..GENERATE_MAX_ROUNDS - 2)
            .map(|_| {
                vec![
                    tool_use("lg_inspect", serde_json::json!({})),
                    done(StopReason::ToolUse),
                ]
            })
            .collect();
        script.push(vec![
            tool_use(
                "lg_patch",
                serde_json::json!({ "operations": operations }),
            ),
            done(StopReason::ToolUse),
        ]);
        script.push(vec![
            tool_use("lg_finish", serde_json::json!({})),
            done(StopReason::ToolUse),
        ]);

        let record = engine(Arc::clone(&service))
            .run_loops(
                ScriptedProvider::new(script),
                "test-model",
                "数学基础",
                ctx,
                ROUND_TOKEN_BUDGET,
                ThinkingConfig::Disabled,
            )
            .await
            .expect("publish on the final round must win over the round-budget error");
        assert_eq!(record.topic, "数学基础");
        assert_eq!(record.graph.nodes.len(), 30);
    }

    /// 修复 loop：生成 loop 只铺了 2 个单元（coverage danger）——发布被
    /// 门禁阻塞；修复 loop 补齐到 30 个且收敛结构（3 个多父节点）后通过。
    /// 修复 loop 的工具面必须不含 lg_start。
    #[tokio::test]
    async fn repair_loop_fixes_danger_findings_before_publish() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = seeded_context(Arc::clone(&service)).await;

        // Repair batch: n3 branches off n1; n4/n5 converge two threads each;
        // n6 converges n4+n5; n7..n30 depend on n6 — 30 nodes, 3 multi-parent
        // nodes (>= 10%), one component, depth 4.
        let mut operations = vec![
            add_op("n3", &["n1"]),
            add_op("n4", &["n1", "n2", "n3"]),
            add_op("n5", &["n1", "n2"]),
            add_op("n6", &["n4", "n5"]),
        ];
        for index in 7..=30 {
            operations.push(add_op(&format!("n{index}"), &["n6"]));
        }

        let provider = ScriptedProvider::new(vec![
            // ── generation loop: build only 2 units on the injected draft ──
            vec![
                tool_use(
                    "lg_patch",
                    serde_json::json!({
                        "operations": [
                            add_op("n1", &[]),
                            add_op("n2", &["n1"])
                        ]
                    }),
                ),
                done(StopReason::ToolUse),
            ],
            // The model declares the generation done WITHOUT lg_finish; the
            // gate blocks the publish (2 < 30 units) and the repair loop
            // starts.
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // ── repair loop ──
            vec![
                tool_use("lg_patch", serde_json::json!({ "operations": operations })),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("fixed".into()), done(StopReason::EndTurn)],
        ]);

        let record = engine(Arc::clone(&service))
            .run_loops(
                provider.clone(),
                "test-model",
                "数学基础",
                ctx,
                ROUND_TOKEN_BUDGET,
                ThinkingConfig::Disabled,
            )
            .await
            .unwrap();
        assert_eq!(record.graph.nodes.len(), 30);
        assert_eq!(record.graph.edges.len(), 33, "2 + 28 adds, 3 multi-parent joins");

        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 4, "2 generation rounds + 2 repair rounds");
        // The SQLite publish replaced the legacy JSON directory.
        let published = service.list_learning_graphs().await.unwrap();
        assert_eq!(published.len(), 1, "one published record");
        assert_eq!(published[0].edge_count, 33, "2 + 28 adds, 3 multi-parent joins");
    }

    /// 模型从未调用 lg_start 就宣告完成：无草稿可发布，明确报错。
    #[tokio::test]
    async fn finishing_without_a_draft_fails() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service.clone());
        let provider = ScriptedProvider::new(vec![vec![
            LlmEvent::TextDelta("nothing to do".into()),
            done(StopReason::EndTurn),
        ]]);
        let error = engine(service)
            .run_loops(
                provider,
                "test-model",
                "数学基础",
                ctx,
                ROUND_TOKEN_BUDGET,
                ThinkingConfig::Disabled,
            )
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::Internal(message) if message.contains("without creating a draft")));
    }

    /// 修复预算耗尽：每次修复 loop 后审计仍有 danger，最终返回阻塞报告
    /// 而非静默失败。
    #[tokio::test]
    async fn repair_budget_exhaustion_reports_blocking_findings() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = seeded_context(Arc::clone(&service)).await;
        let provider = ScriptedProvider::new(vec![
            // generation loop: one unit only
            vec![
                tool_use("lg_patch", serde_json::json!({ "operations": [add_op("n1", &[])] })),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // repair loop 1: refuses to touch anything, then stops
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            // repair loop 2
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            // repair loop 3
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
        ]);
        let error = engine(Arc::clone(&service))
            .run_loops(
                provider.clone(),
                "test-model",
                "数学基础",
                ctx,
                ROUND_TOKEN_BUDGET,
                ThinkingConfig::Disabled,
            )
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::UnprocessableEntity(message) if message.contains("exhausted 3 repair loops")));
        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 5, "2 generation rounds + 3 repair rounds");
    }

    /// 同轮重试：流中报出 malformed tool-call arguments 的 Error 事件时，
    /// 同一轮原样重发一次并继续——失败轮零副作用（fail-closed 解析），
    /// 消息历史不变。
    #[tokio::test]
    async fn stream_error_retries_the_same_round_and_recovers() {
        let tool = OneShotTool {
            name: "ping".into(),
            description: "test".into(),
            input_schema: serde_json::json!({}),
            handler: one_shot_handler(|_| async { Ok("pong".to_owned()) }),
        };
        let provider = ScriptedProvider::new(vec![
            vec![LlmEvent::Error(
                "OpenAI-compatible provider returned malformed JSON arguments for tool `lg_patch` (call `call_x`): expected `,` or `}` at line 1 column 2183".into(),
            )],
            vec![LlmEvent::TextDelta("ok".into()), done(StopReason::EndTurn)],
        ]);
        let text = run_agent_loop(
            provider.clone(),
            "test-model",
            "system",
            "user",
            std::slice::from_ref(&tool),
            3,
            AGENT_MAX_TOKENS,
            ThinkingConfig::Disabled,
            "test",
            None,
        )
        .await
        .unwrap();
        assert_eq!(text, "ok");
        let seen = provider.seen_messages.lock().unwrap();
        assert_eq!(seen.len(), 2, "the failed round is re-sent once");
        assert_eq!(
            format!("{:?}", seen[0]),
            format!("{:?}", seen[1]),
            "the retry re-sends the identical request"
        );
    }

    /// 同轮重试耗尽后损坏降级为轮内反馈：第 1 轮损坏 → 原样重试；重试再
    /// 损坏 → 注入系统反馈推进到下一轮，模型重新提交（不再判死会话）。
    #[tokio::test]
    async fn corrupted_round_degrades_to_feedback_and_recovers() {
        let tool = OneShotTool {
            name: "ping".into(),
            description: "test".into(),
            input_schema: serde_json::json!({}),
            handler: one_shot_handler(|_| async { Ok("pong".to_owned()) }),
        };
        let malformed = || {
            LlmEvent::Error(
                "OpenAI-compatible provider returned malformed JSON arguments for tool `ping` (call `call_x`): expected `,` or `}` at line 1 column 2183".into(),
            )
        };
        let provider = ScriptedProvider::new(vec![
            vec![malformed()], // 第 1 轮：同轮重试
            vec![malformed()], // 重试再损坏：转入反馈轮
            vec![LlmEvent::TextDelta("ok".into()), done(StopReason::EndTurn)],
        ]);
        let text = run_agent_loop(
            provider.clone(),
            "test-model",
            "system",
            "user",
            std::slice::from_ref(&tool),
            3,
            AGENT_MAX_TOKENS,
            ThinkingConfig::Disabled,
            "test",
            None,
        )
        .await
        .unwrap();
        assert_eq!(text, "ok");
        let seen = provider.seen_messages.lock().unwrap();
        assert_eq!(
            seen.len(),
            3,
            "initial + same-round retry + feedback round"
        );
        // 反馈轮的请求历史末尾携带系统反馈（损坏轮本身零副作用，不污染历史）。
        let last_round = &seen[2];
        let feedback = last_round.last().unwrap();
        assert!(
            matches!(
                &feedback.content[0],
                ContentBlock::Text { text, .. } if text.contains("参数格式有误")
            ),
            "{feedback:?}"
        );
    }

    /// 持续损坏（网关系统性丢块）：同轮重试与反馈注入的上限逐层耗尽后，
    /// 仍以原错误失败——降级不是无限烧钱的免死金牌。
    #[tokio::test]
    async fn sustained_corruption_eventually_fails() {
        let tool = OneShotTool {
            name: "ping".into(),
            description: "test".into(),
            input_schema: serde_json::json!({}),
            handler: one_shot_handler(|_| async { Ok("pong".to_owned()) }),
        };
        let malformed = || {
            LlmEvent::Error(
                "OpenAI-compatible provider returned malformed JSON arguments for tool `ping` (call `call_x`): expected `,` or `}` at line 1 column 2183".into(),
            )
        };
        let provider = ScriptedProvider::new(vec![
            vec![malformed()],
            vec![malformed()],
            vec![malformed()],
            vec![malformed()],
            vec![malformed()],
            vec![malformed()],
            vec![malformed()],
            vec![malformed()],
            vec![malformed()],
            vec![malformed()],
        ]);
        let error = run_agent_loop(
            provider,
            "test-model",
            "system",
            "user",
            std::slice::from_ref(&tool),
            30,
            AGENT_MAX_TOKENS,
            ThinkingConfig::Disabled,
            "test",
            None,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&error, AppError::BadGateway(message) if message.contains("malformed JSON arguments")),
            "{error}"
        );
    }

    /// 失败诊断：生成循环中途流错误（重试后仍失败）→ 错误消息附上存活
    /// 草稿的 id 与审计状态——前端续建与人工排查都依赖它。
    #[tokio::test]
    async fn generation_failure_attaches_surviving_draft_context() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = seeded_context(Arc::clone(&service)).await;
        let malformed = || {
            LlmEvent::Error(
                "OpenAI-compatible provider returned malformed JSON arguments for tool `lg_patch` (call `call_x`): expected `,` or `}` at line 1 column 2183".into(),
            )
        };
        let provider = ScriptedProvider::new(vec![
            // round 1 + 同轮重试：连续损坏 → 每轮限额触发，错误传播
            vec![malformed()],
            vec![malformed()],
        ]);
        let error = engine(Arc::clone(&service))
            .run_loops(
                provider,
                "test-model",
                "数学基础",
                ctx,
                ROUND_TOKEN_BUDGET,
                ThinkingConfig::Disabled,
            )
            .await
            .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("仍在"), "must mention the surviving draft: {text}");
        assert!(text.contains("可续建"), "{text}");
        assert!(
            text.contains("empty_graph"),
            "the draft's live audit state rides along: {text}"
        );
    }

    /// 取消不保留草稿（2026-09 决策）：取消旗标置位后循环失败 → 终态消息
    /// 只有一句中性提示，草稿与轮次日志一并丢弃，重试即全新生成。
    #[tokio::test]
    async fn cancellation_discards_the_draft_instead_of_keeping_it() {
        let (service, _dir) = test_service().await;
        let (ctx, draft_slot, _published) = seeded_context(Arc::clone(&service)).await;
        let draft_id = draft_slot
            .lock()
            .unwrap()
            .clone()
            .expect("seeded draft registers its id");
        service.cancel_generation();
        let provider = ScriptedProvider::new(vec![vec![LlmEvent::Error("provider exploded".into())]]);
        let error = engine(Arc::clone(&service))
            .run_loops(
                provider,
                "test-model",
                "数学基础",
                ctx,
                ROUND_TOKEN_BUDGET,
                ThinkingConfig::Disabled,
            )
            .await
            .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("生成已被用户取消"), "{text}");
        assert!(!text.contains("可续建"), "cancel keeps no draft note: {text}");
        assert!(
            service.inspect_learning_graph_draft(&draft_id).is_err(),
            "the draft must be discarded on cancellation"
        );
    }

    /// 续建：预置存活草稿（已建 2 个单元）后重入生成循环——模型直接在现
    /// 有网络上补齐，门禁通过发布，结果与全新生成同构。
    #[tokio::test]
    async fn resume_continues_from_surviving_draft() {
        let (service, _dir) = test_service().await;

        // 预置中断点：草稿里已有 n1、n2 两个单元。
        let view = service
            .create_learning_graph_draft("数学基础", None, None)
            .await
            .unwrap();
        service
            .patch_learning_graph_draft(
                &view.draft_id,
                vec![
                    GraphOp::Add { name: "n1".into(), pre: vec![], min: Some(10) },
                    GraphOp::Add { name: "n2".into(), pre: vec!["n1".into()], min: Some(10) },
                ],
            )
            .unwrap();

        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&service),
            user_id: UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
            model_override: None,
            draft_slot: Arc::new(Mutex::new(Some(view.draft_id.clone()))),
            published_slot: Arc::new(Mutex::new(None)),
            round_log: Mutex::new(Vec::new()),
        });

        // 补齐到 30 个单元的收敛结构（与修复测试同构）。
        let mut operations = vec![
            add_op("n3", &["n1"]),
            add_op("n4", &["n1", "n2", "n3"]),
            add_op("n5", &["n1", "n2"]),
            add_op("n6", &["n4", "n5"]),
        ];
        for index in 7..=30 {
            operations.push(add_op(&format!("n{index}"), &["n6"]));
        }

        let provider = ScriptedProvider::new(vec![
            // 生成轮：直接在预置草稿上补齐剩余网络
            vec![
                tool_use("lg_patch", serde_json::json!({ "operations": operations })),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("已补齐".into()), done(StopReason::EndTurn)],
        ]);

        let record = engine(Arc::clone(&service))
            .run_loops(
                provider.clone(),
                "test-model",
                "数学基础",
                ctx,
                ROUND_TOKEN_BUDGET,
                ThinkingConfig::Disabled,
            )
            .await
            .unwrap();
        assert_eq!(record.graph.nodes.len(), 30, "the resumed draft publishes");
        let published = service.list_learning_graphs().await.unwrap();
        assert_eq!(published.len(), 1, "one published record");
        assert_eq!(published[0].node_count, 30);
    }

    /// 服务层续建入口：无存活草稿（未生成过/已过 TTL/重启）时返回
    /// NotFound——前端据此回退全量重生成。
    #[tokio::test]
    async fn resume_without_a_live_draft_reports_not_found() {
        let (service, _dir) = test_service().await;
        service.set_learning_graph_engine(Arc::new(engine(Arc::clone(&service))));
        let user = UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap();
        let error = service
            .resume_learning_graph_course(&user, None, None)
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::NotFound(_)), "{error}");
    }

    fn test_request() -> LlmRequest {
        LlmRequest {
            model: "test-model".into(),
            system: String::new(),
            messages: vec![],
            tools: vec![],
            max_tokens: Some(1024),
            thinking: Some(ThinkingConfig::Disabled),
            reasoning_effort: None,
            temperature: None,
            retain_provider_round: false,
        }
    }

    /// 轮次日志：agent_round 事件的意图文本与工具摘要被记入 `round_log`，
    /// 续建时注入开场恢复认知。
    #[tokio::test]
    async fn round_log_records_each_round_intent() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service);
        ctx.log(
            "agent_round",
            serde_json::json!({
                "loop": "generate",
                "round": 12,
                "text": "91个节点，覆盖10/12大块。现在需要继续构建解析几何与微积分。",
                "tool_calls": [
                    { "name": "lg_patch", "is_error": false },
                    { "name": "lg_inspect", "is_error": false },
                ],
            }),
        );
        ctx.log(
            "agent_round",
            serde_json::json!({
                "loop": "repair",
                "round": 1,
                "text": "",
                "tool_calls": [{ "name": "lg_patch", "is_error": true }],
            }),
        );
        let log = ctx.round_log.lock().unwrap();
        assert_eq!(log.len(), 2);
        assert!(
            log[0].contains("第12轮(构建)")
                && log[0].contains("lg_patch✓")
                && log[0].contains("lg_inspect✓"),
            "{log:?}"
        );
        assert!(
            log[0].contains("覆盖10/12大块"),
            "the plan text must ride along: {:?}",
            log[0]
        );
        assert!(
            log[1].contains("第1轮(修复)") && log[1].contains("lg_patch✗"),
            "{log:?}"
        );
    }

    /// 续建开场：轮次日志注入「上次会话构建日志」段，并引导先通读全图。
    #[test]
    fn compose_opening_text_carries_previous_rounds() {
        let text = compose_opening_user_text(
            "从零基础到大学数学",
            "{}",
            None,
            &["第12轮(构建): lg_patch ✓ 补齐立体几何".into()],
        );
        assert!(text.contains("上次会话构建日志（1 轮后中断"), "{text}");
        assert!(text.contains("第12轮(构建)"), "{text}");
        assert!(text.contains("lg_inspect(full=true)"), "{text}");
        // 新会话（无日志）不出现该段。
        let fresh = compose_opening_user_text("t", "{}", None, &[]);
        assert!(!fresh.contains("上次会话构建日志"), "{fresh}");
    }

    /// lg_inspect(full=true)：概览后附全部单元的紧凑清单（续建恢复认知与
    /// 上交前自查共用的一次性全图读取通道）。
    #[tokio::test]
    async fn inspect_full_dumps_the_whole_graph() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = seeded_context(Arc::clone(&service)).await;
        // 预置 a -> b 两个单元，验证 dump 的「名称 [分钟] <- 前置」行格式。
        let draft_id = ctx.draft_slot.lock().unwrap().clone().unwrap();
        service
            .patch_learning_graph_draft(
                &draft_id,
                vec![
                    GraphOp::Add { name: "a".into(), pre: vec![], min: Some(10) },
                    GraphOp::Add { name: "b".into(), pre: vec!["a".into()], min: Some(15) },
                ],
            )
            .unwrap();
        let provider = ScriptedProvider::new(vec![
            vec![
                tool_use("lg_inspect", serde_json::json!({ "full": true })),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("ok".into()), done(StopReason::EndTurn)],
        ]);
        run_agent_loop(
            provider.clone(),
            "test-model",
            GENERATE_AGENT_SYSTEM,
            "数学基础",
            &learning_graph_tools(Arc::clone(&ctx)),
            GENERATE_MAX_ROUNDS,
            ROUND_TOKEN_BUDGET,
            ThinkingConfig::Disabled,
            "test",
            Some(ctx.as_ref()),
        )
        .await
        .unwrap();
        let rounds = provider.seen_messages.lock().unwrap();
        let dumped = rounds.iter().flat_map(|round| round.iter()).any(|message| {
            matches!(
                &message.content[0],
                ContentBlock::ToolResult { content, .. }
                    if content.contains("全图清单")
                        && content.contains("a [10]")
                        && content.contains("b [15] <- a")
            )
        });
        assert!(dumped, "full inspect must dump units with minutes and pres");
    }

    /// 取消包装器：旗标置位时请求在 stream 边界直接拒绝——没有任何请求
    /// 到达底层 provider；`Api(499)` 不可重试，循环错误分支会原样传播。
    #[tokio::test]
    async fn cancelled_provider_rejects_before_the_request() {
        let inner = ScriptedProvider::new(vec![vec![
            LlmEvent::TextDelta("x".into()),
            done(StopReason::EndTurn),
        ]]);
        let provider = CancellableProvider {
            inner: inner.clone(),
            cancel: Arc::new(AtomicBool::new(true)),
        };
        let error = provider.stream(&test_request()).await.unwrap_err();
        assert!(
            matches!(&error, ProviderError::Api { status: 499, message } if message.contains("取消")),
            "{error}"
        );
        assert_eq!(
            inner.seen_messages.lock().unwrap().len(),
            0,
            "no request may reach the inner provider after cancellation"
        );
    }

    /// 取消包装器：流中置位 → 转发器注入 Error(取消) 并停止转发；置位前
    /// 已透传的事件数量不定（竞态），但取消事件必须出现且流随之终止。
    #[tokio::test]
    async fn cancelled_provider_interrupts_mid_stream() {
        let inner = ScriptedProvider::new(vec![vec![
            LlmEvent::TextDelta("partial".into()),
            LlmEvent::TextDelta("more".into()),
            done(StopReason::EndTurn),
        ]]);
        let cancel = Arc::new(AtomicBool::new(false));
        let provider = CancellableProvider {
            inner,
            cancel: Arc::clone(&cancel),
        };
        let mut rx = provider.stream(&test_request()).await.unwrap();
        cancel.store(true, Ordering::Relaxed);
        let mut saw_cancel = false;
        while let Some(event) = rx.recv().await {
            match event {
                LlmEvent::Error(message) => {
                    assert!(message.contains("取消"), "{message}");
                    saw_cancel = true;
                    break;
                }
                LlmEvent::TextDelta(_) => {}
                _ => {}
            }
        }
        assert!(saw_cancel, "the forwarder must inject the cancellation error");
    }
}
