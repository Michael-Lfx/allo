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

/// m10 自动补批的环境闸环境变量（`NOMI_LEARNING_AUTO_REFILL`）：off/0/false
/// = 自动补批关（verdict 恒 Disabled、端点如实读数）；未设或其它取值 = 开
/// （设置开关 `learning.autoRefillEnabled` 正常起作用）。
pub const AUTO_REFILL_SWITCH_ENV: &str = "NOMI_LEARNING_AUTO_REFILL";

/// 自动补批环境闸关闭判定（off/0/false，大小写与首尾空白宽容）。
pub fn is_auto_refill_disabled_value(value: &str) -> bool {
    is_off_switch_value(value)
}

/// verify_math 批量核验开关（u05，`NOMI_LEARNING_VERIFY_BATCH`）：未设 =
/// 关（**开发期缺省 off**——正优化协议：未经验证不缺省启用；真机 A/B
/// 达标后在收尾提交统一翻 on。票面主指标：verify_math 调用次数 /
/// 总 LLM 调用数 / 累计输入字符；推翻条件 = claim 覆盖下降 >20% 或
/// math_verify failed 上升 → 默认 off）；显式 on/1/true = 开（批量面在
/// 位，单 claim 形状向后兼容照旧可用）；off/0/false = 关——工具 schema/
/// 描述/R14 条款逐字节回退基线单 claim 形状（同二进制对照臂）。
pub const VERIFY_BATCH_SWITCH_ENV: &str = "NOMI_LEARNING_VERIFY_BATCH";

/// verify_math 批量核验开关判定（取值注入面，纯函数——测试不触进程 env）。
/// **缺省 off**：未设 = 基线单 claim 形状；显式非 off 取值 = 批量面。
pub fn verify_batch_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(false)
}

/// verify_math 批量核验开关的进程读数（[`verify_batch_enabled_value`] 同款语义）。
pub fn verify_batch_enabled() -> bool {
    verify_batch_enabled_value(std::env::var(VERIFY_BATCH_SWITCH_ENV).ok().as_deref())
}

/// 课时历史压缩实验开关（u06，`NOMI_LEARNING_LESSON_HISTORY_COMPACT`）：
/// **缺省 off**——高风险项（压缩后模型看不到自己写过的正文，可能伤跨节
/// 一致性），与碎片裁决闸同款「缺省关、显式开」反向语义：未设 / off / 0 /
/// false = 关（历史逐字节重发，基线行为）；其它取值（on/1/true/…）= 开
/// （已落库正文在历史中换摘要 + ls_inspect 可取回原文）。缺省值待真机
/// A/B（输入字符降 ≥40% 且护栏全过）才翻 on。
pub const LESSON_HISTORY_COMPACT_SWITCH_ENV: &str = "NOMI_LEARNING_LESSON_HISTORY_COMPACT";

/// 课时历史压缩开关判定（**缺省关**——取值语义与 [`verify_batch_enabled_value`]
/// 反向，见常量注记）：未设 = 关；显式非 off 取值 = 开。
pub fn lesson_history_compact_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(false)
}

/// 课时历史压缩开关的进程读数（[`lesson_history_compact_enabled_value`] 同款语义）。
pub fn lesson_history_compact_enabled() -> bool {
    lesson_history_compact_enabled_value(std::env::var(LESSON_HISTORY_COMPACT_SWITCH_ENV).ok().as_deref())
}

/// 开放题评审执行器开关（n5 机制面开关臂，`NOMI_LEARNING_OPEN_JUDGE`）：
/// 未设或其它取值 = 新评审链（重试环 N=2 带原因回灌 + 90s 显式超时 +
/// complete_json_with_thinking 通道 + schema 互斥校验）；off/0/false =
/// 旧 `grade_open_answer` 单次裸调路径（同二进制对照臂——双臂验证用，
/// off 臂缺 missing_points 语义，对照卡按此读数）。
pub const OPEN_JUDGE_SWITCH_ENV: &str = "NOMI_LEARNING_OPEN_JUDGE";

/// 开放题评审执行器开关判定（取值注入面，纯函数——测试不触进程 env）。
pub fn open_judge_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(true)
}

/// 开放题评审执行器开关的进程读数（[`open_judge_enabled_value`] 同款语义）。
pub fn open_judge_enabled() -> bool {
    open_judge_enabled_value(std::env::var(OPEN_JUDGE_SWITCH_ENV).ok().as_deref())
}

// ── 模型可用性修复波（u 系）与同形相邻开关的三件套收拢 ─────────────────
// u01 REDRAFT_SPLICE / u03 REVIEW_CONTEXT_V2 / u04 ANTI_LOOP / u07
// TIER_PRIORITY / u10 VARIANT_V2 五个本波开关与同形相邻的 EST_BUDGET
//（k03）原散在 learning_graph/mod.rs、lesson_draft.rs、
// generation/activities.rs 各自维护三件套（ENV 常量 + 进程读数 + 取值
// 判定）；本文件是唯一住址，原模块以 `pub(crate) use`（或既有 `pub use`
// 链）再导出保持旧路径，调用点零改动。取值面统一肯定式命名
// `*_enabled_value(Option<&str>)`（纯函数——测试不触进程 env）。

/// 测试级 env 窗口守卫：`NOMI_LEARNING_*` 开关是进程全局态——写 env 的
/// 测试必须 `#[serial_test::serial]` 互斥，Drop 时恢复原值（非串行的
/// save/restore 会泄漏进并行测试）；`value = None` = 移除该变量（「未设」
/// 缺省臂）。此前 growth_retry.rs / learning_graph/mod.rs /
/// lesson_draft.rs / nomifun-ai-agent lesson_content_loop.rs 四份逐字
/// 重复的脚手架合并为这一份（ai-agent 经 `nomifun_learning::` 引用同一
/// 份）。测试辅助面：产品代码不写 env。
pub fn switch_env_guard(env_var: &'static str, value: Option<&str>) -> impl Drop {
    struct Restore(&'static str, Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            match self.1.take() {
                Some(value) => unsafe { std::env::set_var(self.0, value) },
                None => unsafe { std::env::remove_var(self.0) },
            }
        }
    }
    let previous = std::env::var(env_var).ok();
    match value {
        Some(value) => unsafe { std::env::set_var(env_var, value) },
        None => unsafe { std::env::remove_var(env_var) },
    }
    Restore(env_var, previous)
}

/// 点修回填的运维开关（u01，模型可用性修复波）：置 off/0/false = 回填臂
/// 休眠——重裁候选稿不回填、守卫文案与修订纪律逐字回基线形（off 臂）；
/// 未设 = 关（**开发期缺省 off**——正优化协议：未经验证不缺省启用，真机
/// A/B 达标后收尾提交统一翻 on）；显式非 off 取值 = 开。on 臂三件：①重裁解析
/// 后守卫前，上一稿中不在点名集、候选稿同标题仍在的节点整对象回填（抹掉
/// 标点/措辞漂移——真机 run-1793951137445：重裁把未点名节点 purpose
/// 「粗/中/细」写成「粗、中、细」→ 守卫拦 → 原地重试又改另一节点 → 二次
/// 越权耗轮）；②守卫文案改指令式并给「从上一稿复制该节点 JSON」的可执行
/// 出口；③修订纪律补回填句（改了也白改，模型不再被要求搬运未点名节点）。
pub const REDRAFT_SPLICE_SWITCH_ENV: &str = "NOMI_LEARNING_REDRAFT_SPLICE";

/// 点修回填开关判定（取值注入面，纯函数——测试不触进程 env）。缺省 off。
pub fn redraft_splice_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(false)
}

/// 点修回填是否开启（未设 = 关；显式非 off 取值 = 开）。
pub fn redraft_splice_enabled() -> bool {
    redraft_splice_enabled_value(std::env::var(REDRAFT_SPLICE_SWITCH_ENV).ok().as_deref())
}

/// 评审输入补全的运维开关（u03，模型可用性修复波）：置 off/0/false = 补全
/// 臂休眠——评审 user 不渲染候选词块、系统提示词不织两条款（off 臂逐字节
/// 回基线）；未设 = 关（**开发期缺省 off**——正优化协议：未经验证不缺省
/// 启用，真机 A/B 达标后收尾提交统一翻 on）；显式非 off 取值 = 开。on 臂三件：
/// ①评审 user 每条提名下渲染「挂号表候选词」——定义原文的连续 CJK 片段
/// 机械切出（不掩码），常识/缺失行的 term 逐字照抄有源（真机两次挂号表
/// 违约都死在机械层只给掩码残片、模型凭记忆复述定义：滤杯循环自证、
/// 「可溶物质」案）；②一条 problem 的 issue 可并列指名多词（顿号分隔），
/// 名词裁决按词核验不受 8 条上限约束（run-1793859632428：判 11 个名词时
/// 7 个无法逐条指名的算术死结）；③「宁放勿错」与碎片「拿不准按名词」两
/// 类不确定默认的分工厘清。
pub const REVIEW_CONTEXT_V2_SWITCH_ENV: &str = "NOMI_LEARNING_REVIEW_CONTEXT_V2";

/// 评审输入补全开关判定（取值注入面，纯函数——测试不触进程 env）。
pub fn review_context_v2_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(false)
}

/// 评审输入补全是否开启（未设 = 关；显式非 off 取值 = 开）。
pub fn review_context_v2_enabled() -> bool {
    review_context_v2_enabled_value(std::env::var(REVIEW_CONTEXT_V2_SWITCH_ENV).ok().as_deref())
}

/// 反循环臂的运维开关（u04，模型可用性修复波）：置 off/0/false = 三件全
/// 休眠、提示词逐字节回基线；未设 = 关（**开发期缺省 off**——正优化协议：
/// 未经验证不缺省启用，真机 A/B 达标后收尾提交统一翻 on）；显式非 off
/// 取值 = 开。on 臂三件：①课程语境词——课程标题 + 学习目标的确定性切片（复用
/// generation 的 term_slices）渲染为「可作定义的背景用语，不单独要求挂
/// 号；作为本节点教学对象（teaches）时照常挂号」，进教练首批块与评审
/// user，并加入点名清单已知面（这些词不再作为碎片下发）——空已教账 +
/// 判据①要求每个领域词有出处的死结由此解开（「手冲咖啡」课的入门定义不
/// 可能不用「手冲/冲煮/咖啡粉」）；②评审翻案锁——有状态复审时前轮意见
/// 随评审 user 回灌并锁定「按前轮意见做出的修改本身不得再判为问题」
/// （反乒乓纪律此前只约束教练，评审不受约束才有了跨轮翻案）；③教练声
/// 明面条款补「边界只写一次」——『不教 X / 留待后续』只写 scope，intent
/// 只写角色，不再两字段各写一遍（5 节点 4 个重复的边界声明）。
pub const ANTI_LOOP_SWITCH_ENV: &str = "NOMI_LEARNING_ANTI_LOOP";

/// 反循环臂开关判定（取值注入面，纯函数——测试不触进程 env）。缺省 off。
pub fn anti_loop_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(false)
}

/// 反循环臂是否开启（未设 = 关；显式非 off 取值 = 开）。
pub fn anti_loop_enabled() -> bool {
    anti_loop_enabled_value(std::env::var(ANTI_LOOP_SWITCH_ENV).ok().as_deref())
}

/// 图课时「时长预算」行与超容自报的运维开关（k03，learnhub #154）：
/// 置 off/0/false 时不注入预算行、计划工具的自报键按缺席处理（=旧行为）。
/// 与 `NOMI_LEARNING_SOURCE_PLAN` 同款进程内运维开关，不进 UI/用户配置面。
pub const EST_BUDGET_SWITCH_ENV: &str = "NOMI_LEARNING_EST_BUDGET";

/// 时长预算开关判定（取值注入面，纯函数——测试不触进程 env）。
pub fn est_budget_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(true)
}

/// 时长预算机制是否开启（未设/其它值 = 开启）。
pub fn est_budget_enabled() -> bool {
    est_budget_enabled_value(std::env::var(EST_BUDGET_SWITCH_ENV).ok().as_deref())
}

/// 档位优先序运维开关（u07，`NOMI_LEARNING_TIER_PRIORITY`）：只管两项
/// 模型可见面——①生成系统提示词的「档位优先序」条款（ai-agent 面）；
/// ②本面审计的节长上限 warning（`section_over_budget`）。未设 = 关（
/// **开发期缺省 off**——正优化协议：未经验证不缺省启用，真机 A/B 达标后
/// 收尾提交统一翻 on；票面推翻条件：on 臂知道档出现「只剩定义、无情境
/// 引入」类空心化 → 默认 off）；显式非 off 取值 = 开；off/0/false = 关
///（两处逐字节回基线）。修复轮上下文注入与发布前数学失败回给模型两项为
/// 机械正确性补全，**不挂本开关**（票面明示）。
pub const TIER_PRIORITY_SWITCH_ENV: &str = "NOMI_LEARNING_TIER_PRIORITY";

/// 档位优先序开关判定（取值注入面，纯函数——测试不触进程 env）。缺省 off。
pub fn tier_priority_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(false)
}

/// 档位优先序开关的进程读数（[`tier_priority_enabled_value`] 同款语义）。
pub fn tier_priority_enabled() -> bool {
    tier_priority_enabled_value(std::env::var(TIER_PRIORITY_SWITCH_ENV).ok().as_deref())
}

/// 变式链 V2 开关（u10，正优化验收协议）：`NOMI_LEARNING_VARIANT_V2`——on
/// 臂装配变式判底（有正文才装、缺席如实跳过）、证据逐字核、按题型契约、
/// 变式指令独立段；off 臂与基线逐字节一致（单测钉住）。未设 = 关（**开发
/// 期缺省 off**——正优化协议：未经验证不缺省启用）；真机 A/B 达标（变式
/// 成功率/违约重试/证据逐字命中改善且护栏不劣化）后在收尾提交统一翻 on。
pub const VARIANT_V2_SWITCH_ENV: &str = "NOMI_LEARNING_VARIANT_V2";

/// 变式链 V2 开关判定（取值注入面，纯函数——测试不触进程 env）。
pub fn variant_v2_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_off_switch_value(value)).unwrap_or(false)
}

/// 变式链 V2 是否开启（未设 = 关；显式非 off 取值 = 开）。
pub fn variant_v2_enabled() -> bool {
    variant_v2_enabled_value(std::env::var(VARIANT_V2_SWITCH_ENV).ok().as_deref())
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

    /// 开放题评审执行器开关（n5）：未设 = 开（新评审链缺省在位）；off 系 =
    /// 关（旧单次裸调对照臂）。
    #[test]
    fn open_judge_switch_defaults_on_and_off_arm_is_explicit() {
        assert!(open_judge_enabled_value(None), "未设 = 新评审链");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!open_judge_enabled_value(Some(value)), "{value:?} = 旧路径对照臂");
        }
        for value in ["on", "1", "true", "yes", ""] {
            assert!(open_judge_enabled_value(Some(value)), "{value:?} = 新评审链");
        }
    }

    /// verify_math 批量核验开关（u05）：未设 = 关（**开发期缺省 off**——
    /// 正优化协议：未经验证不缺省启用，A/B 达标后收尾提交统一翻 on）；
    /// 显式 on = 批量面在位；off 系 = 关（工具面/R14 条款逐字节回退基线的
    /// 对照臂）。纯函数面测试（不触进程 env）。
    #[test]
    fn verify_batch_switch_defaults_off_and_explicit_on_enables() {
        assert!(!verify_batch_enabled_value(None), "未设 = 基线单 claim 形状（开发期缺省 off）");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!verify_batch_enabled_value(Some(value)), "{value:?} = 基线对照臂");
        }
        for value in ["on", "1", "true", "yes", ""] {
            assert!(verify_batch_enabled_value(Some(value)), "{value:?} = 批量面");
        }
    }

    /// 课时历史压缩开关（u06）：**缺省 off**（高风险项——压缩伤跨节一致性
    /// 的风险未证伪前不缺省开）；显式非 off 取值 = 开。与 u05 同为纯函数面
    /// 测试（不触进程 env）。
    #[test]
    fn lesson_history_compact_switch_defaults_off_and_requires_explicit_on() {
        assert!(!lesson_history_compact_enabled_value(None), "未设 = 关（缺省 off）");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!lesson_history_compact_enabled_value(Some(value)), "{value:?} = 关");
        }
        for value in ["on", "1", "true", "yes"] {
            assert!(lesson_history_compact_enabled_value(Some(value)), "{value:?} = 开");
        }
    }

    // ── 收拢面（u 系五开关 + EST_BUDGET）：三件套的纯函数取值判定统一住
    // 本文件；缺省臂断言与收尾口径一致（本波开发期缺省一律 off——正优化
    // 协议：未经验证不缺省启用，A/B 达标后收尾提交统一翻 on）。
    // 纯函数面（不触进程 env）。

    /// 点修回填开关（u01）：未设 = 关（开发期缺省 off）；显式 on = 开；
    /// off 系 = 关。
    #[test]
    fn redraft_splice_switch_defaults_off_and_explicit_on_enables() {
        assert!(!redraft_splice_enabled_value(None), "未设 = 基线（开发期缺省 off）");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!redraft_splice_enabled_value(Some(value)), "{value:?} = 基线对照臂");
        }
        for value in ["on", "1", "true", "yes", ""] {
            assert!(redraft_splice_enabled_value(Some(value)), "{value:?} = 回填臂");
        }
    }

    /// 评审输入补全开关（u03）：未设 = 关（开发期缺省 off）；显式 on = 开；
    /// off 系 = 关。
    #[test]
    fn review_context_v2_switch_defaults_off_and_explicit_on_enables() {
        assert!(!review_context_v2_enabled_value(None), "未设 = 基线（开发期缺省 off）");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!review_context_v2_enabled_value(Some(value)), "{value:?} = 基线对照臂");
        }
        for value in ["on", "1", "true", "yes", ""] {
            assert!(review_context_v2_enabled_value(Some(value)), "{value:?} = 补全臂");
        }
    }

    /// 反循环臂开关（u04）：未设 = 关（开发期缺省 off）；显式 on = 开；
    /// off 系 = 关。
    #[test]
    fn anti_loop_switch_defaults_off_and_explicit_on_enables() {
        assert!(!anti_loop_enabled_value(None), "未设 = 基线（开发期缺省 off）");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!anti_loop_enabled_value(Some(value)), "{value:?} = 基线对照臂");
        }
        for value in ["on", "1", "true", "yes", ""] {
            assert!(anti_loop_enabled_value(Some(value)), "{value:?} = 反循环臂");
        }
    }

    /// 时长预算开关（k03）：未设 = 开（默认开）；off 系 = 关。
    #[test]
    fn est_budget_switch_defaults_on_and_off_values_disable() {
        assert!(est_budget_enabled_value(None), "未设 = 预算行在位（默认开）");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!est_budget_enabled_value(Some(value)), "{value:?} = 关");
        }
        for value in ["on", "1", "true", "yes", ""] {
            assert!(est_budget_enabled_value(Some(value)), "{value:?} = 开");
        }
    }

    /// 档位优先序开关（u07）：未设 = 关（开发期缺省 off）；显式 on = 开；
    /// off 系 = 关。
    #[test]
    fn tier_priority_switch_defaults_off_and_explicit_on_enables() {
        assert!(!tier_priority_enabled_value(None), "未设 = 基线（开发期缺省 off）");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!tier_priority_enabled_value(Some(value)), "{value:?} = 基线对照臂");
        }
        for value in ["on", "1", "true", "yes", ""] {
            assert!(tier_priority_enabled_value(Some(value)), "{value:?} = 条款在位");
        }
    }

    /// 变式链 V2 开关（u10）：未设 = 关（开发期缺省 off）；显式 on = 开；
    /// off 系 = 关。
    #[test]
    fn variant_v2_switch_defaults_off_and_explicit_on_enables() {
        assert!(!variant_v2_enabled_value(None), "未设 = 基线（开发期缺省 off）");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!variant_v2_enabled_value(Some(value)), "{value:?} = 基线对照臂");
        }
        for value in ["on", "1", "true", "yes", ""] {
            assert!(variant_v2_enabled_value(Some(value)), "{value:?} = 变式 V2");
        }
    }
}
