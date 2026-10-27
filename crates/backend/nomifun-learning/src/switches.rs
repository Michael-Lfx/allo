//! 运维开关的公共取值判定（四轮吸收 k 系）：`NOMI_LEARNING_*` 系列的
//! off/0/false 语义归一住这里——各机制的公开判定函数只做委派，避免六份
//! 逐字相同的 match。
//!
//! 开关是运维级旋钮（不进 UI/用户配置面）：未设或其它取值 = 机制开启，
//! 只有明确写入 off/0/false（大小写与首尾空白宽容）才关闭。

/// 关闭取值判定：`off` / `0` / `false`（大小写与首尾空白宽容）。
pub fn is_off_switch_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "off" | "0" | "false"
    )
}

/// 铸名门外部通行名证据开关（m11，ADR-0030 门内增强）：未设/其它取值 =
/// 开启；`off` = 零搜索零证据 = 现行为（证据获取器 fail-open 的总闸）。
pub const MINT_RESEARCH_SWITCH_ENV: &str = "NOMI_LEARNING_MINT_RESEARCH";

/// 铸名门外部证据开关判定（[`is_off_switch_value`] 同款语义）。
pub fn mint_research_enabled() -> bool {
    std::env::var(MINT_RESEARCH_SWITCH_ENV)
        .map(|value| !is_off_switch_value(&value))
        .unwrap_or(true)
}

/// m7 任务资源闸的开关环境变量（`NOMI_LEARNING_TASK_GATE`）：off/0/false =
/// 无上限直跑（闸前行为）；未设或其它取值 = 过闸（在跑上限 5 + FIFO 排队
/// + 排队可见/可取消）。
pub const TASK_GATE_SWITCH_ENV: &str = "NOMI_LEARNING_TASK_GATE";

/// 任务资源闸关闭判定（off/0/false，大小写与首尾空白宽容）。
pub fn is_task_gate_disabled_value(value: &str) -> bool {
    is_off_switch_value(value)
}

/// 碎片裁决阻断开关（m9 补接，learnhub `settings.review_fragment_enforce`
/// 的 allo 开关位）：**缺省关闭**——这是本仓第一个缺省关的运维开关（阻断门
/// 缺省不开：用语表 v0 未成熟前机械阻断是假阳性放大器，m4 Resolution 同
/// learnhub #176 实证）。取值语义与全仓约定**反向**：
/// - 未设 / off / 0/false = 关（v1 report-only——评审碎片面照跑，未处置
///   片段只降 findings 不打回）；
/// - 其它取值（on/1/true/…）= 开：未处置片段合成打回问题（L1 级门失败，
///   走既有评审重裁回路）。
pub const LEXICON_ENFORCE_SWITCH_ENV: &str = "NOMI_LEARNING_LEXICON_ENFORCE";

/// 碎片裁决阻断开关判定（**缺省关**——与 [`is_off_switch_value`] 的「未设
/// = 开」约定反向，见常量注记）：未设 = 关；显式非 off 取值 = 开。
pub fn lexicon_enforce_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(false)
}

/// 碎片裁决阻断开关的进程读数（[`lexicon_enforce_enabled_value`] 同款语义）。
pub fn lexicon_enforce_enabled() -> bool {
    lexicon_enforce_enabled_value(std::env::var(LEXICON_ENFORCE_SWITCH_ENV).ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_switch_values_are_strict() {
        for value in ["off", "OFF", "Off", "0", "false", "False", " off "] {
            assert!(is_off_switch_value(value), "{value:?} 关闭");
        }
        for value in ["on", "1", "true", "yes", "", " disable"] {
            assert!(!is_off_switch_value(value), "{value:?} 不关闭");
        }
    }

    #[test]
    fn task_gate_switch_delegates_to_off_values() {
        for value in ["off", "0", "false", " OFF ", "False"] {
            assert!(
                is_task_gate_disabled_value(value),
                "{value:?} = 闸关（直跑）"
            );
        }
        for value in ["on", "1", "true", ""] {
            assert!(
                !is_task_gate_disabled_value(value),
                "{value:?} = 闸开（cap 5 + 排队）"
            );
        }
    }

    /// 碎片裁决阻断开关（m9 补接）：**缺省关**（未设 = report-only，不阻断
    /// ——缺省关是本开关与全仓约定的唯一反向点）；显式非 off 取值 = 阻断。
    /// 纯函数面测试（不触进程 env——env 窗口会泄漏进并行生长测试）。
    #[test]
    fn lexicon_enforce_switch_defaults_off_and_blocks_only_when_explicit() {
        // 未设 = 关（v1 report-only 缺省臂）。
        assert!(!lexicon_enforce_enabled_value(None), "未设 = 关（阻断门缺省不开）");
        // 显式 off 取值 = 关。
        for value in ["off", "OFF", "0", "false", " False "] {
            assert!(!lexicon_enforce_enabled_value(Some(value)), "{value:?} = 关");
        }
        // 显式非 off 取值 = 开（L1 阻断臂）。
        for value in ["on", "1", "true", "yes"] {
            assert!(lexicon_enforce_enabled_value(Some(value)), "{value:?} = 开（L1 阻断）");
        }
    }
}
