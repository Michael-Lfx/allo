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

/// m10 自动补批的环境闸环境变量（`NOMI_LEARNING_AUTO_REFILL`）：off/0/false
/// = 自动补批关（verdict 恒 Disabled、端点如实读数）；未设或其它取值 = 开
/// （设置开关 `learning.autoRefillEnabled` 正常起作用）。
pub const AUTO_REFILL_SWITCH_ENV: &str = "NOMI_LEARNING_AUTO_REFILL";

/// 自动补批环境闸关闭判定（off/0/false，大小写与首尾空白宽容）。
pub fn is_auto_refill_disabled_value(value: &str) -> bool {
    is_off_switch_value(value)
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

    /// m10 自动补批环境闸：off 系 = 关（verdict 恒 Disabled）；on 系/未设
    /// 面的宽容值 = 开。
    #[test]
    fn auto_refill_switch_delegates_to_off_values() {
        for value in ["off", "0", "false", " OFF ", "False"] {
            assert!(
                is_auto_refill_disabled_value(value),
                "{value:?} = 自动补批关"
            );
        }
        for value in ["on", "1", "true", ""] {
            assert!(
                !is_auto_refill_disabled_value(value),
                "{value:?} = 自动补批开"
            );
        }
    }
}
