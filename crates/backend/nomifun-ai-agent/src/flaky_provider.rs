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
        // 只在仍有剩余注入时扣减：无条件 `fetch_sub` 会在归零后回绕成
        // usize::MAX，从此每次开流都被误掐、注入永不恢复（真机 f3 验收
        // 实录：`还剩 18446744073709551611 次`，门面预算三试耗尽）。
        let mut observed = self.remaining.load(Ordering::SeqCst);
        loop {
            if observed == 0 {
                break;
            }
            match self.remaining.compare_exchange_weak(
                observed,
                observed - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Err(ProviderError::Connection(format!(
                        "smoke 瞬断注入：连接被掐断（本轮注入后还剩 {} 次）",
                        observed - 1
                    )));
                }
                Err(actual) => observed = actual,
            }
        }
        self.inner.stream(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 归零后必须恢复透传：多余开流不得回绕计数器、不得再掐（真机回归钉）。
    #[tokio::test]
    async fn stops_cutting_once_counter_hits_zero() {
        use std::sync::atomic::AtomicUsize;

        struct NoopProvider;
        #[async_trait::async_trait]
        impl LlmProvider for NoopProvider {
            fn input_tokens_include_cache(&self) -> bool {
                false
            }
            async fn stream(
                &self,
                _request: &LlmRequest,
            ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
                let (tx, rx) = mpsc::channel(1);
                drop(tx);
                Ok(rx)
            }
        }

        let remaining = Arc::new(AtomicUsize::new(2));
        let provider = FlakyStreamProvider::new(Arc::new(NoopProvider), remaining.clone());
        let request = LlmRequest::default();
        let mut cuts = 0;
        for _ in 0..6 {
            if provider.stream(&request).await.is_err() {
                cuts += 1;
            }
        }
        assert_eq!(cuts, 2, "恰好掐断注入的 2 次");
        assert_eq!(remaining.load(Ordering::SeqCst), 0, "计数归零且不回绕");
    }
}
