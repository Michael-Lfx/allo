//! 长任务注册表（三轮 t2）：进程内 keyed 注册 + `CancellationToken` 真取消
//! + 快照/统一取消端点的数据源 + begin/finish 观测旁路。
//!
//! - **键**：`{kind}:{course_id}:{node_id|-}`；同键并发 `begin` 拒绝
//!   （Conflict），终态记录被同键新任务替换（历史看批行/草稿，不靠内存）。
//! - **状态机**：`running → done | failed | cancelled`，终态**首写即定**
//!   （幂等）：取消先到 = cancelled，任务先完成 = 竞速落败如实 done。
//! - **排序**：注册序 `AtomicU64` 倒序（在跑置顶，同秒稳定）。
//! - **真取消**：每个任务一枚 token；任务体在检查点轮询 + 经
//!   [`CancellationCompleter`] 在 LLM 调用内 `select!` 真中断（中断 = 断
//!   HTTP；批事务原子落库 = 中断零落库）。课时内容循环尚未接 token
//!   （ADR-0004 显式差异，断开请求即中断）——对它的取消请求如实拒绝。
//! - **观测旁路**：begin/finish 两型 JSONL 行经注入的 observer 旁路落盘
//!   （非执法；observer 缺席或写失败都静默降级）。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::Instant;

use nomifun_common::AppError;
use tokio_util::sync::CancellationToken;

use crate::completer::LearningCompleter;

/// 任务类别（键的第一段；快照面的机器名，人面文案由 `topic`/`detail` 承担）。
pub(crate) const KIND_GROWTH: &str = "growth";
pub(crate) const KIND_STRUCTURE: &str = "structure";
pub(crate) const KIND_OUTLINE: &str = "outline";
pub(crate) const KIND_LESSON: &str = "lesson";

/// 任务状态（终态首写即定）。
pub(crate) const STATE_RUNNING: &str = "running";
pub(crate) const STATE_DONE: &str = "done";
pub(crate) const STATE_FAILED: &str = "failed";
pub(crate) const STATE_CANCELLED: &str = "cancelled";

/// 快照命令的单行视图（`GET /api/learning/generation-tasks`）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GenerationTaskView {
    pub key: String,
    pub kind: String,
    pub course_id: String,
    pub node_id: Option<String>,
    pub topic: String,
    /// running | done | failed | cancelled。
    pub state: String,
    /// step 机器面（英文常量；前端不造步骤文案）。
    pub step: Option<String>,
    /// detail 人面（后端单源的单句中文）。
    pub detail: Option<String>,
    pub elapsed_secs: Option<u64>,
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
}

/// 一次长任务的注册句柄：`token()` 供任务体接检查点；显式 `finish_*` 收尾
/// （终态首写即定）；未收尾即 drop（任务被 abort/panic）按 failed 留痕。
pub(crate) struct GenerationTaskGuard {
    registry: Arc<GenerationTaskRegistry>,
    key: String,
    token: CancellationToken,
}

impl GenerationTaskGuard {
    /// 任务体的取消令牌（检查点轮询 + `select!` 竞速）。
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub fn key(&self) -> &str {
        &self.key
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
        self.registry.finish(&self.key, state, detail.into());
        // 阻止 Drop 再标 failed（finish 已消费语义，这里只是双保险）。
        self.key.clear();
    }
}

impl Drop for GenerationTaskGuard {
    fn drop(&mut self) {
        if !self.key.is_empty() {
            self.registry
                .finish(&self.key.clone(), STATE_FAILED, "任务中断（未收尾即退出）".into());
        }
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
}

impl GenerationTaskRegistry {
    pub(crate) fn new() -> Self {
        Self {
            tasks: std::sync::Mutex::new(HashMap::new()),
            sequence: AtomicU64::new(1),
            observer: std::sync::Mutex::new(None),
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

    /// 登记一个长任务：同键已有 `running` 记录 = 并发拒绝（Conflict）；同键
    /// 终态记录被新任务替换。观测旁路发 begin 行。`self: &Arc<Self>`——
    /// guard 需要回引注册表（finish/Drop 收尾）。
    pub(crate) fn begin(
        self: &Arc<Self>,
        kind: &'static str,
        course_id: &str,
        node_id: Option<&str>,
        topic: &str,
    ) -> Result<GenerationTaskGuard, AppError> {
        let key = Self::key(kind, course_id, node_id);
        let mut tasks = self.tasks.lock().expect("generation task registry poisoned");
        if tasks
            .get(&key)
            .is_some_and(|record| record.state == STATE_RUNNING)
        {
            return Err(AppError::Conflict(format!(
                "generation task already running: {key}"
            )));
        }
        let sequence = self
            .sequence
            .fetch_add(1, AtomicOrdering::Relaxed);
        let token = CancellationToken::new();
        tasks.insert(
            key.clone(),
            TaskRecord {
                sequence,
                kind,
                course_id: course_id.to_owned(),
                node_id: node_id.map(str::to_owned),
                topic: topic.to_owned(),
                state: STATE_RUNNING,
                token: token.clone(),
                step: None,
                detail: None,
                started_at: Instant::now(),
            },
        );
        drop(tasks);
        self.emit_observation(serde_json::json!({
            "type": "begin",
            "key": key,
            "kind": kind,
            "topic": topic,
            "at": now_ms(),
        })
        .to_string());
        Ok(GenerationTaskGuard {
            registry: Arc::clone(self),
            key,
            token,
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
    /// finish 行。
    fn finish(&self, key: &str, state: &'static str, detail: String) {
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
            })
            .to_string()
        };
        self.emit_observation(line);
    }

    /// 请求取消：任务在跑且可取消（growth/structure/outline；lesson 尚未
    /// 接 token，见模块注释）→ 置 cancelled 终态 + 触发 token，返回
    /// `Some(cancelled 视图状态)`；任务已终态（竞速落败）→ None。
    /// 取消胜出 = cancelled 终态；随后任务体的 finish 全部无操作。
    pub(crate) fn cancel(&self, key: &str) -> Result<Option<&'static str>, AppError> {
        let mut tasks = self.tasks.lock().expect("generation task registry poisoned");
        let Some(record) = tasks.get_mut(key) else {
            return Ok(None);
        };
        if record.state != STATE_RUNNING {
            return Ok(None);
        }
        if record.kind == KIND_LESSON {
            return Err(AppError::Conflict(
                "课时内容生成暂不支持远程取消：请求断开即中断".into(),
            ));
        }
        record.state = STATE_CANCELLED;
        record.detail = Some("已取消".into());
        record.step = None;
        record.token.cancel();
        Ok(Some(STATE_CANCELLED))
    }

    /// 取消全部在跑任务（legacy 全局取消端点的转发面；lesson 跳过）。
    /// 返回实际取消数。
    pub(crate) fn cancel_running(&self) -> usize {
        let keys: Vec<String> = {
            let tasks = self.tasks.lock().expect("generation task registry poisoned");
            tasks
                .iter()
                .filter(|(_, record)| {
                    record.state == STATE_RUNNING && record.kind != KIND_LESSON
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

    /// 快照（在跑置顶、注册序倒序；终态随后同序）。
    pub(crate) fn snapshot(&self) -> Vec<GenerationTaskView> {
        let tasks = self.tasks.lock().expect("generation task registry poisoned");
        let mut rows: Vec<(u64, bool, TaskRecord)> = tasks
            .values()
            .map(|record| {
                (
                    record.sequence,
                    record.state == STATE_RUNNING,
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
                    },
                )
            })
            .collect();
        drop(tasks);
        rows.sort_by(|a, b| b.0.cmp(&a.0));
        rows.sort_by_key(|(_, running, _)| !*running);
        rows.into_iter()
            .map(|(_, _, record)| GenerationTaskView {
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

    /// 终态幂等：finish 之后再 finish/取消都不改写终态。
    #[test]
    fn terminal_states_are_idempotent() {
        let registry = Arc::new(GenerationTaskRegistry::new());
        let task = registry
            .begin(KIND_GROWTH, "course-1", None, "t")
            .expect("first begin");
        task.finish_done("ok");
        // 终态后的取消/收尾全是无操作（竞速落败如实 done）。
        assert_eq!(registry.cancel("growth:course-1:-").unwrap(), None);
        let guard = registry
            .begin(KIND_GROWTH, "course-2", None, "t2")
            .expect("second begin on another key");
        guard.finish_failed("boom");
        assert_eq!(registry.cancel("growth:course-2:-").unwrap(), None);
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.iter().all(|task| task.state != STATE_RUNNING));
        assert!(snapshot.iter().all(|task| task.state != STATE_CANCELLED));
    }

    /// 同键并发 begin 拒绝；终态后同键新任务替换旧记录。
    #[test]
    fn same_key_begin_is_rejected_until_terminal() {
        let registry = Arc::new(GenerationTaskRegistry::new());
        let first = registry.begin(KIND_STRUCTURE, "c", None, "a").unwrap();
        assert!(registry.begin(KIND_STRUCTURE, "c", None, "b").is_err());
        first.finish_done("ok");
        let second = registry
            .begin(KIND_STRUCTURE, "c", None, "b")
            .expect("terminal record is replaced");
        second.finish_cancelled();
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 1, "same-key record is replaced, not appended");
        assert_eq!(snapshot[0].state, STATE_CANCELLED);
    }

    /// 取消胜出 = cancelled 终态 + token 触发；guard 的 drop 不改写终态。
    #[test]
    fn cancel_wins_the_race_and_fires_the_token() {
        let registry = Arc::new(GenerationTaskRegistry::new());
        let guard = registry.begin(KIND_GROWTH, "c", None, "t").unwrap();
        let token = guard.token();
        assert_eq!(registry.cancel("growth:c:-").unwrap(), Some(STATE_CANCELLED));
        assert!(token.is_cancelled(), "cancellation fires the task token");
        guard.finish_done("late finish must lose");
        assert_eq!(registry.snapshot()[0].state, STATE_CANCELLED);
    }

    /// drop 未收尾 = failed 留痕（abort/panic 的观测面兜底）。
    #[test]
    fn dropping_a_running_guard_marks_failed() {
        let registry = Arc::new(GenerationTaskRegistry::new());
        {
            let _guard = registry.begin(KIND_GROWTH, "c", None, "t").unwrap();
        }
        assert_eq!(registry.snapshot()[0].state, STATE_FAILED);
    }

    /// 快照排序：在跑置顶、注册序倒序（同秒稳定序判据）。
    #[test]
    fn snapshot_orders_running_first_by_registration_sequence() {
        let registry = Arc::new(GenerationTaskRegistry::new());
        let a = registry.begin(KIND_GROWTH, "a", None, "a").unwrap();
        let _b = registry.begin(KIND_GROWTH, "b", None, "b").unwrap();
        a.finish_done("ok");
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].key, "growth:b:-", "running task leads");
        assert_eq!(snapshot[1].key, "growth:a:-");
    }

    /// 课时任务如实拒绝远程取消（循环内 token 接线的显式差异）。
    #[test]
    fn lesson_tasks_refuse_cancellation() {
        let registry = Arc::new(GenerationTaskRegistry::new());
        let _guard = registry.begin(KIND_LESSON, "c", Some("l"), "t").unwrap();
        assert!(registry.cancel("lesson:c:l").is_err());
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
