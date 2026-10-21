//! XP 结算纯函数包（j05：三轮 t1，参考 learnhub `engine/questions/settle.rs`）。
//!
//! 零 IO：输入作答事实，输出一笔 [`Settlement`]；落账（`learning_xp_events`
//! 追加只增）与 companion 桥都是调用方的事。写侧条款 [`validate_settlement`]
//! 是全部形态的唯一判官——条款先入（检测线载体随 g4 后置，但 quiz 恒非负的
//! 条款现在就位，防将来带病上线）。
//!
//! # 量纲宣言（二轮 g1）
//!
//! 1 XP = 1 est 分钟 = 1 分钟专注学习。教新基数 = est 分钟；复习基数 =
//! Σ 完成题数 × 题型当量、下限 [`REVIEW_MIN_MINUTES`]。XP 记努力不记掌握
//! （进度 = 结算数，两把尺严格分离）。
//!
//! # 分级授予（learnhub award 阶梯）
//!
//! 全对 = 基数 + [`XP_FULL_BONUS`] 奖励；部分 = `(基数 × ratio).floor()`；
//! 砍短 = 恰 0；乱猜（[`is_guessing`]：n ≥ [`GUESS_MIN_QUESTIONS`] 且
//! 实际正确率 < [`GUESS_SIGNIFICANCE`] × [`GUESS_TEXT_BASELINE`]）= 负分
//! `−max(基数 ÷ [`GUESS_PENALTY_DIVISOR`], 1)`。起步值全部自标「待校准」
//! ——learnhub 同款；乱猜判据是报警阈值，可调不改结构。
//!
//! # quiz 准确率分级（q1，learnhub `settle_quiz` 移植）
//!
//! quiz 是自动性检测（测量非门禁）：基数 = Σ 题型当量（**无
//! [`REVIEW_MIN_MINUTES`] 下限**——quiz 线与 node/review 阶梯的关键区别，
//! 单题迷你检测也按当量原样入账）；授予按准确率四档：≥ [`QUIZ_EXCELLENT_RATIO`]
//! = 足额 + 全对级 bonus / ≥ [`QUIZ_PASS_RATIO`] = 足额 / ≥ [`QUIZ_HALF_RATIO`]
//! = 半额 / 低于此值 = 四分之一额。**恒非负**——无乱猜通道（惩罚性反馈归
//! 归因面，条款 j05 先入：quiz 走 [`validate_settlement`] 零放宽）；零题
//! 拒绝（测量语义：无准确率可分级）。触发面（K 计数）是账本回放派生，
//! 见 `service::xp`；[`QUIZ_TRIGGER_SETTLEMENTS`] 是起步值待校准。

use crate::models::ActivityKind;

/// 全对奖励（full 形态在基数之上的 bonus 分量）。
pub const XP_FULL_BONUS: i64 = 2;
/// 复习基数下限（分钟当量）——单卡作答的工作量地板。
pub const REVIEW_MIN_MINUTES: i64 = 3;
/// 乱猜判据：作答题数下限（少于该数不作乱猜判定——宁可漏判不可误伤）。
pub const GUESS_MIN_QUESTIONS: i64 = 4;
/// 乱猜判据：实际正确率显著低于基线的倍数阈值。
pub const GUESS_SIGNIFICANCE: f64 = 0.5;
/// 乱猜罚金分母：负分 = −max(基数 ÷ 2, 1)。
pub const GUESS_PENALTY_DIVISOR: i64 = 2;
/// 乱猜基线：文本作答的保守随机正确率上界（宁可漏判不可误伤）。
pub const GUESS_TEXT_BASELINE: f64 = 0.25;
/// 日 XP 目标缺省值（≈ MA 课堂基线）；住应用设置 `learning.dailyXpGoal`，
/// 写侧拒绝 1..=1000 之外的值。
pub const DEFAULT_DAILY_XP_GOAL: i64 = 40;

/// quiz 触发阈值（K 起步值待校准；MA 150 XP ≈ est 中位 × 6 同量级）。
/// 计数面 = 距该课程上次 quiz 结算之后的 `node-completed` 数（复习/quiz
/// 账不推进），见 `service::xp`。
pub const QUIZ_TRIGGER_SETTLEMENTS: i64 = 6;
/// 单场 quiz 题量上限（≈ 题量与成本护栏；贪心装填随 q2 载体）。
pub const QUIZ_QUESTION_BUDGET: usize = 15;
/// quiz 优秀档门槛：准确率 ≥ 此值 = 足额 + 全对级 bonus（奖励「干净自动」）。
pub const QUIZ_EXCELLENT_RATIO: f64 = 0.9;
/// quiz 达标档门槛：准确率 ≥ 此值 = 足额（不奖不罚）。
pub const QUIZ_PASS_RATIO: f64 = 0.6;
/// quiz 衰减档门槛：准确率 ≥ 此值 = 半额；低于此值 = 四分之一额。
pub const QUIZ_HALF_RATIO: f64 = 0.3;

/// 题型工作量当量（分钟/题，起步值待校准；learnhub 1/2/3 骨架按认知负荷
/// 分档）：单选/判断 = 1；多选/填空/数值/匹配/排序 = 2（构造完整答案集合
/// 的作答与填空同级）；简答/开放/AI 批改 = 3。
pub const fn question_equivalent(kind: ActivityKind) -> i64 {
    match kind {
        ActivityKind::SingleChoice | ActivityKind::TrueFalse => 1,
        ActivityKind::MultiChoice
        | ActivityKind::FillInBlank
        | ActivityKind::Numeric
        | ActivityKind::Matching
        | ActivityKind::Ordering => 2,
        ActivityKind::Reflection | ActivityKind::OpenQuestion => 3,
    }
}

/// 结算形态（`learning_xp_events.basis`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementBasis {
    /// 全对：基数 + 全对奖励，无 ratio。
    Full,
    /// 部分：`(基数 × ratio).floor()`，必带 ratio ∈ [0, 1]，无 bonus。
    Partial,
    /// 砍短：未完成/中途放弃，恰 0。
    Trimmed,
    /// 乱猜：负分 −max(基数 ÷ 2, 1)。
    GuessPenalty,
}

impl SettlementBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::Trimmed => "trimmed",
            Self::GuessPenalty => "guess-penalty",
        }
    }
}

/// 一笔 XP 结算（写入 `learning_xp_events` 前的纯值）。`earned_xp` 是净入账
/// （full 时含 `xp_bonus` 分量）。
#[derive(Debug, Clone, PartialEq)]
pub struct Settlement {
    pub earned_xp: i64,
    pub xp_bonus: i64,
    pub basis: SettlementBasis,
    /// 仅 [`SettlementBasis::Partial`] 携带，∈ [0, 1]。
    pub ratio: Option<f64>,
}

impl Settlement {
    /// 教新线（node-completed）：基数 = est 分钟，按作答事实走 award 阶梯。
    ///
    /// `total_questions` = 课时客观题总数；`answered`/`passed` = 有作答/有
    /// 通过作答的题数（distinct activity 口径）。授予比例 = passed/total
    /// （未作答即未完成该步，拉低比例）；乱猜的实际正确率 = passed/answered
    /// （对已作答题的判据，基线是单题随机率）。
    pub fn settle_node(base_est: i64, total_questions: i64, answered: i64, passed: i64) -> Self {
        let base = base_est.max(0);
        // 乱猜判据先行：判据命中即负分，覆盖其余形态。
        if is_guessing(answered, if answered > 0 { passed as f64 / answered as f64 } else { 0.0 }) {
            return Self {
                earned_xp: -guess_penalty(base),
                xp_bonus: 0,
                basis: SettlementBasis::GuessPenalty,
                ratio: None,
            };
        }
        if total_questions <= 0 {
            // 无练习课时：完成步骤即全额（无正确率可奖，bonus 缺席）。
            return Self {
                earned_xp: base,
                xp_bonus: 0,
                basis: SettlementBasis::Full,
                ratio: None,
            };
        }
        let ratio = (passed as f64 / total_questions as f64).clamp(0.0, 1.0);
        if passed >= total_questions {
            Self {
                earned_xp: base + XP_FULL_BONUS,
                xp_bonus: XP_FULL_BONUS,
                basis: SettlementBasis::Full,
                ratio: None,
            }
        } else if ratio <= 0.0 {
            // 完成而零通过：与砍短同价（步骤走完、练习全空）。
            Self {
                earned_xp: 0,
                xp_bonus: 0,
                basis: SettlementBasis::Trimmed,
                ratio: None,
            }
        } else {
            Self {
                earned_xp: ((base as f64) * ratio).floor() as i64,
                xp_bonus: 0,
                basis: SettlementBasis::Partial,
                ratio: Some(ratio),
            }
        }
    }

    /// 复习线（review-completed）：一次真实推进的结算。`score` ∈ [0, 1]
    /// （客观题 0/1；承认遗忘按 0）。基数 = max(题型当量,
    /// [`REVIEW_MIN_MINUTES`])。
    pub fn settle_review(kind: ActivityKind, score: f64) -> Self {
        let base = question_equivalent(kind).max(REVIEW_MIN_MINUTES);
        let score = score.clamp(0.0, 1.0);
        if score >= 1.0 {
            Self {
                earned_xp: base + XP_FULL_BONUS,
                xp_bonus: XP_FULL_BONUS,
                basis: SettlementBasis::Full,
                ratio: None,
            }
        } else if score <= 0.0 {
            Self {
                earned_xp: 0,
                xp_bonus: 0,
                basis: SettlementBasis::Partial,
                ratio: Some(0.0),
            }
        } else {
            Self {
                earned_xp: ((base as f64) * score).floor() as i64,
                xp_bonus: 0,
                basis: SettlementBasis::Partial,
                ratio: Some(score),
            }
        }
    }

    /// quiz 线结算（quiz-completed，q1：learnhub `settle_quiz` 移植）：基数
    /// = Σ 题型当量（**无** [`REVIEW_MIN_MINUTES`] 下限），授予按准确率四档
    /// 分级（见模块文档；恒非负——无乱猜通道）。`kinds` = 本场 quiz 作答题
    /// 的题型清单（定价按题型），`answered` = 作答题数，`correct` = 答对数。
    /// quiz 必作答（测量语义）：零题 / 题单与作答数错配 / 答对数越界 =
    /// 输入责任 fail loud（错配即错价，不做静默宽容）。
    pub fn settle_quiz(kinds: &[ActivityKind], correct: i64, answered: i64) -> Result<Self, String> {
        if answered <= 0 || kinds.is_empty() {
            return Err("quiz 会话必作答（零题 quiz 无准确率可分级）".into());
        }
        if kinds.len() as i64 != answered {
            return Err(format!(
                "quiz 结算题单与作答数不符（题单 {} / 作答 {answered}；定价按题型，错配即错价）",
                kinds.len()
            ));
        }
        if !(0..=answered).contains(&correct) {
            return Err(format!("quiz 答对数越界（应 0..={answered}）：{correct}"));
        }
        // quiz 无下限：worked 原样作基数（与复习线的 max(当量, 3) 相区别）。
        let worked: i64 = kinds.iter().map(|kind| question_equivalent(*kind)).sum();
        let accuracy = correct as f64 / answered as f64;
        // 准确率四档（门槛常量集中，见模块文档）：优秀档足额 + bonus /
        // 达标档足额 / 衰减档半额 / 低档四分之一额——恒非负。
        let settlement = if accuracy >= QUIZ_EXCELLENT_RATIO {
            Self {
                earned_xp: worked + XP_FULL_BONUS,
                xp_bonus: XP_FULL_BONUS,
                basis: SettlementBasis::Full,
                ratio: None,
            }
        } else if accuracy >= QUIZ_PASS_RATIO {
            Self {
                earned_xp: worked,
                xp_bonus: 0,
                basis: SettlementBasis::Full,
                ratio: None,
            }
        } else if accuracy >= QUIZ_HALF_RATIO {
            Self {
                earned_xp: worked / 2,
                xp_bonus: 0,
                basis: SettlementBasis::Partial,
                ratio: Some(accuracy),
            }
        } else {
            Self {
                earned_xp: worked / 4,
                xp_bonus: 0,
                basis: SettlementBasis::Partial,
                ratio: Some(accuracy),
            }
        };
        Ok(settlement)
    }
}

/// 写侧条款（learnhub `infra/ledger/event.rs` 同款，四形态 + quiz 恒非负）。
/// 违约返回可读错误——落账侧必须先过这里再写表（表级 CHECK 只是地板）。
pub fn validate_settlement(kind: &str, settlement: &Settlement) -> Result<(), String> {
    let ok = |condition: bool, message: String| if condition { Ok(()) } else { Err(message) };
    match settlement.basis {
        SettlementBasis::Full => {
            ok(
                settlement.earned_xp >= 0,
                "full settlement must be non-negative".into(),
            )?;
            ok(
                settlement.ratio.is_none(),
                "full settlement must not carry a ratio".into(),
            )?;
            ok(
                settlement.xp_bonus >= 0 && settlement.earned_xp >= settlement.xp_bonus,
                "full settlement must include its bonus component (earned_xp >= xp_bonus)"
                    .into(),
            )
        }
        SettlementBasis::Partial => {
            let ratio = settlement.ratio.ok_or_else(|| {
                "partial settlement must carry its ratio".to_string()
            })?;
            ok(
                (0.0..=1.0).contains(&ratio),
                format!("partial ratio must be in [0,1], got {ratio}"),
            )?;
            ok(
                settlement.earned_xp >= 0,
                "partial settlement must be non-negative".into(),
            )?;
            ok(settlement.xp_bonus == 0, "bonus is full-only".into())
        }
        SettlementBasis::Trimmed => ok(
            settlement.earned_xp == 0 && settlement.xp_bonus == 0 && settlement.ratio.is_none(),
            "trimmed settlement is exactly 0 (no bonus, no ratio)".into(),
        ),
        SettlementBasis::GuessPenalty => ok(
            settlement.earned_xp < 0 && settlement.xp_bonus == 0 && settlement.ratio.is_none(),
            "guess-penalty settlement must be strictly negative (no bonus, no ratio)".into(),
        ),
    }?;
    // 检测线条款先入：quiz 结算恒非负（测量非门禁，无惩罚通道）。
    ok(
        kind != "quiz-completed" || settlement.earned_xp >= 0,
        "quiz settlements are never negative (measurement, not a gate)".into(),
    )
}

/// 乱猜判据：`n ≥ GUESS_MIN_QUESTIONS && actual < GUESS_SIGNIFICANCE ×
/// GUESS_TEXT_BASELINE`。n 不足直接不判（宁可漏判不可误伤）。
pub fn is_guessing(answered: i64, accuracy: f64) -> bool {
    answered >= GUESS_MIN_QUESTIONS
        && accuracy < GUESS_SIGNIFICANCE * GUESS_TEXT_BASELINE
}

/// 乱猜罚金：−max(基数 ÷ GUESS_PENALTY_DIVISOR, 1) 的绝对值。
fn guess_penalty(base: i64) -> i64 {
    (base / GUESS_PENALTY_DIVISOR).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    // —— 写侧条款红绿样（learnhub 口径） ——

    #[test]
    fn clause_accepts_all_four_well_formed_shapes() {
        let cases = [
            ("node-completed", Settlement { earned_xp: 27, xp_bonus: 2, basis: SettlementBasis::Full, ratio: None }),
            ("node-completed", Settlement { earned_xp: 12, xp_bonus: 0, basis: SettlementBasis::Partial, ratio: Some(0.5) }),
            ("node-completed", Settlement { earned_xp: 0, xp_bonus: 0, basis: SettlementBasis::Trimmed, ratio: None }),
            ("node-completed", Settlement { earned_xp: -13, xp_bonus: 0, basis: SettlementBasis::GuessPenalty, ratio: None }),
            // quiz 恒非负：非负形态与 0 值都合法。
            ("quiz-completed", Settlement { earned_xp: 8, xp_bonus: 2, basis: SettlementBasis::Full, ratio: None }),
            ("quiz-completed", Settlement { earned_xp: 0, xp_bonus: 0, basis: SettlementBasis::Trimmed, ratio: None }),
        ];
        for (kind, settlement) in cases {
            assert!(
                validate_settlement(kind, &settlement).is_ok(),
                "well-formed {kind}/{settlement:?} must pass"
            );
        }
    }

    #[test]
    fn clause_rejects_violations() {
        let cases: [( &str, Settlement, &str); 10] = [
            (
                "node-completed",
                Settlement { earned_xp: -1, xp_bonus: 0, basis: SettlementBasis::Full, ratio: None },
                "full 负值",
            ),
            (
                "node-completed",
                Settlement { earned_xp: 5, xp_bonus: 0, basis: SettlementBasis::Full, ratio: Some(0.5) },
                "full 不带 ratio",
            ),
            (
                "node-completed",
                Settlement { earned_xp: 1, xp_bonus: 2, basis: SettlementBasis::Full, ratio: None },
                "full 必须含 bonus 分量（earned ≥ bonus）",
            ),
            (
                "node-completed",
                Settlement { earned_xp: 1, xp_bonus: 0, basis: SettlementBasis::Partial, ratio: None },
                "partial 必带 ratio",
            ),
            (
                "node-completed",
                Settlement { earned_xp: 1, xp_bonus: 0, basis: SettlementBasis::Partial, ratio: Some(1.5) },
                "ratio 越界",
            ),
            (
                "node-completed",
                Settlement { earned_xp: -1, xp_bonus: 0, basis: SettlementBasis::Partial, ratio: Some(0.5) },
                "partial 负值",
            ),
            (
                "node-completed",
                Settlement { earned_xp: 3, xp_bonus: 0, basis: SettlementBasis::Trimmed, ratio: None },
                "trimmed 恰 0",
            ),
            (
                "node-completed",
                Settlement { earned_xp: 5, xp_bonus: 0, basis: SettlementBasis::GuessPenalty, ratio: None },
                "guess-penalty 必为负",
            ),
            (
                "node-completed",
                Settlement { earned_xp: 5, xp_bonus: 2, basis: SettlementBasis::Partial, ratio: Some(0.5) },
                "bonus 仅 full",
            ),
            (
                "quiz-completed",
                Settlement { earned_xp: -5, xp_bonus: 0, basis: SettlementBasis::GuessPenalty, ratio: None },
                "quiz 恒非负（负的 quiz 落账拒绝）",
            ),
        ];
        for (kind, settlement, why) in cases {
            assert!(
                validate_settlement(kind, &settlement).is_err(),
                "violation must be rejected: {why}"
            );
        }
    }

    // —— 分级授予阶梯 ——

    #[test]
    fn node_ladder_full_partial_trimmed_guess() {
        // 全对：基数 + bonus 2。
        assert_eq!(
            Settlement::settle_node(25, 4, 4, 4),
            Settlement { earned_xp: 27, xp_bonus: 2, basis: SettlementBasis::Full, ratio: None }
        );
        // 部分：(25 × 0.5).floor()，带 ratio。
        assert_eq!(
            Settlement::settle_node(25, 4, 4, 2),
            Settlement { earned_xp: 12, xp_bonus: 0, basis: SettlementBasis::Partial, ratio: Some(0.5) }
        );
        // 零通过且达乱猜题数：乱猜先行（−12），砍短不适用。
        assert_eq!(
            Settlement::settle_node(25, 4, 4, 0),
            Settlement { earned_xp: -12, xp_bonus: 0, basis: SettlementBasis::GuessPenalty, ratio: None }
        );
        // 未作答即未完成：比例被拉低（2/6 过 = partial 8）。
        assert_eq!(
            Settlement::settle_node(25, 6, 2, 2),
            Settlement { earned_xp: 8, xp_bonus: 0, basis: SettlementBasis::Partial, ratio: Some(2.0 / 6.0) }
        );
        // 无练习课时：full 全额、无 bonus。
        assert_eq!(
            Settlement::settle_node(25, 0, 0, 0),
            Settlement { earned_xp: 25, xp_bonus: 0, basis: SettlementBasis::Full, ratio: None }
        );
        // 总数 3、零通过：n=3 不判乱猜 → 按阶梯砍短 0。
        assert_eq!(
            Settlement::settle_node(25, 3, 3, 0),
            Settlement { earned_xp: 0, xp_bonus: 0, basis: SettlementBasis::Trimmed, ratio: None }
        );
    }

    #[test]
    fn node_guess_takes_precedence_and_is_negative() {
        // n=5、对 0 → 乱猜负分（阶梯其余形态不适用）。
        let settlement = Settlement::settle_node(25, 5, 5, 0);
        assert_eq!(settlement.basis, SettlementBasis::GuessPenalty);
        assert_eq!(settlement.earned_xp, -12);
        assert!(validate_settlement("node-completed", &settlement).is_ok());
    }

    #[test]
    fn review_base_floors_at_minimum_and_ladder_by_score() {
        // 客观题当量 1/2 都被下限 3 抬平；全对 = 3 + bonus 2。
        assert_eq!(
            Settlement::settle_review(ActivityKind::SingleChoice, 1.0),
            Settlement { earned_xp: 5, xp_bonus: 2, basis: SettlementBasis::Full, ratio: None }
        );
        assert_eq!(
            Settlement::settle_review(ActivityKind::Matching, 1.0),
            Settlement { earned_xp: 5, xp_bonus: 2, basis: SettlementBasis::Full, ratio: None }
        );
        // AI 批改当量 3：下限不抬，全对 = 3 + 2。
        assert_eq!(
            Settlement::settle_review(ActivityKind::OpenQuestion, 1.0),
            Settlement { earned_xp: 5, xp_bonus: 2, basis: SettlementBasis::Full, ratio: None }
        );
        // 错答/承认遗忘：partial ratio 0 → 恰 0（不误伤成负分）。
        assert_eq!(
            Settlement::settle_review(ActivityKind::SingleChoice, 0.0),
            Settlement { earned_xp: 0, xp_bonus: 0, basis: SettlementBasis::Partial, ratio: Some(0.0) }
        );
        // 部分正确（AI 批改得分率 0.7）：(3 × 0.7).floor() = 2。
        assert_eq!(
            Settlement::settle_review(ActivityKind::Reflection, 0.7),
            Settlement { earned_xp: 2, xp_bonus: 0, basis: SettlementBasis::Partial, ratio: Some(0.7) }
        );
    }

    // —— 乱猜判据 ——

    #[test]
    fn guessing_boundary_is_n4_and_half_baseline() {
        // 恰 4 题、0.124 < 0.125 → 判；0.125 及以上不判。
        assert!(is_guessing(4, 0.124));
        assert!(!is_guessing(4, 0.125));
        assert!(!is_guessing(3, 0.0), "n 不足不判（宁可漏判不可误伤）");
        assert!(!is_guessing(10, 0.5), "正常正确率不判");
        assert_eq!(guess_penalty_for_test(1), 1, "小基数罚金下限 1");
        assert_eq!(guess_penalty_for_test(25), 12);
    }

    fn guess_penalty_for_test(base: i64) -> i64 {
        // 经由公开面的等价路径：settle_node 的乱猜分支。
        -Settlement::settle_node(base, GUESS_MIN_QUESTIONS, GUESS_MIN_QUESTIONS, 0).earned_xp
    }

    // —— 题型当量表（起步值钉住，待校准改这里） ——

    #[test]
    fn question_equivalents_starting_values() {
        assert_eq!(question_equivalent(ActivityKind::SingleChoice), 1);
        assert_eq!(question_equivalent(ActivityKind::TrueFalse), 1);
        assert_eq!(question_equivalent(ActivityKind::MultiChoice), 2);
        assert_eq!(question_equivalent(ActivityKind::FillInBlank), 2);
        assert_eq!(question_equivalent(ActivityKind::Numeric), 2);
        assert_eq!(question_equivalent(ActivityKind::Matching), 2);
        assert_eq!(question_equivalent(ActivityKind::Ordering), 2);
        assert_eq!(question_equivalent(ActivityKind::Reflection), 3);
        assert_eq!(question_equivalent(ActivityKind::OpenQuestion), 3);
    }

    // —— quiz 准确率分级（q1，learnhub 真值表） ——

    /// quiz 准确率四档真值表（4 单选 = worked 4；门槛 0.9 / 0.6 / 0.3）：
    /// 1.0 → 4 + 2（full 含 bonus）；0.75 → 4（full 无 bonus）；0.5 → 2
    /// （partial 半额）；0.25 与 0 → 1（partial 四分之一额，恒非负）。
    /// 每档输出都过 `validate_settlement("quiz-completed", …)` 零放宽。
    #[test]
    fn quiz_ladder_tiers_by_accuracy() {
        let kinds = [ActivityKind::SingleChoice; 4];
        let cases: [(i64, i64, i64, SettlementBasis, Option<f64>); 5] = [
            (4, 6, 2, SettlementBasis::Full, None),
            (3, 4, 0, SettlementBasis::Full, None),
            (2, 2, 0, SettlementBasis::Partial, Some(0.5)),
            (1, 1, 0, SettlementBasis::Partial, Some(0.25)),
            (0, 1, 0, SettlementBasis::Partial, Some(0.0)),
        ];
        for (correct, earned, bonus, basis, ratio) in cases {
            let settlement = Settlement::settle_quiz(&kinds, correct, 4)
                .unwrap_or_else(|error| panic!("settle_quiz({correct}/4) failed: {error}"));
            assert_eq!(
                settlement,
                Settlement { earned_xp: earned, xp_bonus: bonus, basis, ratio },
                "准确率 {correct}/4 的授予档"
            );
            validate_settlement("quiz-completed", &settlement)
                .unwrap_or_else(|error| panic!("quiz settlement must pass the clause: {error}"));
        }
    }

    /// quiz 无 3 分钟下限（与复习线的关键区别）：2 单选全对 = worked 2 原样
    /// 作基数（复习线同型会被下限 3 抬平成 5）。
    #[test]
    fn quiz_has_no_minimum_floor_unlike_review() {
        let settlement = Settlement::settle_quiz(
            &[ActivityKind::SingleChoice, ActivityKind::SingleChoice],
            2,
            2,
        )
        .unwrap();
        assert_eq!(
            settlement,
            Settlement { earned_xp: 4, xp_bonus: 2, basis: SettlementBasis::Full, ratio: None },
            "worked 2 原样作基数，无 REVIEW_MIN_MINUTES 下限"
        );
        validate_settlement("quiz-completed", &settlement).unwrap();
    }

    /// quiz 无乱猜通道（测量非门禁）：8 单选全错 = 四分之一额 2，恒非负；
    /// 与教新线的乱猜负分（同结局 −12）专项对照。
    #[test]
    fn quiz_never_penalizes_even_all_wrong() {
        let kinds = [ActivityKind::SingleChoice; 8];
        let settlement = Settlement::settle_quiz(&kinds, 0, 8).unwrap();
        assert_eq!(
            settlement,
            Settlement { earned_xp: 2, xp_bonus: 0, basis: SettlementBasis::Partial, ratio: Some(0.0) },
            "worked 8 的四分之一额 = 2，恒非负"
        );
        validate_settlement("quiz-completed", &settlement).unwrap();
    }

    /// 零题拒绝（测量语义：无准确率可分级）+ 题单/作答数错配与答对数越界
    /// fail loud（错配即错价，不做静默宽容）。
    #[test]
    fn quiz_zero_questions_and_mismatches_fail_loud() {
        assert!(Settlement::settle_quiz(&[], 0, 0).is_err(), "零题拒绝");
        let kinds = [ActivityKind::SingleChoice, ActivityKind::FillInBlank];
        assert!(
            Settlement::settle_quiz(&kinds, 1, 1).is_err(),
            "题单与作答数错配拒绝"
        );
        assert!(
            Settlement::settle_quiz(&kinds, 3, 2).is_err(),
            "答对数超过作答数拒绝"
        );
    }

    /// quiz 常量集中表起步值钉住（待校准改这里一处）。
    #[test]
    fn quiz_constants_starting_values() {
        assert_eq!(QUIZ_TRIGGER_SETTLEMENTS, 6);
        assert_eq!(QUIZ_QUESTION_BUDGET, 15);
        assert!((QUIZ_EXCELLENT_RATIO - 0.9).abs() < f64::EPSILON);
        assert!((QUIZ_PASS_RATIO - 0.6).abs() < f64::EPSILON);
        assert!((QUIZ_HALF_RATIO - 0.3).abs() < f64::EPSILON);
    }
}
