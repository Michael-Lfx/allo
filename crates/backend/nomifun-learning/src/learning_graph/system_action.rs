//! 学习系统动作簿的枚举单源（y04，ADR-0015：枚举集校验在 Rust，DB 无 CHECK）。
//!
//! 系统动作 = 课程上的自主/后台动作（维护修补、维护挂载、生长）的**终态**
//! 留痕。进行中状态由 generation task 注册表承担（内存栏），终态由
//! `learning_system_actions` 承担——两边不互相复制。
//!
//! 命名面是机器名（HTTP 视图与日志直接透出），人面文案在前端 i18n。

use std::fmt;

/// 动作类别：哪一类系统动作落账。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemActionKind {
    /// 结构修补回合（findings/挂载观察驱动或手动触发）。
    MaintenanceRepair,
    /// 结构挂载回合（待挂节点 → 结构块）。
    MaintenanceMount,
    /// 生长批次（教练起草 → 门 → 落库）。
    Growth,
}

impl SystemActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MaintenanceRepair => "maintenance_repair",
            Self::MaintenanceMount => "maintenance_mount",
            Self::Growth => "growth",
        }
    }
}

impl fmt::Display for SystemActionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 触发源：谁点燃了这次动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemActionTrigger {
    /// 维护定时器（app-server 调度的一轮 tick）。
    Timer,
    /// 用户显式命令（POST mount/repair/grow）。
    Manual,
    /// 学习结算后的节点完成/跳过钩子（自动补批触发①）。
    Settlement,
    /// 课程工作区加载时的自动补批判定（自动补批触发②）。
    AutoRefill,
}

impl SystemActionTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Timer => "timer",
            Self::Manual => "manual",
            Self::Settlement => "settlement",
            Self::AutoRefill => "auto_refill",
        }
    }

    /// 手动动作才记操作者；自动动作（timer/settlement/auto_refill）不记。
    pub fn records_actor(self) -> bool {
        matches!(self, Self::Manual)
    }

    /// POST grow 的可选 query `trigger=` 解析（缺省 manual，向后兼容）：
    /// `manual` / `auto_refill` 两个外部可达值；settlement 只来自结算钩子，
    /// timer 只来自维护调度器，均不接受外部注入。
    pub fn from_grow_query(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "auto_refill" => Some(Self::AutoRefill),
            _ => None,
        }
    }
}

impl fmt::Display for SystemActionTrigger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 终态结果：一次动作只有 done 或 failed（取消/忙拒不落账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemActionResult {
    Done,
    Failed,
}

impl SystemActionResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for SystemActionResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_names_are_the_wire_contract() {
        assert_eq!(SystemActionKind::MaintenanceRepair.as_str(), "maintenance_repair");
        assert_eq!(SystemActionKind::MaintenanceMount.as_str(), "maintenance_mount");
        assert_eq!(SystemActionKind::Growth.as_str(), "growth");
        assert_eq!(SystemActionTrigger::Timer.as_str(), "timer");
        assert_eq!(SystemActionTrigger::Manual.as_str(), "manual");
        assert_eq!(SystemActionTrigger::Settlement.as_str(), "settlement");
        assert_eq!(SystemActionTrigger::AutoRefill.as_str(), "auto_refill");
        assert_eq!(SystemActionResult::Done.as_str(), "done");
        assert_eq!(SystemActionResult::Failed.as_str(), "failed");
    }

    #[test]
    fn only_manual_records_the_actor() {
        assert!(SystemActionTrigger::Manual.records_actor());
        for trigger in [
            SystemActionTrigger::Timer,
            SystemActionTrigger::Settlement,
            SystemActionTrigger::AutoRefill,
        ] {
            assert!(!trigger.records_actor(), "{trigger} must not record an actor");
        }
    }

    #[test]
    fn grow_query_accepts_manual_and_auto_refill_only() {
        assert_eq!(
            SystemActionTrigger::from_grow_query("manual"),
            Some(SystemActionTrigger::Manual)
        );
        assert_eq!(
            SystemActionTrigger::from_grow_query("auto_refill"),
            Some(SystemActionTrigger::AutoRefill)
        );
        assert_eq!(SystemActionTrigger::from_grow_query("settlement"), None);
        assert_eq!(SystemActionTrigger::from_grow_query("timer"), None);
        assert_eq!(SystemActionTrigger::from_grow_query("bogus"), None);
    }
}
