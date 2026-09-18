//! Compatibility report derivation per `03-codebuddy-compatibility-matrix.md`.
//!
//! `semantic_status` values match the wire serialization of
//! `nomifun_api_types::AppServerCompatibilityStatus` (snake_case);
//! `reasons` carry the documented hyphenated reason codes
//! (`ignored-by-source-runtime`, `unsupported-auth`, ...) — reason codes are
//! never first-class statuses (02 §11.2).

use crate::models::{
    CompatTriple, DIST_LOCAL_ONLY, RUNTIME_NOT_VERIFIED, REASON_IGNORED_BY_SOURCE_RUNTIME,
};

fn triple(semantic_status: &str, reasons: Vec<String>) -> CompatTriple {
    CompatTriple {
        semantic_status: semantic_status.to_owned(),
        runtime_status: RUNTIME_NOT_VERIFIED.to_owned(),
        distribution_status: DIST_LOCAL_ONLY.to_owned(),
        reasons,
    }
}

pub fn agent(has_ignored_permission_fields: bool) -> CompatTriple {
    let mut reasons = vec![
        "frontmatter 字段可结构化保留（02 §5.1）".into(),
        "AgentDefinition→Preset/ResolvedPresetSnapshot→Runtime 链路待验证（03 §6）".into(),
    ];
    if has_ignored_permission_fields {
        reasons.push(format!(
            "{REASON_IGNORED_BY_SOURCE_RUNTIME}: 插件 Agent 的 mcpServers/permissionMode 被来源运行时忽略（02 §5.1）"
        ));
    }
    triple("compatible_with_adapter", reasons)
}

pub fn team(all_members_resolved: bool) -> CompatTriple {
    let mut reasons = vec![
        "固定成员 + Planning Context 驱动的 planned DAG/局部并行/重试/replan 属 V1 能力（03 §3）".into(),
        "完整 Mailbox/成员自主认领/长期成员会话不属于 V1".into(),
    ];
    let semantic = if all_members_resolved {
        "compatible_with_adapter"
    } else {
        reasons.push("成员文件缺失/不可解析 → 需人工审查（02 §6 step 3）".into());
        "manual_review"
    };
    triple(semantic, reasons)
}

pub fn skill() -> CompatTriple {
    triple("compatible", vec![
        "SKILL.md 正文/frontmatter/$ARGUMENTS 保留，路径在快照内（02 §5）".into(),
        "脚本执行默认关闭，运行验收依赖可加载性检查（03 §6 Skill 主体）".into(),
    ])
}

pub fn command() -> CompatTriple {
    triple("compatible_with_adapter", vec![
        "可作为用户可调用 prompt/skill 定义（plugin:command 形态，03 §2 Command）".into(),
    ])
}

pub fn hook() -> CompatTriple {
    triple("manual_review", vec![
        "V1 只导入与静态校验，默认不执行（02 §5 / 03 §3）".into(),
    ])
}

pub fn lsp() -> CompatTriple {
    triple("manual_review", vec![
        "V1 元数据级；进程托管能力未落地（03 §2 LSP）".into(),
    ])
}

pub fn connector() -> CompatTriple {
    triple("compatible_with_adapter", vec![
        "工具命名空间化 connector__name__tool（03 §2 MCP）".into(),
        "凭据绑定与授权校验在连接器运行时验证（02 §5）".into(),
    ])
}

pub fn credential() -> CompatTriple {
    triple("compatible_with_adapter", vec![
        "导入只生成 schema 与引用；值由用户后续经安全存储提供（02 §10）".into(),
    ])
}

pub fn dependency() -> CompatTriple {
    triple("compatible_with_adapter", vec![
        "跨市场依赖默认禁止，需显式 allowlist（02 §8）".into(),
    ])
}

pub fn script() -> CompatTriple {
    triple("manual_review", vec![
        "导入期不执行任何脚本或命令（02 §10）".into(),
    ])
}

/// Aggregated snapshot-level triple used by the import result: reflects the
/// "worst" component semantic so the UI never overstates readiness.
pub fn snapshot_aggregate(components: &[crate::models::Component]) -> CompatTriple {
    let mut semantic = "compatible".to_owned();
    let mut reasons: Vec<String> = Vec::new();
    for component in components {
        match component.compatibility.semantic_status.as_str() {
            "manual_review" => semantic = "manual_review".to_owned(),
            "unsupported" if semantic != "manual_review" => semantic = "unsupported".to_owned(),
            "compatible_with_adapter" if semantic == "compatible" => {
                semantic = "compatible_with_adapter".to_owned()
            }
            _ => {}
        }
        for reason in &component.compatibility.reasons {
            if !reasons.contains(reason) {
                reasons.push(reason.clone());
            }
        }
    }
    reasons.truncate(8);
    triple(&semantic, reasons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Component, KIND_AGENT, KIND_HOOK, KIND_SKILL, KIND_TEAM};

    fn component(kind: &str, triple: CompatTriple) -> Component {
        Component::new(kind, "id".into(), "name".into(), None, triple, serde_json::json!({}))
    }

    #[test]
    fn statuses_follow_the_matrix() {
        assert_eq!(agent(false).semantic_status, "compatible_with_adapter");
        assert_eq!(agent(true).semantic_status, "compatible_with_adapter");
        assert!(agent(true).reasons.iter().any(|reason| reason.contains(REASON_IGNORED_BY_SOURCE_RUNTIME)));
        assert_eq!(team(true).semantic_status, "compatible_with_adapter");
        assert_eq!(team(false).semantic_status, "manual_review");
        assert_eq!(skill().semantic_status, "compatible");
        assert_eq!(command().semantic_status, "compatible_with_adapter");
        assert_eq!(hook().semantic_status, "manual_review");
        assert_eq!(lsp().semantic_status, "manual_review");
        assert_eq!(connector().semantic_status, "compatible_with_adapter");
        assert_eq!(credential().semantic_status, "compatible_with_adapter");
        assert_eq!(script().semantic_status, "manual_review");
        for t in [skill(), agent(false), team(true)] {
            assert_eq!(t.runtime_status, "not-verified");
            assert_eq!(t.distribution_status, "local-only");
        }
    }

    #[test]
    fn snapshot_aggregate_takes_the_worst_semantic() {
        let all_ok = snapshot_aggregate(&[component(KIND_SKILL, skill())]);
        assert_eq!(all_ok.semantic_status, "compatible");
        let mixed = snapshot_aggregate(&[
            component(KIND_SKILL, skill()),
            component(KIND_AGENT, agent(false)),
            component(KIND_HOOK, hook()),
        ]);
        assert_eq!(mixed.semantic_status, "manual_review");
        let adapter_only =
            snapshot_aggregate(&[component(KIND_AGENT, agent(false)), component(KIND_TEAM, team(true))]);
        assert_eq!(adapter_only.semantic_status, "compatible_with_adapter");
    }
}