//! 统一的学习生成循环外壳（ADR-0004）：「生成循环 → finish 门 → 审计驱动
//! 的修复循环」这一曾逐字复制三份的骨架，收敛为一个按每流插头参数化的
//! deep module。
//!
//! 学习图（[`crate::learning_graph_loop`]）/ 课程大纲
//! （[`crate::course_outline_loop`]）/ 课时内容
//! （[`crate::lesson_content_loop`]）三条流各自实现 [`FlowCycle`]，只提供
//! 真正属于流的东西：系统提示词、工具白名单、finish 门、发布帧、以及
//! [`LoopBudgets`] / [`WireConfig`] 两张显式差异表。外壳负责其余一切：
//! 轮次预算、修复循环、空闲 nudge、发布优先级、错误诊断接线与 WS 事件
//! 翻译。
//!
//! **差异即配置**（ADR-0004 的反损失保证）：三流之间的全部已知行为差异——
//! 轮次/时长/token 预算、kind 帧标记、round_feedback 翻译、轮次日志、
//! start 阶段帧、发布优先于报错、修复轮错误诊断——都落在显式声明里。想把
//! 某个差异统一掉，必须修改对应声明并说明理由；不存在无声归并的通道。
//!
//! 内层单圈机制（重试、退避、损坏反馈降级）仍在 [`crate::loop_core`]。

use std::sync::{Arc, Mutex};

use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomifun_common::AppError;
use nomifun_learning::LearningService;
use tokio::sync::mpsc;

use crate::loop_core::{
    GENERATE_REASONING_EFFORT, LoopEventSink, REPAIR_LOOP_LIMIT, REPAIR_MAX_ROUNDS,
    REPAIR_REASONING_EFFORT, log_text, run_agent_loop,
};
use crate::one_shot::OneShotTool;

/// 取消提示语：以 `LlmEvent::Error` / `ProviderError::Api` 注入。循环的
/// 错误分支不会把它当作可同轮重试的错误（只匹配 malformed JSON），整个
/// 生成以"已取消"失败收场。取消不保留草稿（重试即全新生成）；真实失
/// 败仍保留草稿供续建。学习图与课程大纲两个生成循环共用（课时内容循环
/// 未接入取消——显式差异，见 ADR-0004）。
pub(crate) const CANCEL_MESSAGE: &str = "生成已被用户取消";

/// 生成循环共用的 provider 包装：每次 LLM 请求开始前与流转发途中轮询
/// 取消旗标（旗标挂在 [`LearningService`] 的生成注册上，取消端点置位）。
/// 取消在流边界即刻生效——请求前直接拒绝；流中把取消作为 Error 事件注入
/// 并停止转发（下游 receiver 被 drop 后上游发送失败，HTTP 流随之终止）。
pub(crate) struct CancellableProvider {
    pub(crate) inner: Arc<dyn LlmProvider>,
    pub(crate) cancel: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait::async_trait]
impl LlmProvider for CancellableProvider {
    async fn stream(
        &self,
        request: &LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
        use std::sync::atomic::Ordering;
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

/// 每流显式预算表（ADR-0004）：轮次、token、时长都在各自流里逐字声明，
/// 不再共享继承——学习图 100/32768/1800 与大纲、课时的 50/8192/600 之间
/// 的差异读一张表即见，`loop_core` 的共享常量只作为大纲/课时的取值来源。
#[derive(Clone, Copy)]
pub(crate) struct LoopBudgets {
    /// 生成循环的轮次上限。
    pub generate_max_rounds: usize,
    /// 每轮 token 预算（学习图在解析时还会收敛到模型 output 上限）。
    pub round_tokens: u32,
    /// 整条管线（生成 + 修复 + 审计门禁）的总时长预算。
    pub timeout_secs: u64,
}

/// 线上翻译差异表（ADR-0004）：同一份 loop-core 事件流，三条流在
/// WebSocket 上的形状各不相同——哪些差异存在、哪些流携带哪些字段，以此
/// 表为准。修改任何一项都是显式的行为变更，须有测试与理由。
#[derive(Clone, Copy)]
pub(crate) struct WireConfig {
    /// 每个上线帧的 `kind` 标记（学习图 `"learning_graph"`；大纲/课时无）。
    pub kind_tag: Option<&'static str>,
    /// `agent_round` 轮次日志行的生成轮标签（学习图「构建」/ 课时「生成」）；
    /// `None` = 不记轮次日志（大纲）。
    pub round_log_gen_label: Option<&'static str>,
    /// 是否把 loop-core 的 `round_feedback`（损坏降级）翻译上线（仅学习图）。
    pub translate_round_feedback: bool,
    /// `generate_loop_start` / `repair_loop_start` 帧的 `phase` 字段——
    /// 没有 `phase` 的帧不会上线（翻译器只放行带 phase 的事件），因此该
    /// 字段同时控制「是否上线」与「phase 取值」（仅学习图声明）。
    pub generate_start_phase: Option<&'static str>,
    pub repair_start_phase: Option<&'static str>,
    /// 事件流选择：`false` → `learning.course-generation`，`true` →
    /// `learning.lesson-generation`（仅课时内容）。
    pub lesson_stream: bool,
}

/// 轮次日志 + WS 帧翻译：每流 `LoopContext` 内嵌一份，替代原先三份
/// `emit_progress` 复制体。翻译行为由 [`WireConfig`] 逐项声明。
pub(crate) struct LoopChannel {
    wire: WireConfig,
    generate_max_rounds: usize,
    /// 轮次日志：每轮的意图文本与工具摘要。对话历史无法跨会话保留，这些
    /// 计划轨迹在续建/续跑时注入开场消息，恢复模型对「做到哪了、接下来干
    /// 什么」的认知。`round_log_gen_label = None` 的流永远为空。
    round_log: Mutex<Vec<String>>,
}

impl LoopChannel {
    pub(crate) fn new(wire: WireConfig, generate_max_rounds: usize) -> Self {
        Self { wire, generate_max_rounds, round_log: Mutex::new(Vec::new()) }
    }

    /// 轮次日志快照（失败/超时时归档到草稿名下）。
    pub(crate) fn round_log_snapshot(&self) -> Vec<String> {
        self.round_log.lock().ok().map(|log| log.clone()).unwrap_or_default()
    }

    /// 把 loop-core 事件翻译为线上帧。`agent_round` 重排为 round 帧；
    /// 带 `phase` 的事件按 [`WireConfig`] 决定是否加 kind 后放行；其余
    /// loop 内部事件不上线。best-effort，绝不 fail 调用方。
    pub(crate) fn emit(&self, service: &LearningService, event: &str, fields: &serde_json::Value) {
        let payload = match event {
            "agent_round" => {
                let repair =
                    fields.get("loop").and_then(serde_json::Value::as_str) != Some("generate");
                if let Some(gen_label) = self.wire.round_log_gen_label {
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
                        if repair { "修复" } else { gen_label },
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
                }
                let mut payload = serde_json::json!({
                    "phase": "round",
                    "loop": fields.get("loop"),
                    "round": fields.get("round"),
                    "max_rounds": if repair { REPAIR_MAX_ROUNDS } else { self.generate_max_rounds },
                    "tools": fields.get("tool_calls").cloned().unwrap_or_default(),
                    "text": fields.get("text").cloned().unwrap_or_default(),
                });
                if let Some(kind) = self.wire.kind_tag {
                    payload["kind"] = serde_json::json!(kind);
                }
                payload
            }
            "round_feedback" if self.wire.translate_round_feedback => {
                // 损坏降级上 WS：否则用户会看到轮次凭空从 1 跳到 2，不知道
                // 中间发生过一次传输损坏与自动恢复。复用 round 行渲染。
                let mut payload = serde_json::json!({
                    "phase": "round",
                    "loop": fields.get("loop"),
                    "round": fields.get("round"),
                    "tools": [],
                    "text": format!(
                        "工具调用参数损坏，本轮操作未执行——已自动要求模型重新提交（第 {} 次）",
                        fields.get("feedbacks_used").and_then(serde_json::Value::as_u64).unwrap_or(0),
                    ),
                });
                if let Some(kind) = self.wire.kind_tag {
                    payload["kind"] = serde_json::json!(kind);
                }
                payload
            }
            _other if fields.get("phase").is_some() => {
                let mut payload = fields.clone();
                if let Some(kind) = self.wire.kind_tag {
                    if let Some(object) = payload.as_object_mut() {
                        object.insert("kind".to_owned(), serde_json::json!(kind));
                    }
                }
                payload
            }
            _ => return,
        };
        if self.wire.lesson_stream {
            service.emit_lesson_event(payload);
        } else {
            service.emit_course_event(payload);
        }
    }
}

/// 双循环外壳的每流插头。`Ctx` 是各流自己的 `LoopContext`（工具句柄捕获
/// 它；draft/published 槽位保持裸 `Arc<Mutex<Option<_>>>`，测试可以直接
/// 脚本化），`Output` 是 finish 门发布的产物类型。除提示词与工具集外的
/// 一切行为差异都必须以方法/配置表的形式声明在这里。
#[async_trait::async_trait]
pub(crate) trait FlowCycle: Send + Sync {
    type Ctx: LoopEventSink + Send + Sync + 'static;
    type Output: Send + 'static;

    /// 流名：耗尽报告与缺草稿错误消息的前缀。
    fn name(&self) -> &'static str;
    /// 显式预算表。
    fn budgets(&self) -> LoopBudgets;
    /// 线上翻译差异表。
    fn wire(&self) -> WireConfig;
    fn generate_system(&self) -> &'static str;
    fn repair_system(&self) -> &'static str;
    /// 工具白名单。`repair_face = true` 表示修复轮（大纲/课时收窄掉 start
    /// 工具；学习图无 start 工具，两个面相同）。
    fn tools(&self, ctx: Arc<Self::Ctx>, repair_face: bool) -> Vec<OneShotTool>;
    /// finish 门（确定性审计）：`Ok` 即发布；`UnprocessableEntity` 表示
    /// danger findings 未清，修复循环继续。学习图的门是 async（终审软门
    /// + 落库），大纲/课时为同步语义——统一在 async 插头之后。
    async fn finish(&self, ctx: &Self::Ctx, draft_id: &str) -> Result<Self::Output, AppError>;
    /// 草稿 revision（空闲检测）。
    fn revision(&self, ctx: &Self::Ctx, draft_id: &str) -> Result<u64, AppError>;
    /// 修复轮的审计依据：返回全文（模型修复的输入）。学习图只取文本；
    /// 大纲/课时先发 `audit` 进度帧（UI 的审计徽标）再取文本。
    fn repair_audit(&self, ctx: &Self::Ctx, draft_id: &str, round: usize) -> Result<String, AppError>;
    /// 修复预算耗尽时的最终审计快照（全部为纯文本，无帧）。
    fn exhausted_audit(&self, ctx: &Self::Ctx, draft_id: &str) -> Result<String, AppError>;
    /// `publish_ok` 帧的每流形状（学习图 record_id / 大纲 modules /
    /// 课时 activities）。
    fn publish_frame(
        output: &Self::Output,
        loop_label: &str,
        round: Option<usize>,
    ) -> serde_json::Value;
    /// 发布成功后的每流收尾（课时清轮次日志；学习图/大纲的清理在各自的
    /// run_generation 尾部）。
    fn on_published(&self, ctx: &Self::Ctx);
    /// 空手结束 nudge 中的工具名片段（nudge 模板三流同文，仅此段不同）。
    fn idle_nudge_actions(&self) -> &'static str;
    /// 循环内错误映射：学习图附加存活草稿诊断（并就地处理取消）；大纲/
    /// 课时透传（取消归一在各自 run_generation 尾部）。
    fn map_loop_error(&self, ctx: &Self::Ctx, error: AppError) -> AppError;
    /// 发布优先于循环错误（ADR-0004 显式差异）：学习图 true——末轮发布
    /// 可存活于预算墙；大纲/课时 false——保持错误优先的历史顺序。
    fn publish_before_error(&self) -> bool;
    /// 生成循环后没有草稿可取时的（日志 reason，错误消息）。
    fn missing_draft_texts(&self) -> (&'static str, &'static str);
    /// 草稿槽锁损坏时的错误。
    fn slot_poisoned(&self) -> AppError;
    fn draft_slot(ctx: &Self::Ctx) -> &Arc<Mutex<Option<String>>>;
    fn published_slot(ctx: &Self::Ctx) -> &Arc<Mutex<Option<Self::Output>>>;
}

/// 双循环外壳：生成循环（全工具面 + 开场 user 消息）→ finish 门 → 审计
/// 驱动的修复循环（[`REPAIR_LOOP_LIMIT`] 轮，每轮失败带定位回灌 + 空闲
/// nudge）。`round_tokens` 由调用方按流预算解析（学习图在此收敛到模型
/// output 上限）；思考模式禁用（三流一致的实验中策略）。`provider` 注入
/// 以便测试脚本化 LLM（与 `run_one_shot_turn_with_provider` 同一 seam）。
pub(crate) async fn run_loops<F: FlowCycle>(
    flow: &F,
    provider: Arc<dyn LlmProvider>,
    model: &str,
    user_text: &str,
    ctx: Arc<F::Ctx>,
    round_tokens: u32,
) -> Result<F::Output, AppError> {
    let budgets = flow.budgets();

    // ── 生成循环：全工具面，注入的开场消息作为 user 轮 ──────────────────
    let generate_tools = flow.tools(Arc::clone(&ctx), false);
    let mut start = serde_json::json!({
        "max_rounds": budgets.generate_max_rounds,
        "tool_count": generate_tools.len(),
    });
    if let Some(phase) = flow.wire().generate_start_phase {
        start["phase"] = serde_json::json!(phase);
    }
    ctx.log("generate_loop_start", start);
    let loop_result = run_agent_loop(
        provider.clone(),
        model,
        flow.generate_system(),
        user_text,
        &generate_tools,
        budgets.generate_max_rounds,
        round_tokens,
        ThinkingConfig::Disabled,
        GENERATE_REASONING_EFFORT,
        "generate",
        Some(ctx.as_ref()),
    )
    .await;
    // 发布优先级是显式差异（`publish_before_error`）：发布可能落在循环的
    // 最后一轮（预算恰好在发布轮耗尽）。
    if flow.publish_before_error() {
        if let Some(output) = take_published::<F>(&ctx) {
            return Ok(publish(flow, &ctx, output, "generate", None));
        }
    }
    let final_text = loop_result.map_err(|error| flow.map_loop_error(ctx.as_ref(), error))?;
    if !flow.publish_before_error() {
        if let Some(output) = take_published::<F>(&ctx) {
            return Ok(publish(flow, &ctx, output, "generate", None));
        }
    }
    let draft_id = match F::draft_slot(&ctx).lock() {
        Ok(slot) => slot.clone().ok_or_else(|| {
            let (reason, message) = flow.missing_draft_texts();
            ctx.log("generate_loop_end", serde_json::json!({
                "ok": false,
                "reason": reason,
                "text": log_text(&final_text),
            }));
            AppError::Internal(message.to_owned())
        })?,
        Err(_) => return Err(flow.slot_poisoned()),
    };
    ctx.log("generate_loop_end", serde_json::json!({
        "ok": true,
        "draft_id": draft_id,
        "text": log_text(&final_text),
    }));

    // ── 修复循环：审计门有最终决定权 ────────────────────────────────────
    // finish 门即确定性门禁：成功代表草稿过关；UnprocessableEntity 表示
    // danger findings 未清，修复循环拿到完整报告。`idle_nudge` 在模型修
    // 复轮空手（revision 未变）时把警告带进下一轮——模型可能「回复而不修
    // 复」。
    let mut idle_nudge: Option<String> = None;
    for round in 0..REPAIR_LOOP_LIMIT {
        let mut start = serde_json::json!({
            "round": round + 1,
            "draft_id": draft_id,
        });
        if let Some(phase) = flow.wire().repair_start_phase {
            start["phase"] = serde_json::json!(phase);
        }
        ctx.log("repair_loop_start", start);
        match flow.finish(ctx.as_ref(), &draft_id).await {
            Ok(output) => {
                return Ok(publish(flow, &ctx, output, "repair", Some(round + 1)));
            }
            Err(AppError::UnprocessableEntity(_)) => {
                ctx.log("finish_blocked", serde_json::json!({
                    "round": round + 1,
                    "draft_id": draft_id,
                }));
            }
            Err(error) => return Err(error),
        }
        let audit = flow.repair_audit(ctx.as_ref(), &draft_id, round + 1)?;
        let repair_user = match &idle_nudge {
            Some(nudge) => format!("{nudge}\n\n{audit}"),
            None => audit,
        };
        let repair_tools = flow.tools(Arc::clone(&ctx), true);
        let revision_before = flow.revision(ctx.as_ref(), &draft_id)?;
        let loop_result = run_agent_loop(
            provider.clone(),
            model,
            flow.repair_system(),
            &repair_user,
            &repair_tools,
            REPAIR_MAX_ROUNDS,
            round_tokens,
            ThinkingConfig::Disabled,
            REPAIR_REASONING_EFFORT,
            "repair",
            Some(ctx.as_ref()),
        )
        .await;
        // 发布优先于循环错误（同生成循环，按流的显式声明）。
        if flow.publish_before_error() {
            if let Some(output) = take_published::<F>(&ctx) {
                return Ok(publish(flow, &ctx, output, "repair", Some(round + 1)));
            }
        }
        let final_text = loop_result.map_err(|error| flow.map_loop_error(ctx.as_ref(), error))?;
        if !flow.publish_before_error() {
            if let Some(output) = take_published::<F>(&ctx) {
                return Ok(publish(flow, &ctx, output, "repair", Some(round + 1)));
            }
        }
        let revision_after = flow.revision(ctx.as_ref(), &draft_id)?;
        if revision_after == revision_before {
            ctx.log("repair_loop_idle", serde_json::json!({
                "round": round + 1,
                "revision": revision_after,
                "text": log_text(&final_text),
            }));
            idle_nudge = Some(format!(
                "警告：你上一轮没有对草稿做任何修改（revision 仍是 {revision_after}），只回复了文字。\
                 禁止空手结束：本轮必须调用 {}；只输出文字而不调用任何工具，会被判定为拒绝修复，\
                 整个生成将以失败告终。",
                flow.idle_nudge_actions(),
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

    // 预算耗尽：如实报告存活 findings（草稿保留，人或后续运行可续）。
    let audit = flow.exhausted_audit(ctx.as_ref(), &draft_id)?;
    ctx.log("repair_budget_exhausted", serde_json::json!({
        "draft_id": draft_id,
    }));
    Err(AppError::UnprocessableEntity(format!(
        "{} agent exhausted {REPAIR_LOOP_LIMIT} repair loops; the draft survives with these blocking findings:\n{audit}",
        flow.name(),
    )))
}

/// 发布收尾：publish_ok 帧 + 每流钩子，三个发布点共用。
fn publish<F: FlowCycle>(
    flow: &F,
    ctx: &Arc<F::Ctx>,
    output: F::Output,
    loop_label: &str,
    round: Option<usize>,
) -> F::Output {
    ctx.log("publish_ok", F::publish_frame(&output, loop_label, round));
    flow.on_published(ctx);
    output
}

/// 把已发布的产物取出（一次）——每个循环之后都调用，模型可能在任一循环
/// 中合法发布。
fn take_published<F: FlowCycle>(ctx: &Arc<F::Ctx>) -> Option<F::Output> {
    F::published_slot(ctx).lock().ok().and_then(|mut slot| slot.take())
}

#[cfg(test)]
pub(crate) mod test_support {
    //! 三条学习流共享的脚本化测试底座（原三份复制假件的超集）：
    //! `seen_effort`（学习图的修复升档观测）与 `open_failures`（大纲的
    //! 开流故障注入）对所有流可用；各流的 fixture 仍留在各自的测试模块。

    use std::sync::{Arc, Mutex};

    use nomi_providers::{LlmProvider, ProviderError};
    use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
    use nomi_types::message::{Message, StopReason, TokenUsage};
    use nomifun_common::AppError;
    use nomifun_learning::{LearningCompleter, LearningService};
    use tokio::sync::mpsc;

    use crate::one_shot::OneShotDeps;

    /// Scripted fake provider: each `stream` call pops the next script
    /// entry; every observed request (tool names + messages + thinking +
    /// reasoning effort) is recorded. Queued `open_failures` (LIFO) fail
    /// BEFORE recording anything or touching the script — models a
    /// transient connect/429 fault at the open boundary.
    pub(crate) struct ScriptedProvider {
        script: Mutex<Vec<Vec<LlmEvent>>>,
        pub(crate) open_failures: Mutex<Vec<ProviderError>>,
        pub(crate) seen_tool_names: Mutex<Vec<Vec<String>>>,
        pub(crate) seen_messages: Mutex<Vec<Vec<Message>>>,
        pub(crate) seen_thinking: Mutex<Vec<Option<ThinkingConfig>>>,
        pub(crate) seen_effort: Mutex<Vec<Option<String>>>,
    }

    impl ScriptedProvider {
        pub(crate) fn new(script: Vec<Vec<LlmEvent>>) -> Arc<Self> {
            Arc::new(Self {
                script: Mutex::new(script),
                open_failures: Mutex::new(Vec::new()),
                seen_tool_names: Mutex::new(Vec::new()),
                seen_messages: Mutex::new(Vec::new()),
                seen_thinking: Mutex::new(Vec::new()),
                seen_effort: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn stream(
            &self,
            request: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            if let Some(error) = self.open_failures.lock().unwrap().pop() {
                return Err(error);
            }
            self.seen_tool_names
                .lock()
                .unwrap()
                .push(request.tools.iter().map(|tool| tool.name.clone()).collect());
            self.seen_messages.lock().unwrap().push(request.messages.clone());
            self.seen_thinking.lock().unwrap().push(request.thinking.clone());
            self.seen_effort.lock().unwrap().push(request.reasoning_effort.clone());
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

    pub(crate) fn done(stop_reason: StopReason) -> LlmEvent {
        LlmEvent::Done { stop_reason, usage: TokenUsage::default() }
    }

    pub(crate) fn tool_use(name: &str, input: serde_json::Value) -> LlmEvent {
        LlmEvent::ToolUse {
            id: format!("call_{name}"),
            name: name.into(),
            input,
            extra: None,
        }
    }

    /// Completer whose reply never parses as a scope reference: every draft
    /// starts scope-free, keeping the deterministic audit fully structural.
    pub(crate) struct FakeCompleter;

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
    pub(crate) struct NoopBroadcaster;

    impl nomifun_realtime::UserEventSink for NoopBroadcaster {
        fn send_to_user(
            &self,
            _user_id: &str,
            _event: nomifun_api_types::WebSocketMessage<serde_json::Value>,
        ) {
        }
    }

    /// 引擎测试用的空 provider 仓储 deps（resolve 阶段直接走默认失败分支，
    /// 循环本体由脚本 provider 驱动）。
    pub(crate) fn test_deps() -> OneShotDeps {
        use crate::knowledge_completer::tests::{ListOnlyModelRepo, ListOnlyRepo};
        OneShotDeps {
            provider_repo: Arc::new(ListOnlyRepo(Vec::new())),
            provider_model_repo: Arc::new(ListOnlyModelRepo(Vec::new())),
            encryption_key: [0u8; 32],
            workspace: std::env::temp_dir(),
        }
    }

    /// [`test_service_with`] 的默认变体（scope 分析永远降级为无 scope）。
    pub(crate) async fn test_service() -> (Arc<LearningService>, tempfile::TempDir) {
        test_service_with(Arc::new(FakeCompleter)).await
    }

    /// A service wired with the given completer and a scratch knowledge dir;
    /// the temp dir stays alive for the test's duration.
    pub(crate) async fn test_service_with(
        completer: Arc<dyn LearningCompleter>,
    ) -> (Arc<LearningService>, tempfile::TempDir) {
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
        service.set_generation_dependencies(knowledge_service, completer);
        (service, dir)
    }
}

#[cfg(test)]
mod tests {
    //! 行为差异矩阵 + 线上翻译器单测——ADR-0004 的反损失验收：三流之间
    //! 的每一个已声明差异在此钉死，无声归并会在这里先红。

    use std::sync::{Arc, Mutex};

    use nomifun_realtime::UserEventSink;
    use nomifun_learning::{LearningEventEmitter, LearningService};

    use super::test_support::{FakeCompleter, test_service_with};
    use super::LoopChannel;
    use crate::course_outline_loop::{BUDGETS as CO_BUDGETS, WIRE as CO_WIRE};
    use crate::learning_graph_loop::{BUDGETS as LG_BUDGETS, WIRE as LG_WIRE};
    use crate::lesson_content_loop::{BUDGETS as LS_BUDGETS, WIRE as LS_WIRE};

    /// 捕获 learning 事件帧的 sink：翻译器测试直接断言（流名，帧）。
    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<(String, serde_json::Value)>>);

    impl RecordingSink {
        fn frames(&self, stream: &str) -> Vec<serde_json::Value> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|(name, _)| name == stream)
                .map(|(_, data)| data.clone())
                .collect()
        }
    }

    impl UserEventSink for RecordingSink {
        fn send_to_user(
            &self,
            _user_id: &str,
            event: nomifun_api_types::WebSocketMessage<serde_json::Value>,
        ) {
            self.0.lock().unwrap().push((event.name, event.data));
        }
    }

    async fn recording_service() -> (Arc<LearningService>, tempfile::TempDir, Arc<RecordingSink>) {
        let (service, dir) = test_service_with(Arc::new(FakeCompleter)).await;
        let sink = Arc::new(RecordingSink::default());
        service.set_event_sink(LearningEventEmitter::new(
            Arc::clone(&sink) as Arc<dyn UserEventSink>,
            Arc::from("0190f5fe-7c00-7a00-8000-000000000001"),
        ));
        (service, dir, sink)
    }

    /// 行为差异矩阵：三流的预算与线上翻译配置逐字钉死。统一某个差异 =
    /// 修改对应流的声明 + 更新这里的断言（ADR-0004）。
    #[test]
    fn flow_difference_matrix_is_pinned() {
        // 预算：学习图 100 轮 / 32768 token / 1800s；大纲与课时与 loop_core
        // 的共享默认逐字一致。
        assert_eq!(
            (LG_BUDGETS.generate_max_rounds, LG_BUDGETS.round_tokens, LG_BUDGETS.timeout_secs),
            (100, 32768, 1800)
        );
        for (name, budgets) in [("course outline", CO_BUDGETS), ("lesson content", LS_BUDGETS)] {
            assert_eq!(budgets.generate_max_rounds, crate::loop_core::GENERATE_MAX_ROUNDS, "{name}");
            assert_eq!(budgets.round_tokens, crate::loop_core::AGENT_MAX_TOKENS, "{name}");
            assert_eq!(budgets.timeout_secs, crate::loop_core::TOTAL_TIMEOUT_SECS, "{name}");
        }
        // 线上差异表：kind 标记、round_feedback 翻译、轮次日志、start 阶段
        // 帧、事件流选择。
        assert_eq!(LG_WIRE.kind_tag, Some("learning_graph"));
        assert!(LG_WIRE.translate_round_feedback);
        assert_eq!(LG_WIRE.round_log_gen_label, Some("构建"));
        assert_eq!(LG_WIRE.generate_start_phase, Some("generating"));
        assert_eq!(LG_WIRE.repair_start_phase, Some("repairing"));
        assert!(!LG_WIRE.lesson_stream);
        assert_eq!(CO_WIRE.kind_tag, None);
        assert!(!CO_WIRE.translate_round_feedback);
        assert_eq!(CO_WIRE.round_log_gen_label, None);
        assert_eq!(CO_WIRE.generate_start_phase, None);
        assert_eq!(CO_WIRE.repair_start_phase, None);
        assert!(!CO_WIRE.lesson_stream);
        assert_eq!(LS_WIRE.kind_tag, None);
        assert!(!LS_WIRE.translate_round_feedback);
        assert_eq!(LS_WIRE.round_log_gen_label, Some("生成"));
        assert_eq!(LS_WIRE.generate_start_phase, None);
        assert_eq!(LS_WIRE.repair_start_phase, None);
        assert!(LS_WIRE.lesson_stream);
    }

    #[tokio::test]
    async fn lg_wire_tags_frames_translates_round_feedback_and_keeps_round_log() {
        let (service, _dir, sink) = recording_service().await;
        let channel = LoopChannel::new(LG_WIRE, LG_BUDGETS.generate_max_rounds);
        channel.emit(
            &service,
            "agent_round",
            &serde_json::json!({
                "loop": "generate", "round": 3, "text": "计划：覆盖10/12大块",
                "tool_calls": [{ "name": "lg_patch", "is_error": false }],
            }),
        );
        channel.emit(
            &service,
            "round_feedback",
            &serde_json::json!({
                "loop": "generate", "round": 4, "feedbacks_used": 1, "error": "malformed",
            }),
        );
        // 无 phase 的 loop 内部事件不上线。
        channel.emit(
            &service,
            "finish_blocked",
            &serde_json::json!({ "round": 1, "draft_id": "d" }),
        );
        let frames = sink.frames("learning.course-generation");
        assert_eq!(frames.len(), 2, "phase-less events stay off the wire: {frames:?}");
        assert_eq!(frames[0]["kind"], "learning_graph");
        assert_eq!(frames[0]["phase"], "round");
        assert_eq!(frames[0]["max_rounds"], 100);
        assert_eq!(frames[1]["kind"], "learning_graph");
        assert!(frames[1]["text"].as_str().unwrap().contains("第 1 次"));
        let log = channel.round_log_snapshot();
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("第3轮(构建)") && log[0].contains("lg_patch✓"), "{log:?}");
    }

    #[tokio::test]
    async fn co_wire_passes_frames_verbatim_without_kind_or_round_log() {
        let (service, _dir, sink) = recording_service().await;
        let channel = LoopChannel::new(CO_WIRE, CO_BUDGETS.generate_max_rounds);
        channel.emit(
            &service,
            "agent_round",
            &serde_json::json!({
                "loop": "repair", "round": 2, "text": "x",
                "tool_calls": [{ "name": "co_patch", "is_error": false }],
            }),
        );
        channel.emit(
            &service,
            "round_feedback",
            &serde_json::json!({
                "loop": "repair", "round": 3, "feedbacks_used": 1, "error": "malformed",
            }),
        );
        // 带 phase 的事件原样上线（audit / session_end / publish_ok），不加 kind。
        channel.emit(
            &service,
            "audit",
            &serde_json::json!({ "phase": "audit", "danger": 1 }),
        );
        let frames = sink.frames("learning.course-generation");
        assert_eq!(frames.len(), 2, "round_feedback stays off the co wire: {frames:?}");
        assert!(frames[0].get("kind").is_none());
        assert_eq!(frames[0]["max_rounds"], crate::loop_core::REPAIR_MAX_ROUNDS);
        assert!(frames[1]["phase"] == "audit" && frames[1].get("kind").is_none());
        assert!(channel.round_log_snapshot().is_empty(), "co keeps no round log");
    }

    #[tokio::test]
    async fn ls_wire_targets_the_lesson_stream() {
        let (service, _dir, sink) = recording_service().await;
        let channel = LoopChannel::new(LS_WIRE, LS_BUDGETS.generate_max_rounds);
        channel.emit(
            &service,
            "agent_round",
            &serde_json::json!({
                "loop": "generate", "round": 1, "text": "",
                "tool_calls": [{ "name": "ls_start", "is_error": false }],
            }),
        );
        assert!(sink.frames("learning.course-generation").is_empty());
        let frames = sink.frames("learning.lesson-generation");
        assert_eq!(frames.len(), 1, "{frames:?}");
        assert!(frames[0].get("kind").is_none());
        assert_eq!(frames[0]["max_rounds"], crate::loop_core::GENERATE_MAX_ROUNDS);
        let log = channel.round_log_snapshot();
        assert!(log[0].contains("第1轮(生成)") && log[0].contains("ls_start✓"), "{log:?}");
    }
}
