//! 冒烟瞬断注入 provider（e4）：包装真实 provider，共享计数器 > 0 时每次
//! `stream` 开流直接失败（传输瞬断形态，`ProviderError::Connection`——
//! `is_retryable`，loop_core 的开流单重试会吃掉一断），计数归零后原样透
//! 传。smoke content 档用它模拟「代理掐流两次再恢复」，钉「provider 层单
//! 次重试吸收第一断、门面重试环吸收第二断，生成仍成功」的判据。无 feature
//! 门（smoke 命令在 nomifun-app 的常规依赖面上取用）：计数器恒 0 = 纯透
//! 传，生产零装配。
//!
//! 计数器是 `Arc<AtomicUsize>` 跨包装共享：门面重试环的每次尝试都会重新解
//! 析 provider 并重跑装饰钩子——共享计数才让「总共断 N 次」跨尝试成立。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::llm::{LlmEvent, LlmRequest};
use tokio::sync::mpsc;

pub struct FlakyStreamProvider {
    inner: Arc<dyn LlmProvider>,
    /// 剩余注入次数（共享；每个包装的 `stream` 开流先原子扣减，>0 = 注入
    /// 瞬断，≤0 = 透传）。
    remaining: Arc<AtomicUsize>,
}

impl FlakyStreamProvider {
    /// 包装真实 provider；`remaining` 由调用方持有（smoke 在生成前写入注入
    /// 次数，事后读 0 = 全部注入被吸收——计数器由调用方直读，包装不再另设
    /// 读数口）。
    pub fn new(inner: Arc<dyn LlmProvider>, remaining: Arc<AtomicUsize>) -> Arc<Self> {
        Arc::new(Self { inner, remaining })
    }
}

#[async_trait::async_trait]
impl LlmProvider for FlakyStreamProvider {
    fn input_tokens_include_cache(&self) -> bool {
        self.inner.input_tokens_include_cache()
    }

    async fn stream(
        &self,
        request: &LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
        let before = self.remaining.fetch_sub(1, Ordering::SeqCst);
        if before > 0 {
            return Err(ProviderError::Connection(format!(
                "smoke 瞬断注入：连接被掐断（本轮注入后还剩 {} 次）",
                before - 1
            )));
        }
        self.inner.stream(request).await
    }
}
