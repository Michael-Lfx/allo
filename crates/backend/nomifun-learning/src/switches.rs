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
}
