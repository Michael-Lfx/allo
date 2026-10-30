//! 长任务注册表（三轮 t2 + m7 资源闸）：进程内 keyed 注册 + `CancellationToken`
//! 真取消 + 全局并发上限/排队（m7）+ 快照/统一取消端点的数据源 + begin/finish
//! 观测旁路。
//!
//! - **键**：`{kind}:{course_id}:{node_id|-}`；同键并发 `begin` 拒绝
//!   （Conflict；排队态同样拒绝——防双击双跑），终态记录被同键新任务替换
//!   （历史看批行/草稿，不靠内存）。
//! - **状态机**：`queued → running → done | failed | cancelled`（m7 起 queued
//!   只在过闸路径出现；`NOMI_LEARNING_TASK_GATE=off` 时 begin 直跑、无
//!   queued），终态**首写即定**（幂等）：取消先到 = cancelled，任务先完成 =
//!   竞速落败如实 done。
//! - **资源闸（m7，learnhub ADR-0029 缩编版）**：全局
//!   [`tokio::sync::Semaphore`]（cap [`TASK_GATE_LIMIT`]，FIFO 公平）+ 排队
//!   可见（快照 `queued_at_ms`/`queue_position`）+ 排队期取消窗口双侧闭合——
//!   取消与 queued→running 迁移都在 tasks 锁内落账，acquire 到手后同锁复查
//!   终态：半观察到的 queued 记录不可能跑 body（取消侧先写终态再触发 token，
//!   等待侧 select 两臂都过同一把锁复查）。无完整资源元组注册表：allo 任务
//!   面全部学习域任务，键级互斥（begin Conflict + 生成槽）已存在，全局 cap
//!   之上不再叠 course 级串行（register_alias_tx 在 apply 事务内、defnear
//!   纯读，风险面与 learnhub Grow 持 Registry 不同）。
//! - **panic 容量防漏（m7）**：许可住 [`GenerationTaskGuard`]（RAII）——
//!   tokio 任务 panic/abort 展开时 guard drop 归还许可并按 failed 留痕，
//!   任何退出路径都不泄漏容量（选 RAII 而非 catch_unwind：任务体是 async
//!   future，catch_unwind 需逐 poll 包 AssertUnwindSafe；tokio::spawn 边界
//!   本就隔离 panic，RAII 覆盖 panic/abort/早退全部路径且零逐点代码）。
//! - **排序**：注册序 `AtomicU64` 倒序（活跃置顶，同秒稳定）。
//! - **真取消**：每个任务一枚 token；任务体在检查点轮询 + 经
//!   [`CancellationCompleter`] 在 LLM 调用内 `select!` 真中断（中断 = 断
//!   HTTP；批事务原子落库 = 中断零落库）。课时内容循环尚未接 token
//!   （ADR-0004 显式差异）——e4 起课时任务已后台化（脱离请求生命周期，
//!   断连**不再**中断生成），远程取消仍未接线，对它的取消请求如实拒绝
//!   （排队期课时任务取消安全：body 零执行，不涉及 token 接线）。
//! - **观测旁路**：begin/finish 两型 JSONL 行经注入的 observer 旁路落盘
//!   （非执法；observer 缺席或写失败都静默降级）。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::time::Instant;

use nomifun_common::AppError;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::completer::LearningCompleter;

/// 任务类别（键的第一段；快照面的机器名，人面文案由 `topic`/`detail` 承担）。
pub(crate) const KIND_GROWTH: &str = "growth";
pub(crate) const KIND_STRUCTURE: &str = "structure";
pub(crate) const KIND_STRUCTURE_MOUNT: &str = "structure_mount";
pub(crate) const KIND_STRUCTURE_REPAIR: &str = "structure_repair";
pub(crate) const KIND_OUTLINE: &str = "outline";
pub(crate) const KIND_LESSON: &str = "lesson";

/// 客观题型 SQL 白名单（单源；挂载种子/教新结算/quiz 选题/题目列表同筛——
/// AI 批改轨不入客观题面）。嵌进 `kind IN (…)` 使用。
pub(crate) const OBJECTIVE_KIND_SQL_LIST: &str =
    "'single_choice', 'true_false', 'fill_in_blank', 'multi_choice', 'numeric', \
     'ordering', 'matching'";

/// 任务状态（终态首写即定）。
pub(crate) const STATE_QUEUED: &str = "queued";
pub(crate) const STATE_RUNNING: &str = "running";
pub(crate) const STATE_DONE: &str = "done";
pub(crate) const STATE_FAILED: &str = "failed";
pub(crate) const STATE_CANCELLED: &str = "cancelled";

/// 全局在跑任务上限（m7 资源闸；learnhub MAX_RUNNING_TASKS 同值 5）。只约束
/// 过闸的 `begin` 路径；`NOMI_LEARNING_TASK_GATE=off` 时无上限直跑（闸前行为）。
pub(crate) const TASK_GATE_LIMIT: usize = 5;

/// 快照命令的单行视图（`GET /api/learning/generation-tasks`）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GenerationTaskView {
    pub key: String,
    pub kind: String,
    pub course_id: String,
    pub node_id: Option<String>,
    pub topic: String,
    /// queued | running | done | failed | cancelled（queued 只在过闸路径出现）。
    pub state: String,
    /// step 机器面（英文常量；前端不造步骤文案）。
    pub step: Option<String>,
    /// detail 人面（后端单源的单句中文）。
    pub detail: Option<String>,
    pub elapsed_secs: Option<u64>,
    /// 排队登记时刻（unix ms；非 queued = None，m7 排队可见）。
    pub queued_at_ms: Option<i64>,
    /// 排队位次（1 起，注册序升序 = FIFO 提交序；非 queued = None）。
    pub queue_position: Option<u32>,
}

struct TaskRecord {
    sequence: u64,
    kind: &'static str,
    course_id: String,
    node_id: Option<String>,
    topic: String,
    state: &'static str,
    token: CancellationToken,
    step: Option<String>,
    detail: Option<String>,
    started_at: Instant,
    /// 排队登记时刻（unix ms；过闸路径在 queued 落位时写）。
    queued_at_ms: Option<i64>,
}

/// 闸许可的 RAII 包装：drop（finish_*/panic/abort 任何退出路径）即归还许可
/// 并回收在跑读数——panic 容量防漏的落点（模块注释「panic 容量防漏」节）。
/// 在跑读数两臂都记（off 臂无许可可还，但读数要反映直跑形态）。
struct GatePermit {
    permit: Option<OwnedSemaphorePermit>,
    registry: Arc<GenerationTaskRegistry>,
}

impl GatePermit {
    /// 过闸臂：持真实许可（drop 归还信号量）。
    fn acquire(
        registry: &Arc<GenerationTaskRegistry>,
        permit: OwnedSemaphorePermit,
    ) -> Self {
        Self {
            permit: Some(permit),
            registry: Self::count_running(registry),
        }
    }

    /// off 臂：无许可，仅记在跑读数（直跑形态读数）。
    fn uncounted(registry: &Arc<GenerationTaskRegistry>) -> Self {
        Self {
            permit: None,
            registry: Self::count_running(registry),
        }
    }

    fn count_running(registry: &Arc<GenerationTaskRegistry>) -> Arc<GenerationTaskRegistry> {
        let running = registry
            .gate_running
            .fetch_add(1, AtomicOrdering::SeqCst)
            + 1;
        registry
            .gate_running_peak
            .fetch_max(running, AtomicOrdering::SeqCst);
        Arc::clone(registry)
    }
}

impl Drop for GatePermit {
    fn drop(&mut self) {
        self.registry.gate_running.fetch_sub(1, AtomicOrdering::SeqCst);
        // 许可（若过闸）随结构体销毁归还信号量（字段仅为 Drop 副作用持有）。
        let _ = &self.permit;
    }
}

/// 一次长任务的注册句柄：`token()` 供任务体接检查点；显式 `finish_*` 收尾
/// （终态首写即定）；未收尾即 drop（任务被 abort/panic）按 failed 留痕。
/// 过闸任务的许可住守卫内（RAII）：任何退出路径（含 panic 展开）都归还
/// 许可，容量不随异常退出泄漏。`usage()` 是相位耗时读数的上报面（e1）：
/// 任务体的计数包装往里加增量，finish 行聚合落档。
pub(crate) struct GenerationTaskGuard {
    registry: Arc<GenerationTaskRegistry>,
    key: String,
    token: CancellationToken,
    /// 在跑占用（过闸 = 真许可，off 臂 = 仅读数；drop 即归还，任何退出路径
    /// 含 panic 展开都不泄漏）。
    permit: GatePermit,
    /// LLM 用量累计（e1 相位耗时读数）：begin 铸造，任务体经计数包装上报，
    /// 显式 finish 与 drop 兜底两条收尾路径都进 finish 行。
    usage: crate::completer::LlmUsageMeter,
}

impl GenerationTaskGuard {
    /// 任务体的取消令牌（检查点轮询 + `select!` 竞速）。
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// LLM 用量累计句柄（e1 相位耗时读数）：计数包装（结构/生长 =
    /// [`CountingCompleter`]，课时 = agent 侧 CountingProvider）上报增量。
    pub fn usage(&self) -> crate::completer::LlmUsageMeter {
        self.usage.clone()
    }

    /// 进度叙事（step 机器面 / detail 人面，后端单源）：快照与事件共用。
    pub fn set_step(&self, step: &str, detail: &str) {
        self.registry.set_step(&self.key, step, detail);
    }

    pub fn finish_done(self, detail: impl Into<String>) {
        self.finish(STATE_DONE, detail);
    }

    pub fn finish_failed(self, detail: impl Into<String>) {
        self.finish(STATE_FAILED, detail);
    }

    pub fn finish_cancelled(self) {
        self.finish(STATE_CANCELLED, "已取消");
    }

    fn finish(mut self, state: &'static str, detail: impl Into<String>) {
        let usage = self.usage.snapshot();
        self.registry
            .finish(&self.key, state, detail.into(), usage);
        // 阻止 Drop 再标 failed（finish 已消费语义，这里只是双保险）。
        self.key.clear();
    }
}

impl Drop for GenerationTaskGuard {
    fn drop(&mut self) {
        if !self.key.is_empty() {
            let usage = self.usage.snapshot();
            self.registry.finish(
                &self.key.clone(),
                STATE_FAILED,
                "任务中断（未收尾即退出）".into(),
                usage,
            );
        }
        // 许可（若过闸）在此归还——panic/abort 展开也走这里，容量不泄漏。
        let _ = &self.permit;
    }
}

/// 进程内注册表。内存态：重启即清（在跑任务本就随进程消亡，终态留痕由
/// 批行/草稿等持久面承担）。
pub(crate) struct GenerationTaskRegistry {
    tasks: std::sync::Mutex<HashMap<String, TaskRecord>>,
    sequence: AtomicU64,
    /// 观测旁路（begin/finish JSONL 行；非执法，写失败降级）。由组合根
    /// 注入（`LearningService::set_generation_task_observer`）。
    observer: std::sync::Mutex<Option<Arc<dyn Fn(&str) + Send + Sync>>>,
    /// 资源闸（m7）：`Some` = 过闸（cap [`TASK_GATE_LIMIT`]，FIFO）；
    /// `None` = off（`NOMI_LEARNING_TASK_GATE=off`，无上限直跑 = 闸前行为）。
    gate: Option<Arc<Semaphore>>,
    /// 闸面读数（smoke 对照卡数据源）：在跑/排队的当前值与峰值。在跑计数
    /// 跟许可生命周期走（[`GatePermit`] 构造 +1 / drop -1，真实资源占用面）；
    /// 排队计数跟 queued 状态迁移走（登记 +1，acquire 迁移或排队取消 -1，
    /// 每条记录离开 queued 恰一次）。
    gate_running: AtomicUsize,
    gate_running_peak: AtomicUsize,
    gate_queued: AtomicUsize,
    gate_queued_peak: AtomicUsize,
}

impl GenerationTaskRegistry {
    pub(crate) fn new() -> Self {
        Self::with_gate(Self::gate_enabled_from_env(), TASK_GATE_LIMIT)
    }

    /// 以给定闸状态建注册表（测试面双臂显式构造，避免环境变量串扰）。
    pub(crate) fn with_gate(enabled: bool, limit: usize) -> Self {
        Self {
            tasks: std::sync::Mutex::new(HashMap::new()),
            sequence: AtomicU64::new(1),
            observer: std::sync::Mutex::new(None),
            gate: enabled.then(|| Arc::new(Semaphore::new(limit.max(1)))),
            gate_running: AtomicUsize::new(0),
            gate_running_peak: AtomicUsize::new(0),
            gate_queued: AtomicUsize::new(0),
            gate_queued_peak: AtomicUsize::new(0),
        }
    }

    /// 开关读数（进程生命周期一次）：`NOMI_LEARNING_TASK_GATE` off/0/false =
    /// 关（无上限直跑）；未设或其它值 = 开。
    fn gate_enabled_from_env() -> bool {
        std::env::var(crate::switches::TASK_GATE_SWITCH_ENV)
            .map(|value| !crate::switches::is_task_gate_disabled_value(&value))
            .unwrap_or(true)
    }

    /// 闸面读数（smoke 对照卡数据源）。
    pub(crate) fn gate_readout(&self) -> crate::models::LearningTaskGateReadout {
        crate::models::LearningTaskGateReadout {
            enabled: self.gate.is_some(),
            limit: TASK_GATE_LIMIT,
            running: self.gate_running.load(AtomicOrdering::SeqCst),
            running_peak: self.gate_running_peak.load(AtomicOrdering::SeqCst),
            queued: self.gate_queued.load(AtomicOrdering::SeqCst),
            queued_peak: self.gate_queued_peak.load(AtomicOrdering::SeqCst),
        }
    }

    pub(crate) fn set_observer(&self, observer: Arc<dyn Fn(&str) + Send + Sync>) {
        *self
            .observer
            .lock()
            .expect("generation task observer lock poisoned") = Some(observer);
    }

    fn emit_observation(&self, line: String) {
        if let Ok(guard) = self.observer.lock() {
            if let Some(observer) = guard.as_ref() {
                observer(&line);
            }
        }
    }

    fn key(kind: &str, course_id: &str, node_id: Option<&str>) -> String {
        format!("{kind}:{course_id}:{}", node_id.unwrap_or("-"))
    }

    /// 登记一个长任务（m7 起异步、过闸）：同键已有 `queued|running` 记录 =
    /// 并发拒绝（Conflict）；同键终态记录被新任务替换。观测旁路发 begin 行。
    /// `self: &Arc<Self>`——guard 需要回引注册表（finish/Drop 收尾）。
    ///
    /// 流程（learnhub `run_task` 提交段的 tokio 等价形态）：
    /// 1. 注册即落 `queued` 记录（gate off = 直落 running，闸前行为零变化）；
    /// 2. `Semaphore::acquire_owned` 与 token 取消 `select!` 等准入；
    /// 3. 许可到手后**同锁复查终态**（排队取消窗口双侧闭合的裁决点：取消侧
    ///    在 tasks 锁内先写终态再触发 token，等待侧 select 两臂都在同一把
    ///    锁内复查——半观察到的 queued 记录构造上不可能跑 body）；
    /// 4. 复查见 cancelled = 排队期取消：许可原样归还、queued 计数回收、
    ///    返回取消冲突错误（body 零执行）；复查见 queued = 迁移 running 开跑。
    pub(crate) async fn begin(
        self: &Arc<Self>,
        kind: &'static str,
        course_id: &str,
        node_id: Option<&str>,
        topic: &str,
    ) -> Result<GenerationTaskGuard, AppError> {
        let key = self.pre_register(kind, course_id, node_id, topic)?;
        self.begin_registered(key).await
    }

    /// 登记（快路径，不等待准入）：同步落 `queued` 记录（gate off = 直落
    /// running，与 [`Self::begin`] 同临界区语义）+ 排队读数 + begin 观测行，
    /// 立即返回任务键。准入等待与 body 执行由调用方经 [`Self::begin_registered`]
    /// 在**任意任务生命周期**接续——e4 课时后台化即：HTTP 请求只登记（202
    /// 秒回，满闸也不等），spawn 出的脱离任务再等准入、跑完 body（断连不
    /// 取消）。同键已有 `queued|running` 记录 = Conflict（防双击双跑，与
    /// begin 同面）。
    pub(crate) fn pre_register(
        &self,
        kind: &'static str,
        course_id: &str,
        node_id: Option<&str>,
        topic: &str,
    ) -> Result<String, AppError> {
        let key = Self::key(kind, course_id, node_id);
        let token = CancellationToken::new();
        let gated = self.gate.is_some();
        {
            let mut tasks = self.tasks.lock().expect("generation task registry poisoned");
            if tasks.get(&key).is_some_and(|record| {
                record.state == STATE_RUNNING || record.state == STATE_QUEUED
            }) {
                return Err(AppError::Conflict(format!(
                    "generation task already running: {key}"
                )));
            }
            let sequence = self
                .sequence
                .fetch_add(1, AtomicOrdering::Relaxed);
            if gated {
                let queued_now = self.gate_queued.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                self.gate_queued_peak
                    .fetch_max(queued_now, AtomicOrdering::SeqCst);
            }
            tasks.insert(
                key.clone(),
                TaskRecord {
                    sequence,
                    kind,
                    course_id: course_id.to_owned(),
                    node_id: node_id.map(str::to_owned),
                    topic: topic.to_owned(),
                    state: if gated { STATE_QUEUED } else { STATE_RUNNING },
                    token: token.clone(),
                    step: None,
                    detail: None,
                    started_at: Instant::now(),
                    queued_at_ms: gated.then(now_ms),
                },
            );
        }
        self.emit_observation(serde_json::json!({
            "type": "begin",
            "key": key,
            "kind": kind,
            "topic": topic,
            "at": now_ms(),
        })
        .to_string());
        Ok(key)
    }

    /// 接续一个已登记任务（[`Self::pre_register`] 的下半程）：等闸准入 +
    /// 同锁复查终态，返回任务 guard。排队期取消 = Err（取消冲突文案，body
    /// 零执行，终态已由取消侧落账）。
    pub(crate) async fn begin_registered(
        self: &Arc<Self>,
        key: String,
    ) -> Result<GenerationTaskGuard, AppError> {
        // 任务令牌在登记时已入册（cancel 触发的就是它）：guard 持同一枚，
        // 两臂一致。
        let token = {
            let tasks = self.tasks.lock().expect("generation task registry poisoned");
            tasks
                .get(&key)
                .map(|record| record.token.clone())
                .ok_or_else(|| {
                    AppError::Internal(format!("generation task record vanished: {key}"))
                })?
        };
        // —— 过闸等待：许可到手后同锁复查终态（见 begin 注释第 3 步）。
        let permit = match self.gate.clone() {
            None => GatePermit::uncounted(self),
            Some(semaphore) => {
                let mut acquired: Option<OwnedSemaphorePermit> = None;
                loop {
                    let cancelled = token.clone();
                    tokio::select! {
                        granted = semaphore.clone().acquire_owned() => {
                            acquired = Some(
                                granted.map_err(|_| {
                                    AppError::Internal("generation task gate closed".into())
                                })?,
                            );
                        }
                        _ = cancelled.cancelled() => {}
                    }
                    // 同锁复查：这里读到什么就是什么（取消与迁移同锁落账）。
                    let terminal = {
                        let mut tasks =
                            self.tasks.lock().expect("generation task registry poisoned");
                        let Some(record) = tasks.get_mut(&key) else {
                            return Err(AppError::Internal(format!(
                                "generation task record vanished: {key}"
                            )));
                        };
                        if record.state == STATE_QUEUED {
                            if acquired.is_some() {
                                // 准入：queued → running 单一临界区（开始时刻
                                // 改写为真实开跑时点）。
                                record.state = STATE_RUNNING;
                                record.started_at = Instant::now();
                            }
                            None
                        } else {
                            Some(record.state)
                        }
                    };
                    if terminal.is_some() {
                        // 排队期取消：许可（若已到手）原样归还，body 零执行。
                        // 排队读数已由取消侧（cancel 的 queued 分支）回收——
                        // 这里只是等待者退场，不是状态迁移，不重复回收。
                        drop(acquired);
                        return Err(AppError::Conflict(CANCELLED_MESSAGE.to_owned()));
                    }
                    if acquired.is_some() {
                        // 准入迁移（queued → running）：排队读数随之回收。
                        let queued_before = self.gate_queued.fetch_sub(1, AtomicOrdering::SeqCst);
                        debug_assert!(queued_before > 0, "queued counter underflow");
                        break;
                    }
                    // token 已触发而终态未落（构造上不可达：cancel 先写终态再
                    // 触发 token，且同一把锁）——防御性重等。
                }
                GatePermit::acquire(self, acquired.expect("loop breaks only after acquire"))
            }
        };
        Ok(GenerationTaskGuard {
            registry: Arc::clone(self),
            key,
            token,
            permit,
            usage: crate::completer::LlmUsageMeter::new(),
        })
    }

    fn set_step(&self, key: &str, step: &str, detail: &str) {
        if let Ok(mut tasks) = self.tasks.lock() {
            if let Some(record) = tasks.get_mut(key) {
                if record.state == STATE_RUNNING {
                    record.step = Some(step.to_owned());
                    record.detail = Some(detail.to_owned());
                }
            }
        }
    }

    /// 终态收尾（首写即定；对已终态记录的收尾是无操作）。观测旁路发
    /// finish 行——携带相位耗时读数（e1）：`elapsed_ms` = 准入到收尾的墙钟
    /// （排队期不计），`llm_calls`/`input_chars`/`output_chars` = 任务体计数
    /// 包装的累计（缺席计数包装的任务三项为 0）。
    fn finish(
        &self,
        key: &str,
        state: &'static str,
        detail: String,
        usage: crate::completer::LlmUsageDelta,
    ) {
        let line = {
            let mut tasks = self.tasks.lock().expect("generation task registry poisoned");
            let Some(record) = tasks.get_mut(key) else {
                return;
            };
            if record.state != STATE_RUNNING {
                return;
            }
            record.state = state;
            record.detail = Some(detail);
            record.step = None;
            serde_json::json!({
                "type": "finish",
                "key": key,
                "kind": record.kind,
                "state": state,
                "at": now_ms(),
                "elapsed_ms": record.started_at.elapsed().as_millis() as u64,
                "llm_calls": usage.llm_calls,
                "input_chars": usage.input_chars,
                "output_chars": usage.output_chars,
            })
            .to_string()
        };
        self.emit_observation(line);
    }

    /// 请求取消：排队中 = 终态 cancelled + 触发 token（唤醒等待者；等待者
    /// 同锁复查见终态即返回取消错误——**body 零执行**，许可未持有、排队位随
    /// 等待 future 丢弃释放；课时任务的排队期取消安全，不涉 token 接线）；
    /// 任务在跑且可取消（growth/structure/outline；lesson 尚未接 token，见
    /// 模块注释）→ 置 cancelled 终态 + 触发 token，返回
    /// `Some(cancelled 视图状态)`；任务已终态（竞速落败）→ None。
    /// 取消胜出 = cancelled 终态；随后任务体的 finish 全部无操作。
    pub(crate) fn cancel(&self, key: &str) -> Result<Option<&'static str>, AppError> {
        let mut tasks = self.tasks.lock().expect("generation task registry poisoned");
        let Some(record) = tasks.get_mut(key) else {
            return Ok(None);
        };
        if record.state == STATE_QUEUED {
            // 排队期取消（m7 取消窗口的取消侧）：终态先落（同锁），再触发
            // token 唤醒等待者——顺序保证等待者复查时终态必已可见。
            record.state = STATE_CANCELLED;
            record.detail = Some("已取消".into());
            record.step = None;
            record.token.cancel();
            let queued_before = self.gate_queued.fetch_sub(1, AtomicOrdering::SeqCst);
            debug_assert!(queued_before > 0, "queued counter underflow");
            return Ok(Some(STATE_CANCELLED));
        }
        if record.state != STATE_RUNNING {
            return Ok(None);
        }
        if record.kind == KIND_LESSON {
            return Err(AppError::Conflict(
                "课时内容生成暂不支持远程取消：任务已后台运行（断连不中断），取消接线另议".into(),
            ));
        }
        record.state = STATE_CANCELLED;
        record.detail = Some("已取消".into());
        record.step = None;
        record.token.cancel();
        Ok(Some(STATE_CANCELLED))
    }

    /// 取消全部在跑/排队任务（legacy 全局取消端点的转发面；在跑 lesson 跳过，
    /// 排队 lesson 安全可取）。返回实际取消数。
    pub(crate) fn cancel_running(&self) -> usize {
        let keys: Vec<String> = {
            let tasks = self.tasks.lock().expect("generation task registry poisoned");
            tasks
                .iter()
                .filter(|(_, record)| {
                    (record.state == STATE_RUNNING && record.kind != KIND_LESSON)
                        || record.state == STATE_QUEUED
                })
                .map(|(key, _)| key.clone())
                .collect()
        };
        let mut cancelled = 0;
        for key in keys {
            if self.cancel(&key).ok().flatten().is_some() {
                cancelled += 1;
            }
        }
        cancelled
    }

    /// 排队面摘要（queued 计数 + 最新排队任务的 topic；generation_status 的
    /// queued 读数源）。
    pub(crate) fn queued_summary(&self) -> (usize, Option<String>) {
        let tasks = self.tasks.lock().expect("generation task registry poisoned");
        let mut rows: Vec<(u64, String)> = tasks
            .values()
            .filter(|record| record.state == STATE_QUEUED)
            .map(|record| (record.sequence, record.topic.clone()))
            .collect();
        drop(tasks);
        rows.sort_by_key(|(sequence, _)| *sequence);
        (
            rows.len(),
            rows.last().map(|(_, topic)| topic.clone()),
        )
    }

    /// 快照（活跃置顶、注册序倒序；终态随后同序）。活跃 = queued ∪ running
    /// （m7 排队可见）；queued 行带 `queued_at_ms` 与 FIFO 位次（注册序升序
    /// 中的名次，1 起）。
    pub(crate) fn snapshot(&self) -> Vec<GenerationTaskView> {
        let tasks = self.tasks.lock().expect("generation task registry poisoned");
        let mut rows: Vec<(u64, bool, TaskRecord)> = tasks
            .values()
            .map(|record| {
                let active = record.state == STATE_RUNNING || record.state == STATE_QUEUED;
                (
                    record.sequence,
                    active,
                    TaskRecord {
                        sequence: record.sequence,
                        kind: record.kind,
                        course_id: record.course_id.clone(),
                        node_id: record.node_id.clone(),
                        topic: record.topic.clone(),
                        state: record.state,
                        token: record.token.clone(),
                        step: record.step.clone(),
                        detail: record.detail.clone(),
                        started_at: record.started_at,
                        queued_at_ms: record.queued_at_ms,
                    },
                )
            })
            .collect();
        drop(tasks);
        rows.sort_by(|a, b| b.0.cmp(&a.0));
        rows.sort_by_key(|(_, active, _)| !*active);
        // 排队位次：注册序升序（= FIFO 提交序）中的名次。
        let mut queue_order: Vec<u64> = rows
            .iter()
            .filter(|(_, _, record)| record.state == STATE_QUEUED)
            .map(|(sequence, _, _)| *sequence)
            .collect();
        queue_order.sort_unstable();
        let queue_position = |sequence: u64| -> Option<u32> {
            queue_order
                .iter()
                .position(|queued| *queued == sequence)
                .map(|index| index as u32 + 1)
        };
        rows.into_iter()
            .map(|(_, _, record)| {
                let queued = record.state == STATE_QUEUED;
                GenerationTaskView {
                    key: Self::key(record.kind, &record.course_id, record.node_id.as_deref()),
                    kind: record.kind.to_owned(),
                    course_id: record.course_id.clone(),
                    node_id: record.node_id.clone(),
                    topic: record.topic.clone(),
                    state: record.state.to_owned(),
                    step: record.step.clone(),
                    detail: record.detail.clone(),
                    elapsed_secs: (record.state == STATE_RUNNING)
                        .then(|| record.started_at.elapsed().as_secs()),
                    queued_at_ms: queued.then(|| record.queued_at_ms.unwrap_or_default()),
                    queue_position: queued.then(|| queue_position(record.sequence))
                        .flatten(),
                }
            })
            .collect()
    }

    /// 最新在跑任务的（topic, elapsed_secs）——legacy 状态端点的数据源。
    pub(crate) fn running_newest(&self) -> Option<(String, u64)> {
        let tasks = self.tasks.lock().expect("generation task registry poisoned");
        tasks
            .values()
            .filter(|record| record.state == STATE_RUNNING)
            .max_by_key(|record| record.sequence)
            .map(|record| (record.topic.clone(), record.started_at.elapsed().as_secs()))
    }

    /// 指定键的在跑 token（大纲流的取消旗标视图：agent 循环经
    /// CancellableProvider 轮询）。无在跑任务 = 新令牌（未触发）。
    pub(crate) fn running_token(&self, kind: &'static str) -> CancellationToken {
        let tasks = self.tasks.lock().expect("generation task registry poisoned");
        tasks
            .values()
            .filter(|record| record.state == STATE_RUNNING && record.kind == kind)
            .max_by_key(|record| record.sequence)
            .map(|record| record.token.clone())
            .unwrap_or_default()
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// m7 闸面测试共用：显式 on 臂构造（不读环境变量，测试互不串扰）。
    fn gated_registry() -> Arc<GenerationTaskRegistry> {
        Arc::new(GenerationTaskRegistry::with_gate(true, TASK_GATE_LIMIT))
    }

    /// 测试闸：占位任务体 arrive 后停在 wait_open，等 open() 放行
    /// （learnhub 测试闸的 tokio 轮询版；current_thread 运行时友好）。
    struct TestGate {
        arrived: AtomicUsize,
        open: std::sync::atomic::AtomicBool,
    }

    impl TestGate {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                arrived: AtomicUsize::new(0),
                open: std::sync::atomic::AtomicBool::new(false),
            })
        }

        fn arrive(&self) {
            self.arrived.fetch_add(1, AtomicOrdering::SeqCst);
        }

        async fn wait_arrived(&self, target: usize, timeout: Duration) -> bool {
            let deadline = std::time::Instant::now() + timeout;
            while self.arrived.load(AtomicOrdering::SeqCst) < target {
                if std::time::Instant::now() >= deadline {
                    return false;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            true
        }

        fn open(&self) {
            self.open.store(true, AtomicOrdering::SeqCst);
        }

        async fn wait_open(&self, timeout: Duration) -> Result<(), String> {
            let deadline = std::time::Instant::now() + timeout;
            while !self.open.load(AtomicOrdering::SeqCst) {
                if std::time::Instant::now() >= deadline {
                    return Err("gate open timeout".into());
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            Ok(())
        }
    }

    /// 占满在跑空位（n 个占位任务，body 等闸放行）：返回 join 句柄组。
    /// `prefix` 区分同注册表内的多批占位（键防撞）。
    fn fill_capacity(
        registry: &Arc<GenerationTaskRegistry>,
        gate: Arc<TestGate>,
        prefix: &str,
        n: usize,
    ) -> Vec<tokio::task::JoinHandle<Result<(), AppError>>> {
        (0..n)
            .map(|i| {
                let registry = Arc::clone(registry);
                let gate = Arc::clone(&gate);
                let key = format!("{prefix}fill{i}");
                tokio::spawn(async move {
                    let guard = registry.begin(KIND_GROWTH, &key, None, "fill").await?;
                    gate.arrive();
                    gate.wait_open(Duration::from_secs(10))
                        .await
                        .map_err(AppError::Conflict)?;
                    guard.finish_done("fill done");
                    Ok(())
                })
            })
            .collect()
    }

    /// 轮询快照直到出现指定键与状态（超时 = false）。
    async fn wait_state(
        registry: &Arc<GenerationTaskRegistry>,
        key: &str,
        state: &str,
        timeout: Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if registry
                .snapshot()
                .iter()
                .any(|task| task.key == key && task.state == state)
            {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// 终态幂等：finish 之后再 finish/取消都不改写终态。
    #[tokio::test]
    async fn terminal_states_are_idempotent() {
        let registry = gated_registry();
        let task = registry
            .begin(KIND_GROWTH, "course-1", None, "t")
            .await
            .expect("first begin");
        task.finish_done("ok");
        // 终态后的取消/收尾全是无操作（竞速落败如实 done）。
        assert_eq!(registry.cancel("growth:course-1:-").unwrap(), None);
        let guard = registry
            .begin(KIND_GROWTH, "course-2", None, "t2")
            .await
            .expect("second begin on another key");
        guard.finish_failed("boom");
        assert_eq!(registry.cancel("growth:course-2:-").unwrap(), None);
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.iter().all(|task| task.state != STATE_RUNNING));
        assert!(snapshot.iter().all(|task| task.state != STATE_CANCELLED));
    }

    /// 同键并发 begin 拒绝；终态后同键新任务替换旧记录。
    #[tokio::test]
    async fn same_key_begin_is_rejected_until_terminal() {
        let registry = gated_registry();
        let first = registry.begin(KIND_STRUCTURE, "c", None, "a").await.unwrap();
        assert!(registry.begin(KIND_STRUCTURE, "c", None, "b").await.is_err());
        first.finish_done("ok");
        let second = registry
            .begin(KIND_STRUCTURE, "c", None, "b")
            .await
            .expect("terminal record is replaced");
        second.finish_cancelled();
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 1, "same-key record is replaced, not appended");
        assert_eq!(snapshot[0].state, STATE_CANCELLED);
    }

    /// 取消胜出 = cancelled 终态 + token 触发；guard 的 drop 不改写终态。
    #[tokio::test]
    async fn cancel_wins_the_race_and_fires_the_token() {
        let registry = gated_registry();
        let guard = registry.begin(KIND_GROWTH, "c", None, "t").await.unwrap();
        let token = guard.token();
        assert_eq!(registry.cancel("growth:c:-").unwrap(), Some(STATE_CANCELLED));
        assert!(token.is_cancelled(), "cancellation fires the task token");
        guard.finish_done("late finish must lose");
        assert_eq!(registry.snapshot()[0].state, STATE_CANCELLED);
    }

    /// drop 未收尾 = failed 留痕（abort/panic 的观测面兜底）。
    #[tokio::test]
    async fn dropping_a_running_guard_marks_failed() {
        let registry = gated_registry();
        {
            let _guard = registry.begin(KIND_GROWTH, "c", None, "t").await.unwrap();
        }
        assert_eq!(registry.snapshot()[0].state, STATE_FAILED);
    }

    /// 快照排序：在跑置顶、注册序倒序（同秒稳定序判据）。
    #[tokio::test]
    async fn snapshot_orders_running_first_by_registration_sequence() {
        let registry = gated_registry();
        let a = registry.begin(KIND_GROWTH, "a", None, "a").await.unwrap();
        let _b = registry.begin(KIND_GROWTH, "b", None, "b").await.unwrap();
        a.finish_done("ok");
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].key, "growth:b:-", "running task leads");
        assert_eq!(snapshot[1].key, "growth:a:-");
    }

    /// 课时任务如实拒绝远程取消（循环内 token 接线的显式差异）。
    #[tokio::test]
    async fn lesson_tasks_refuse_cancellation() {
        let registry = gated_registry();
        let _guard = registry
            .begin(KIND_LESSON, "c", Some("l"), "t")
            .await
            .unwrap();
        assert!(registry.cancel("lesson:c:l").is_err());
    }

    /// m7 并发上限：6 任务同时提交，5 跑 1 排队；排队行带 FIFO 位次与
    /// queued_at；读数面 running_peak 恰为 cap、queued_peak ≥ 1。
    #[tokio::test]
    async fn gate_caps_running_at_five_and_queues_the_rest() {
        let registry = gated_registry();
        let gate = TestGate::new();
        let fillers = fill_capacity(&registry, Arc::clone(&gate), "a", TASK_GATE_LIMIT);
        assert!(
            gate.wait_arrived(TASK_GATE_LIMIT, Duration::from_secs(5)).await,
            "占位任务应全部在跑"
        );
        let readout = registry.gate_readout();
        assert!(readout.enabled);
        assert_eq!(readout.running, TASK_GATE_LIMIT, "占位恰满在跑上限");

        // 第 6 个任务：排队等待。
        let sixth = {
            let registry = Arc::clone(&registry);
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                let guard = registry
                    .begin(KIND_GROWTH, "overflow", None, "t")
                    .await?;
                gate.arrive();
                gate.wait_open(Duration::from_secs(10))
                    .await
                    .map_err(AppError::Conflict)?;
                guard.finish_done("overflow done");
                Ok::<(), AppError>(())
            })
        };
        assert!(
            wait_state(&registry, "growth:overflow:-", STATE_QUEUED, Duration::from_secs(5)).await,
            "超编任务应停在 queued：{:?}",
            registry.snapshot()
        );
        let view = registry
            .snapshot()
            .into_iter()
            .find(|task| task.key == "growth:overflow:-")
            .expect("queued row in snapshot");
        assert_eq!(view.queue_position, Some(1), "唯一排队者位次 1");
        assert!(view.queued_at_ms.is_some(), "排队登记时刻可见");
        assert_eq!(registry.gate_readout().queued, 1);

        // 放行：排队者准入并跑完；峰值读数守上限。
        gate.open();
        for handle in fillers {
            handle.await.unwrap().unwrap();
        }
        sixth.await.unwrap().unwrap();
        let readout = registry.gate_readout();
        assert_eq!(readout.running_peak, TASK_GATE_LIMIT, "在跑峰值 = cap");
        assert!(readout.queued_peak >= 1, "排队峰值在场");
        assert_eq!(readout.running, 0, "全部跑完后在跑归零");
    }

    /// m7 排队取消：queued 期 cancel → 终态 cancelled 立即可见、body 零执行
    /// （排队取消窗口取消侧）；许可与排队位随之释放（后续满编可再准入）。
    #[tokio::test]
    async fn queued_cancel_terminates_without_running_the_body() {
        let registry = gated_registry();
        let gate = TestGate::new();
        let fillers = fill_capacity(&registry, Arc::clone(&gate), "a", TASK_GATE_LIMIT);
        assert!(gate.wait_arrived(TASK_GATE_LIMIT, Duration::from_secs(5)).await);

        let body_ran = Arc::new(AtomicUsize::new(0));
        let waiter = {
            let registry = Arc::clone(&registry);
            let gate = Arc::clone(&gate);
            let body_ran = Arc::clone(&body_ran);
            tokio::spawn(async move {
                let guard = registry.begin(KIND_GROWTH, "victim", None, "t").await?;
                body_ran.fetch_add(1, AtomicOrdering::SeqCst);
                gate.arrive();
                gate.wait_open(Duration::from_secs(10))
                    .await
                    .map_err(AppError::Conflict)?;
                guard.finish_done("victim done");
                Ok::<(), AppError>(())
            })
        };
        assert!(
            wait_state(&registry, "growth:victim:-", STATE_QUEUED, Duration::from_secs(5)).await,
            "排队者应停在 queued"
        );

        assert_eq!(
            registry.cancel("growth:victim:-").unwrap(),
            Some(STATE_CANCELLED),
            "排队取消胜出"
        );
        let view = registry
            .snapshot()
            .into_iter()
            .find(|task| task.key == "growth:victim:-")
            .expect("取消记录在册");
        assert_eq!(view.state, STATE_CANCELLED, "终态立即落账");
        assert_eq!(view.queue_position, None, "取消后不再带排队位次");
        assert_eq!(registry.gate_readout().queued, 0, "排队位随取消释放");

        gate.open();
        for handle in fillers {
            handle.await.unwrap().unwrap();
        }
        let result = waiter.await.unwrap();
        assert!(
            result
                .as_ref()
                .is_err_and(|error| error.to_string().contains(CANCELLED_MESSAGE)),
            "排队取消 = 取消错误（body 零执行）：{result:?}"
        );
        assert_eq!(body_ran.load(AtomicOrdering::SeqCst), 0, "body 零执行");

        // 容量未泄漏：满编 5 可再次全部准入。
        let gate2 = TestGate::new();
        let fillers2 = fill_capacity(&registry, Arc::clone(&gate2), "b", TASK_GATE_LIMIT);
        assert!(
            gate2.wait_arrived(TASK_GATE_LIMIT, Duration::from_secs(5)).await,
            "取消后容量应完整可用"
        );
        gate2.open();
        for handle in fillers2 {
            handle.await.unwrap().unwrap();
        }
    }

    /// 排队取消竞速压力（learnhub 同名测试移植）：N 轮「提交者提交（容量满，
    /// 必然排队）+ 主线程尽快 cancel」全交错压力。不变式：cancel 落在排队段
    /// （返回 Some）⇒ body 零执行且提交以取消错误终；cancel 落在提交前
    /// （返回 None）⇒ 任务照常排队、放行后跑完——坏实现（排队期取消丢失
    /// 窗口）会让 body 在放行潮里被准入跑掉，执行计数当场暴露。
    /// 多线程运行时：提交与取消真并发，交错窗口才真实存在。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_during_submission_never_lets_queued_body_run() {
        const ROUNDS: usize = 60;
        for round in 0..ROUNDS {
            let registry = gated_registry();
            let gate = TestGate::new();
            let fillers = fill_capacity(&registry, Arc::clone(&gate), "a", TASK_GATE_LIMIT);
            assert!(
                gate.wait_arrived(TASK_GATE_LIMIT, Duration::from_secs(5)).await,
                "第 {round} 轮：占位任务应全部在跑：{:?}",
                registry.snapshot()
            );

            let body_ran = Arc::new(AtomicUsize::new(0));
            let submitter = {
                let registry = Arc::clone(&registry);
                let gate = Arc::clone(&gate);
                let body_ran = Arc::clone(&body_ran);
                tokio::spawn(async move {
                    let guard = registry.begin(KIND_GROWTH, "race", None, "t").await?;
                    body_ran.fetch_add(1, AtomicOrdering::SeqCst);
                    gate.arrive();
                    gate.wait_open(Duration::from_secs(10))
                        .await
                        .map_err(AppError::Conflict)?;
                    guard.finish_done("race done");
                    Ok::<(), AppError>(())
                })
            };
            // 尽快取消：与提交侧全交错（注册临界区前/内/后），坏窗口只在
            // 高速交错下出现。
            let cancelled = registry.cancel("growth:race:-");
            gate.open();
            let submit_result = submitter.await.unwrap();
            match cancelled {
                Ok(Some(_)) => {
                    assert_eq!(
                        body_ran.load(AtomicOrdering::SeqCst),
                        0,
                        "第 {round} 轮：排队期取消返回 Some 而 body 执行（排队取消丢失）"
                    );
                    assert!(
                        submit_result
                            .as_ref()
                            .is_err_and(|error| error.to_string().contains(CANCELLED_MESSAGE)),
                        "第 {round} 轮：排队取消应返回取消错误：{submit_result:?}"
                    );
                }
                _ => {
                    // cancel 抢在注册之前（键不存在 = None）：任务随后照常
                    // 排队、跑完、收 done，本轮无取消语义可言。
                    submit_result.unwrap();
                    assert_eq!(body_ran.load(AtomicOrdering::SeqCst), 1);
                }
            }
            for handle in fillers {
                handle.await.unwrap().unwrap();
            }
        }
    }

    /// m7 panic 容量防漏（RAII 臂）：任务体 panic → guard 随 unwind drop，
    /// 许可归还 + failed 留痕；事发后满编 5 个并发全部准入（若泄漏一个许可
    /// 这里到点不足 5）；同键随后可重新提交。
    #[tokio::test]
    async fn body_panic_releases_the_permit_and_marks_failed() {
        let registry = gated_registry();
        let handle = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move {
                let guard = registry
                    .begin(KIND_GROWTH, "panic", None, "t")
                    .await
                    .expect("begin should acquire immediately");
                let _guard = guard;
                panic!("爆炸：任务体内部错误");
            })
        };
        let join = handle.await;
        assert!(join.is_err_and(|error| error.is_panic()), "panic 原样上抛");
        let view = registry
            .snapshot()
            .into_iter()
            .find(|task| task.key == "growth:panic:-")
            .expect("panic 任务也在册");
        assert_eq!(view.state, STATE_FAILED, "guard drop 收 failed 终态");

        // 容量未泄漏：满编并发全准入 = 在跑计数已归零。
        let gate = TestGate::new();
        let fillers = fill_capacity(&registry, Arc::clone(&gate), "a", TASK_GATE_LIMIT);
        assert!(
            gate.wait_arrived(TASK_GATE_LIMIT, Duration::from_secs(5)).await,
            "panic 后许可应归还（5 个并发全准入）：{:?}",
            registry.snapshot()
        );
        gate.open();
        for filler in fillers {
            filler.await.unwrap().unwrap();
        }
        assert_eq!(registry.gate_readout().running, 0);

        // 同键新尝试照常（记录可替换，容量可用）。
        let again = registry
            .begin(KIND_GROWTH, "panic", None, "t2")
            .await
            .expect("panic 终态后同键重新提交");
        again.finish_done("ok");
        assert_eq!(
            registry
                .snapshot()
                .into_iter()
                .find(|task| task.key == "growth:panic:-")
                .unwrap()
                .state,
            STATE_DONE
        );
    }

    /// e4 登记面拆分：`pre_register` 同步落 queued 记录（快照立即可见，满
    /// 闸也不等待——HTTP 202 秒回的地基），`begin_registered` 在脱离任务里
    /// 随后准入跑完 body。
    #[tokio::test]
    async fn pre_register_is_immediate_and_begin_registered_admits_later() {
        let registry = gated_registry();
        let gate = TestGate::new();
        let fillers = fill_capacity(&registry, Arc::clone(&gate), "a", TASK_GATE_LIMIT);
        assert!(gate.wait_arrived(TASK_GATE_LIMIT, Duration::from_secs(5)).await);

        // 满闸：pre_register 不等准入，queued 记录同步可见。
        let key = registry
            .pre_register(KIND_LESSON, "c", Some("l"), "detached")
            .expect("register must not wait for admission");
        assert_eq!(key, "lesson:c:l");
        let view = registry
            .snapshot()
            .into_iter()
            .find(|task| task.key == key)
            .expect("queued record visible immediately after pre_register");
        assert_eq!(view.state, STATE_QUEUED);

        // 同键二次登记拒绝（防双击双跑，与 begin 同面）。
        assert!(registry
            .pre_register(KIND_LESSON, "c", Some("l"), "dup")
            .is_err());

        // 脱离任务里接续：放行在跑占位 → 空位释放 → 准入迁移 running、跑完
        // 收 done（排队等待发生在任务侧，不占提交方）。
        let runner = {
            let registry = Arc::clone(&registry);
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                let guard = registry.begin_registered(key).await?;
                gate.arrive();
                gate.wait_open(Duration::from_secs(10))
                    .await
                    .map_err(AppError::Conflict)?;
                guard.finish_done("detached done");
                Ok::<(), AppError>(())
            })
        };
        gate.open();
        for handle in fillers {
            handle.await.unwrap().unwrap();
        }
        runner.await.unwrap().unwrap();
        // 注册表内的终态（queued → running → done 可能快到 RUNNING 观察不
        // 到——登记时点 queued 已在上面断过，这里只钉终态）。
        let state = registry
            .snapshot()
            .into_iter()
            .find(|task| task.key == "lesson:c:l")
            .unwrap()
            .state;
        assert_eq!(state, STATE_DONE);
    }

    /// e4 排队取消窗口在拆分面上同样闭合：登记后取消 → `begin_registered`
    /// 返回取消冲突，body 零执行（取消侧先写终态再触发 token 的不变式由
    /// pre_register/begin_registered 共享同一把锁承载）。
    #[tokio::test]
    async fn begin_registered_honors_a_queued_cancel() {
        let registry = gated_registry();
        let gate = TestGate::new();
        let fillers = fill_capacity(&registry, Arc::clone(&gate), "a", TASK_GATE_LIMIT);
        assert!(gate.wait_arrived(TASK_GATE_LIMIT, Duration::from_secs(5)).await);
        let key = registry.pre_register(KIND_LESSON, "c", None, "t").unwrap();
        assert_eq!(registry.cancel(&key).unwrap(), Some(STATE_CANCELLED));
        let result = registry.begin_registered(key).await;
        let error = match result {
            Ok(_) => panic!("queued cancel must surface as the cancel conflict"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains(CANCELLED_MESSAGE),
            "queued cancel surfaces as the cancel conflict: {error}"
        );
        gate.open();
        for handle in fillers {
            handle.await.unwrap().unwrap();
        }
    }

    /// m7 键级互斥回归：排队态同样拒绝同键并发 begin（防双击双跑面扩大到
    /// queued）。
    #[tokio::test]
    async fn same_key_rejection_covers_queued_records() {
        let registry = gated_registry();
        let gate = TestGate::new();
        let fillers = fill_capacity(&registry, Arc::clone(&gate), "a", TASK_GATE_LIMIT);
        assert!(gate.wait_arrived(TASK_GATE_LIMIT, Duration::from_secs(5)).await);

        let queued = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.begin(KIND_GROWTH, "dup", None, "a").await })
        };
        assert!(
            wait_state(&registry, "growth:dup:-", STATE_QUEUED, Duration::from_secs(5)).await,
            "排队者在册"
        );
        let second = registry.begin(KIND_GROWTH, "dup", None, "b").await;
        assert!(second.is_err(), "同键排队态同样拒绝（排队态也在互斥面）");

        registry.cancel("growth:dup:-").unwrap();
        gate.open();
        for handle in fillers {
            handle.await.unwrap().unwrap();
        }
        assert!(queued.await.unwrap().is_err(), "排队取消收尾");
    }

    /// m7 off 臂：`NOMI_LEARNING_TASK_GATE=off` = 无上限直跑（闸前行为）——
    /// 8 并发全部立即 running（超 cap 5）、零排队、读数为直跑形态。
    #[tokio::test]
    async fn off_arm_runs_unbounded_without_queue() {
        let registry = Arc::new(GenerationTaskRegistry::with_gate(false, TASK_GATE_LIMIT));
        let gate = TestGate::new();
        let fillers = fill_capacity(&registry, Arc::clone(&gate), "c", 8);
        assert!(
            gate.wait_arrived(8, Duration::from_secs(5)).await,
            "off 臂无上限：8 个占位应全部立即在跑"
        );
        let readout = registry.gate_readout();
        assert!(!readout.enabled, "off 臂闸不在场");
        assert_eq!(readout.queued, 0, "off 臂无排队");
        assert_eq!(readout.queued_peak, 0);
        assert_eq!(
            readout.running_peak, 8,
            "off 臂 running_peak 超 cap（直跑形态）"
        );
        // 快照无 queued 投影字段。
        let snapshot = registry.snapshot();
        assert!(snapshot.iter().all(|task| task.state == STATE_RUNNING));
        assert!(snapshot.iter().all(|task| task.queue_position.is_none()));
        assert!(snapshot.iter().all(|task| task.queued_at_ms.is_none()));
        gate.open();
        for handle in fillers {
            handle.await.unwrap().unwrap();
        }
    }

    /// m7 FIFO 排队：位次按注册序；放行单个空位时队头先准入（非插队）。
    #[tokio::test]
    async fn queued_tasks_report_fifo_positions_and_acquire_in_order() {
        let registry = gated_registry();
        let shared = TestGate::new();
        let mut handles = fill_capacity(&registry, Arc::clone(&shared), "s", TASK_GATE_LIMIT - 1);
        // 第五个占位用独享闸，便于单独放行一枚许可。
        let solo = TestGate::new();
        handles.push(fill_capacity(&registry, Arc::clone(&solo), "z", 1).pop().unwrap());
        assert!(shared.wait_arrived(TASK_GATE_LIMIT - 1, Duration::from_secs(5)).await);
        assert!(solo.wait_arrived(1, Duration::from_secs(5)).await);

        // 两个排队者：位次按提交序。
        let victims = TestGate::new();
        let spawn_victim = |registry: Arc<GenerationTaskRegistry>, key: &'static str| {
            let victims = Arc::clone(&victims);
            tokio::spawn(async move {
                let guard = registry.begin(KIND_GROWTH, key, None, "victim").await?;
                victims.arrive();
                victims.wait_open(Duration::from_secs(10))
                    .await
                    .map_err(AppError::Conflict)?;
                guard.finish_done("victim done");
                Ok::<(), AppError>(())
            })
        };
        let first = spawn_victim(Arc::clone(&registry), "first");
        assert!(
            wait_state(&registry, "growth:first:-", STATE_QUEUED, Duration::from_secs(5)).await,
            "first 应排队"
        );
        let second = spawn_victim(Arc::clone(&registry), "second");
        assert!(
            wait_state(&registry, "growth:second:-", STATE_QUEUED, Duration::from_secs(5)).await,
            "second 应排队"
        );
        let position_of = |key: &str| {
            registry
                .snapshot()
                .into_iter()
                .find(|task| task.key == key)
                .and_then(|task| task.queue_position)
        };
        assert_eq!(position_of("growth:first:-"), Some(1), "先提交者位次 1");
        assert_eq!(position_of("growth:second:-"), Some(2), "后提交者位次 2");

        // 放行一枚许可：队头（first）先准入，second 顶上位次 1。
        solo.open();
        assert!(
            wait_state(&registry, "growth:first:-", STATE_RUNNING, Duration::from_secs(5)).await,
            "队头应先准入：{:?}",
            registry.snapshot()
        );
        assert_eq!(position_of("growth:second:-"), Some(1));

        victims.open();
        shared.open();
        let mut all = handles;
        all.push(first);
        all.push(second);
        for handle in all {
            handle.await.unwrap().unwrap();
        }
    }

    /// CancellationCompleter：调用中取消 → 内层恰被调一次、结果丢弃。
    #[tokio::test]
    async fn cancellation_completer_discards_the_result_of_an_inflight_call() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct SlowCompleter {
            calls: AtomicUsize,
        }
        #[async_trait::async_trait]
        impl LearningCompleter for SlowCompleter {
            async fn complete(
                &self,
                _model_override: Option<(&str, &str)>,
                _system: &str,
                _user: &str,
                _max_tokens: u32,
            ) -> Result<String, AppError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                Ok("{\"late\": true}".to_owned())
            }
        }
        let inner = SlowCompleter { calls: AtomicUsize::new(0) };
        let token = CancellationToken::new();
        let cancel_later = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            cancel_later.cancel();
        });
        let wrapper = CancellationCompleter::new(&inner, token);
        let result = wrapper.complete(None, "s", "u", 8).await;
        assert!(result.is_err_and(|error| error.to_string().contains(CANCELLED_MESSAGE)));
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1, "inner ran exactly once");
    }

    /// e1 相位耗时读数：finish 行携带 `elapsed_ms` + 用量三项；显式 finish
    /// 与 drop 兜底两条收尾路径都带数；begin 行不带（读数只在收尾结算）。
    #[tokio::test]
    async fn finish_line_carries_phase_readings() {
        let registry = gated_registry();
        let lines: Arc<std::sync::Mutex<Vec<serde_json::Value>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&lines);
        registry.set_observer(Arc::new(move |line: &str| {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                sink.lock().unwrap().push(value);
            }
        }));

        let task = registry.begin(KIND_STRUCTURE, "c", None, "t").await.unwrap();
        // 任务体上报两笔增量（如两次 LLM 调用）。
        task.usage().add(crate::completer::LlmUsageDelta {
            llm_calls: 1,
            input_chars: 100,
            output_chars: 30,
        });
        task.usage().add(crate::completer::LlmUsageDelta {
            llm_calls: 1,
            input_chars: 60,
            output_chars: 20,
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
        task.finish_done("ok");

        let begin = lines
            .lock()
            .unwrap()
            .iter()
            .find(|line| line["type"] == "begin")
            .expect("begin line captured")
            .clone();
        assert!(begin.get("elapsed_ms").is_none(), "begin 行不带读数");
        let finish = lines
            .lock()
            .unwrap()
            .iter()
            .find(|line| line["type"] == "finish")
            .expect("finish line captured")
            .clone();
        assert_eq!(finish["llm_calls"], 2, "两笔增量聚合");
        assert_eq!(finish["input_chars"], 160);
        assert_eq!(finish["output_chars"], 50);
        assert!(
            finish["elapsed_ms"].as_u64().is_some_and(|ms| ms < 60_000),
            "elapsed_ms 在场且量级 sane: {finish}"
        );
    }

    /// e1 相位耗时读数（drop 兜底臂）：未收尾即 drop = failed 留痕，finish
    /// 行照带已上报的用量——中断任务的耗时读数不缺口。
    #[tokio::test]
    async fn dropped_guard_finish_line_still_carries_reported_usage() {
        let registry = gated_registry();
        let lines: Arc<std::sync::Mutex<Vec<serde_json::Value>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&lines);
        registry.set_observer(Arc::new(move |line: &str| {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                sink.lock().unwrap().push(value);
            }
        }));
        {
            let task = registry.begin(KIND_LESSON, "c", Some("l"), "t").await.unwrap();
            task.usage().add(crate::completer::LlmUsageDelta {
                llm_calls: 3,
                input_chars: 900,
                output_chars: 120,
            });
        }
        let finish = lines
            .lock()
            .unwrap()
            .iter()
            .find(|line| line["type"] == "finish")
            .expect("drop leaves the failed finish line")
            .clone();
        assert_eq!(finish["state"], STATE_FAILED);
        assert_eq!(finish["llm_calls"], 3);
        assert_eq!(finish["output_chars"], 120);
    }

    /// CountingCompleter：Ok 调用计调用数 + 双向字符量；Err 调用只计调用
    /// 数（失败调用也是花掉的延迟，但没有产出可计）。
    #[tokio::test]
    async fn counting_completer_reports_ok_volume_and_err_calls() {
        struct Stub;
        #[async_trait::async_trait]
        impl LearningCompleter for Stub {
            async fn complete(
                &self,
                _model_override: Option<(&str, &str)>,
                system: &str,
                user: &str,
                _max_tokens: u32,
            ) -> Result<String, AppError> {
                if user == "boom" {
                    return Err(AppError::BadGateway("down".into()));
                }
                Ok(format!("{system}{user}reply"))
            }
        }
        let meter = crate::completer::LlmUsageMeter::new();
        let counting = CountingCompleter::new(&Stub, &meter);
        counting.complete(None, "sys", "usr", 8).await.unwrap();
        counting.complete(None, "sys", "boom", 8).await.unwrap_err();
        let usage = meter.snapshot();
        assert_eq!(usage.llm_calls, 2, "失败调用也计入调用数");
        // Ok 调用：input = system+user 字符；output = 回包字符。Err 调用
        // 不计字符。
        assert_eq!(usage.input_chars, ("sysusr".chars().count()) as u64);
        assert_eq!(usage.output_chars, ("sysusrreply".chars().count()) as u64);
    }
}

/// 取消包装 completer（三轮 t2 真中断）：每个 LLM 调用与取消令牌
/// `select!` 竞速——调用前已取消 = 零调用；调用中被取消 = 断 HTTP（错误
/// 带取消文案）；调用完成先到 = 结果照常（竞速落败语义由循环头检查点承
/// 担）。growth/structure 两条循环的全部 completer 调用经它走。
pub(crate) struct CancellationCompleter<'a> {
    inner: &'a dyn LearningCompleter,
    token: CancellationToken,
}

/// 取消错误的固定文案（run_growth/结构生成按它识别取消终态）。
pub(crate) const CANCELLED_MESSAGE: &str = "生成已被用户取消";

impl<'a> CancellationCompleter<'a> {
    pub(crate) fn new(inner: &'a dyn LearningCompleter, token: CancellationToken) -> Self {
        Self { inner, token }
    }

    async fn race(
        &self,
        future: impl std::future::Future<Output = Result<String, AppError>>,
    ) -> Result<String, AppError> {
        tokio::select! {
            _ = self.token.cancelled() => Err(AppError::Conflict(CANCELLED_MESSAGE.to_owned())),
            result = future => result,
        }
    }
}

#[async_trait::async_trait]
impl LearningCompleter for CancellationCompleter<'_> {
    async fn complete(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        self.race(self.inner.complete(model_override, system, user, max_tokens))
            .await
    }

    async fn complete_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        self.race(
            self.inner
                .complete_with_thinking(model_override, system, user, max_tokens),
        )
        .await
    }

    async fn complete_json(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        self.race(
            self.inner
                .complete_json(model_override, system, user, max_tokens, temperature),
        )
        .await
    }

    async fn complete_json_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        self.race(
            self.inner
                .complete_json_with_thinking(model_override, system, user, max_tokens, temperature),
        )
        .await
    }
}

/// 用量计数包装 completer（e1 相位耗时读数，与 [`CancellationCompleter`]
/// 同层）：委托内层并在每次调用后上报增量——调用数恒 +1（失败调用也是
/// 花掉的延迟），输入/输出字符量只在 Ok 时计（`chars().count()` 全字符，
/// 含空白）。与取消包装的组装次序（两条循环一致）：取消包在外、计数包在
/// 内——被取消竞速丢弃的调用不上报，观测面如实反映「活着走完的调用」。
pub(crate) struct CountingCompleter<'a> {
    inner: &'a dyn LearningCompleter,
    usage: &'a crate::completer::LlmUsageMeter,
}

impl<'a> CountingCompleter<'a> {
    pub(crate) fn new(
        inner: &'a dyn LearningCompleter,
        usage: &'a crate::completer::LlmUsageMeter,
    ) -> Self {
        Self { inner, usage }
    }

    async fn count(
        &self,
        input_chars: usize,
        result: Result<String, AppError>,
    ) -> Result<String, AppError> {
        match &result {
            Ok(reply) => self.usage.add(crate::completer::LlmUsageDelta {
                llm_calls: 1,
                input_chars: input_chars as u64,
                output_chars: reply.chars().count() as u64,
            }),
            Err(_) => self.usage.add(crate::completer::LlmUsageDelta {
                llm_calls: 1,
                input_chars: 0,
                output_chars: 0,
            }),
        }
        result
    }
}

#[async_trait::async_trait]
impl LearningCompleter for CountingCompleter<'_> {
    async fn complete(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        let input = system.chars().count() + user.chars().count();
        let result = self.inner.complete(model_override, system, user, max_tokens).await;
        self.count(input, result).await
    }

    async fn complete_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        let input = system.chars().count() + user.chars().count();
        let result = self
            .inner
            .complete_with_thinking(model_override, system, user, max_tokens)
            .await;
        self.count(input, result).await
    }

    async fn complete_json(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        let input = system.chars().count() + user.chars().count();
        let result = self
            .inner
            .complete_json(model_override, system, user, max_tokens, temperature)
            .await;
        self.count(input, result).await
    }

    async fn complete_json_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        let input = system.chars().count() + user.chars().count();
        let result = self
            .inner
            .complete_json_with_thinking(model_override, system, user, max_tokens, temperature)
            .await;
        self.count(input, result).await
    }
}
