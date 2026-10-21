//! 结构化输出「批拆半」驱动层（t1 新写，t3 图生长复用）：在
//! [`crate::generation::parser::parse_json_object`] 的容错解析之上包一层
//! 「整批尝试 → 解析失败批拆半递归 → 单文件兜底」的驱动。
//!
//! 契约：
//! - 调用层失败（[`StructuredBatchError::Call`]，补全器报错/网络断）拆半
//!   无意义：整片条目按调用失败交给调用方兜底（t1 分诊的规则降级路径）。
//! - 解析/校验失败（[`StructuredBatchError::Parse`]）→ 批对半拆分递归。
//! - 部分覆盖（回包只覆盖了请求清单的一部分）→ 已覆盖条目直接收下，只对
//!   缺失子集递归（截断自救救回的前缀不浪费）。
//! - 单文件兜底仍失败 → 该条目以 [`StructuredBatchFailure`] 形态返回，由
//!   调用方决定语义（t1 分诊 = uncertain + 理由，绝不丢条目）。
//! - 输出保持输入条目序，每个条目恰有一条结果（总数守恒）。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// 一次批尝试的失败形态。
#[derive(Debug, Clone)]
pub(crate) enum StructuredBatchError {
    /// 调用层失败（补全器错误/网络）：换小批重试同样会失败，调用方兜底。
    Call(String),
    /// 解析/校验失败（截断、坏 JSON、契约不合法）：拆半递归。
    Parse(String),
}

/// 单条目最终失败形态（单文件兜底之后仍然失败）。
#[derive(Debug, Clone)]
pub(crate) enum StructuredBatchFailure {
    Call(String),
    Parse(String),
}

/// 一次批尝试的接缝：给定条目 id 列表，产出「id → 结果」映射（调用方负责
/// 提示词、补全调用与解析校验；未覆盖的 id 由驱动层补递归）。
pub(crate) trait BatchAttempt<T>: Send + Sync {
    fn attempt<'a>(
        &'a self,
        slice: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<HashMap<String, T>, StructuredBatchError>> + Send + 'a>>;
}

/// 驱动入口：对整批条目跑「尝试 → 拆半 → 单文件兜底」，返回与输入同序的
/// 逐条目结果。
pub(crate) async fn structured_batch<T: Send>(
    ids: &[String],
    attempt: &dyn BatchAttempt<T>,
) -> Vec<(String, Result<T, StructuredBatchFailure>)> {
    let mut outcomes: HashMap<String, Result<T, StructuredBatchFailure>> = HashMap::new();
    resolve(attempt, ids, &mut outcomes).await;
    ids.iter()
        .map(|id| {
            let outcome = outcomes.remove(id).unwrap_or_else(|| {
                Err(StructuredBatchFailure::Parse(
                    "驱动器未覆盖该条目（内部不变量被破坏）".to_owned(),
                ))
            });
            (id.clone(), outcome)
        })
        .collect()
}

/// 递归核心。终止性：拆半严格变小；部分覆盖的缺失子集严格变小（至少一个
/// 条目已被覆盖）；单条目是叶子。
fn resolve<'a, T: Send>(
    attempt: &'a dyn BatchAttempt<T>,
    slice: &'a [String],
    outcomes: &'a mut HashMap<String, Result<T, StructuredBatchFailure>>,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        if slice.len() == 1 {
            let id = slice[0].clone();
            let outcome = match attempt.attempt(slice).await {
                Ok(mut map) => match map.remove(&id) {
                    Some(value) => Ok(value),
                    None => Err(StructuredBatchFailure::Parse(
                        "模型未返回该条目的判定".to_owned(),
                    )),
                },
                Err(StructuredBatchError::Call(message)) => {
                    Err(StructuredBatchFailure::Call(message))
                }
                Err(StructuredBatchError::Parse(message)) => {
                    Err(StructuredBatchFailure::Parse(message))
                }
            };
            outcomes.insert(id, outcome);
            return;
        }
        match attempt.attempt(slice).await {
            Err(StructuredBatchError::Call(message)) => {
                for id in slice {
                    outcomes.insert(
                        id.clone(),
                        Err(StructuredBatchFailure::Call(message.clone())),
                    );
                }
            }
            Err(StructuredBatchError::Parse(_)) => {
                split_and_recurse(attempt, slice, outcomes).await
            }
            Ok(covered) => {
                // 只收请求过的条目（幻觉路径忽略），已覆盖的先进结果。
                for (id, value) in covered {
                    if slice.contains(&id) {
                        outcomes.insert(id, Ok(value));
                    }
                }
                let missing: Vec<String> = slice
                    .iter()
                    .filter(|id| !outcomes.contains_key(*id))
                    .cloned()
                    .collect();
                if missing.is_empty() {
                    return;
                }
                if missing.len() == slice.len() {
                    // 整批零覆盖：等价于解析失败，拆半（否则会在同大小批上打转）。
                    split_and_recurse(attempt, slice, outcomes).await;
                } else {
                    resolve(attempt, &missing, outcomes).await;
                }
            }
        }
    })
}

fn split_and_recurse<'a, T: Send>(
    attempt: &'a dyn BatchAttempt<T>,
    slice: &'a [String],
    outcomes: &'a mut HashMap<String, Result<T, StructuredBatchFailure>>,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        let mid = slice.len() / 2;
        let (left, right) = slice.split_at(mid);
        resolve(attempt, left, outcomes).await;
        resolve(attempt, right, outcomes).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn ids(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("f{index}.md")).collect()
    }

    /// 记录每次尝试的批次大小，并按策略回包的假尝试器。
    struct FakeAttempt {
        sizes: Mutex<Vec<usize>>,
        policy: fn(usize) -> Result<(), String>,
    }

    impl BatchAttempt<u32> for FakeAttempt {
        fn attempt<'a>(
            &'a self,
            slice: &'a [String],
        ) -> Pin<Box<dyn Future<Output = Result<HashMap<String, u32>, StructuredBatchError>> + Send + 'a>>
        {
            Box::pin(async move {
                self.sizes.lock().unwrap().push(slice.len());
                match (self.policy)(slice.len()) {
                    Ok(()) => Ok(slice
                        .iter()
                        .enumerate()
                        .map(|(index, id)| (id.clone(), index as u32))
                        .collect()),
                    Err(message) => Err(StructuredBatchError::Parse(message)),
                }
            })
        }
    }

    #[tokio::test]
    async fn covers_every_id_in_input_order_on_a_clean_batch() {
        let attempt = FakeAttempt {
            sizes: Mutex::new(Vec::new()),
            policy: |_| Ok(()),
        };
        let results = structured_batch(&ids(3), &attempt).await;
        assert_eq!(
            results
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            ["f0.md", "f1.md", "f2.md"],
            "输出保持输入序"
        );
        assert!(results.iter().all(|(_, result)| result.is_ok()));
        assert_eq!(*attempt.sizes.lock().unwrap(), vec![3], "整批一次成功");
    }

    #[tokio::test]
    async fn parse_failures_split_the_batch_in_half_until_success() {
        // 批大小 > 2 时解析失败，≤ 2 时成功：8 → 4/4 → 2/2×4。
        let attempt = FakeAttempt {
            sizes: Mutex::new(Vec::new()),
            policy: |size| {
                if size > 2 {
                    Err("truncated".into())
                } else {
                    Ok(())
                }
            },
        };
        let results = structured_batch(&ids(8), &attempt).await;
        assert_eq!(results.len(), 8, "拆半后仍覆盖全部条目");
        assert!(
            results.iter().all(|(_, result)| result.is_ok()),
            "小批最终全部成功"
        );
        assert_eq!(
            *attempt.sizes.lock().unwrap(),
            vec![8, 4, 2, 2, 4, 2, 2],
            "整批 → 左半递归到底 → 右半递归到底"
        );
    }

    struct CallFailAttempt {
        calls: AtomicUsize,
    }

    impl BatchAttempt<u32> for CallFailAttempt {
        fn attempt<'a>(
            &'a self,
            _slice: &'a [String],
        ) -> Pin<Box<dyn Future<Output = Result<HashMap<String, u32>, StructuredBatchError>> + Send + 'a>>
        {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Err(StructuredBatchError::Call("provider down".into()))
            })
        }
    }

    #[tokio::test]
    async fn call_failures_do_not_split_the_batch() {
        let attempt = CallFailAttempt {
            calls: AtomicUsize::new(0),
        };
        let results = structured_batch(&ids(40), &attempt).await;
        assert_eq!(results.len(), 40);
        assert!(
            results.iter().all(|(_, result)| matches!(
                result,
                Err(StructuredBatchFailure::Call(message)) if message == "provider down"
            ))
        );
        assert_eq!(
            attempt.calls.load(Ordering::SeqCst),
            1,
            "调用层失败拆半无意义：只试一次"
        );
    }

    /// 首次调用漏掉一个条目（截断自救救回前缀），之后全覆盖。
    struct PartialAttempt {
        sizes: Mutex<Vec<usize>>,
        calls: AtomicUsize,
    }

    impl BatchAttempt<u32> for PartialAttempt {
        fn attempt<'a>(
            &'a self,
            slice: &'a [String],
        ) -> Pin<Box<dyn Future<Output = Result<HashMap<String, u32>, StructuredBatchError>> + Send + 'a>>
        {
            Box::pin(async move {
                self.sizes.lock().unwrap().push(slice.len());
                let calls = self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(slice
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !(calls == 0 && *index == 1))
                    .map(|(index, id)| (id.clone(), index as u32))
                    .collect())
            })
        }
    }

    #[tokio::test]
    async fn partial_coverage_retries_only_the_missing_ids() {
        let attempt = PartialAttempt {
            sizes: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
        };
        let results = structured_batch(&ids(4), &attempt).await;
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|(_, result)| result.is_ok()));
        assert_eq!(
            *attempt.sizes.lock().unwrap(),
            vec![4, 1],
            "已覆盖的不重试，只补缺失的那个条目"
        );
    }

    #[tokio::test]
    async fn single_file_parse_failure_surfaces_as_a_parse_failure() {
        let attempt = FakeAttempt {
            sizes: Mutex::new(Vec::new()),
            policy: |_| Err("cut mid-string".into()),
        };
        let results = structured_batch(&ids(1), &attempt).await;
        assert_eq!(results.len(), 1, "单文件失败也不丢条目");
        match &results[0].1 {
            Err(StructuredBatchFailure::Parse(message)) => {
                assert!(message.contains("cut mid-string"), "{message}");
            }
            other => panic!("expected parse failure, got {other:?}"),
        }
    }
}
