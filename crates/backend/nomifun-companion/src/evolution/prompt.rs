//! 技能起草器 / 评审器的提示词与严格 JSON 解析（design §5.2 / §5.3）。
//!
//! 两个阶段都走 `one_shot_completion(tools:[])`（选 model，不切 agent）。解析容错完全
//! 镜像 `crate::prompt::{parse_learn_output, extract_json_object}`：容忍 ```json 围栏与
//! 周围散文，抽最外层 `{...}`。

use serde::Deserialize;

use super::miner::MinedPattern;

/// 起草器输出：一份技能的 frontmatter 字段 + 正文。
#[derive(Debug, Clone, Deserialize)]
pub struct DraftOutput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub when_to_use: Option<String>,
    #[serde(default)]
    pub body: String,
}

/// 评审器裁决。
#[derive(Debug, Clone, Deserialize)]
pub struct CriticVerdict {
    #[serde(default)]
    pub approve: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

/// 语义判重裁决：新草稿是否与某个已有技能语义等价/是其子集。
#[derive(Debug, Clone, Deserialize)]
pub struct DedupVerdict {
    /// 命中的已有技能名（kebab-case）；`None`/空串表示不重复。
    #[serde(default)]
    pub duplicate_of: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// 一条已有技能的摘要，喂给判重器判断语义重叠。
#[derive(Debug, Clone)]
pub struct SkillDigest {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
}

/// 整合规划中的一个冗余分组：`canonical` 保留、`duplicates` 逐个并入后归档。
#[derive(Debug, Clone, Deserialize)]
pub struct ConsolidateGroup {
    /// 保留下来的规范技能名（必须来自清单）。
    #[serde(default)]
    pub canonical: String,
    /// 与 `canonical` 语义冗余、应并入并归档的技能名（均须来自清单）。
    #[serde(default)]
    pub duplicates: Vec<String>,
}

/// 存量技能整合规划：把已有技能里语义冗余的分组，供逐组融合+归档。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConsolidatePlan {
    #[serde(default)]
    pub groups: Vec<ConsolidateGroup>,
}

/// 起草器 system：只产 JSON，禁围栏/散文，给精确骨架。
///
/// 核心：很多任务是**多轮完成的**（先收集信息、再分析、换角度探索、反复迭代、最后验证收敛）。
/// 提示词明确要求按真实复杂度产出**分阶段方法论**并保留迭代/决策点，避免把多轮过程压成
/// “一次做完”的线性清单（那样会丢掉它真正奏效的做法）。body 首行落 `运行方式：…` 标记，
/// 让使用时就知道这是多轮方法论还是单轮动作。
pub const DRAFT_SYSTEM: &str = "你是技能起草器。主人反复做某套操作,你要把它固化成一个可复用技能(SKILL.md)。\
关键:很多任务是**多轮完成的**——先收集信息、再分析、换多个角度探索、反复迭代、最后验证收敛。\
不要把这种过程压成'一次性做完'的线性清单,那样会丢掉它真正奏效的做法。\
只输出一个 JSON 对象,不要任何解释、不要代码围栏。字段:\n\
{\"name\":\"kebab-case 短名\",\"description\":\"一句话说明做什么;若是多轮方法论请点明\",\
\"when_to_use\":\"什么情况下该用它(一句话)\",\"body\":\"markdown 操作手册\"}\n\
body 写法要求:\n\
1. 第一行写运行方式,二选一:`运行方式：多轮迭代` 或 `运行方式：单轮直接执行`。\n\
2. 按任务真实复杂度决定结构:过程越长/往返越多/越需探索,就写成**分阶段方法论**\
(如 ## 信息收集 / ## 分析与多角度探索 / ## 迭代尝试 / ## 验证与收敛),\
每个阶段写清要收集或判断什么、何时进入下一步、何时该回头补信息或换个角度;\
简单的一次性任务才写成简洁线性步骤。\n\
3. 保留决策点与迭代循环(如'若结果不满足则回到某阶段'),不要抹平成一条直线。\n\
4. name 只含小写字母数字和连字符;description 必须非空。\n\
技能目录下有三个子目录可供引用: references/(只读参考文档) templates/(可复制模板) scripts/(可执行脚本)。\
在 body 中如需引用,用相对路径如 `references/api-spec.md` 或 `templates/component.tsx`;\
不要在 body 中内联大段代码或文档——放到对应子目录的文件中,body 只写步骤和引用路径。";

/// 评审器 system：判断草稿是否一个足够通用、可复用的好技能。
pub const CRITIC_SYSTEM: &str = "你是技能评审器。判断给定技能草稿是否一个足够通用、可复用、安全的好技能。\
只输出一个 JSON 对象,不要解释、不要围栏:\n\
{\"approve\":true|false,\"reason\":\"一句话理由\"}\n\
拒绝条件:过于具体只适用一次、description 空洞、含危险/破坏性操作而无防护、与常识矛盾。";

/// 判重器 system：判断新技能草稿是否与已有技能清单中某一项语义重复(同一类事/适用场景重叠/
/// 仅操作粒度或说法不同也算重复),命中则返回该已有技能名以便合并升级而非新建。
pub const DEDUP_SYSTEM: &str = "你是技能查重器。给你一份新技能草稿和一份已有技能清单,\
判断这份草稿是否与清单里某个已有技能做的是同一类事、适用场景重叠——\
哪怕只是操作粒度不同、换了说法、名字不同,只要本质是同一件事就算重复。\
只输出一个 JSON 对象,不要解释、不要代码围栏:\n\
{\"duplicate_of\":\"命中的已有技能名(必须原样来自清单),不重复则填 null\",\"reason\":\"一句话理由\"}\n\
宁可判为重复以合并升级,也不要制造功能雷同的新技能。";

/// 整合器 system：巡检整份已有技能清单,把"做同一类事/适用场景重叠"的聚成冗余组,
/// 每组指定一个保留名(canonical,选覆盖最全/最通用者),其余作为 duplicates 并入后归档。
pub const CONSOLIDATE_SYSTEM: &str = "你是技能库整理器。给你一份伙伴已有的技能清单,\
请找出其中\"做的是同一类事、适用场景高度重叠\"的技能,把它们聚成若干冗余组——\
哪怕名字不同、操作粒度或说法不同,只要本质是同一件事就该合并。\
每个冗余组挑一个覆盖最全/最通用的技能名作为 canonical 保留,其余作为 duplicates(都要并入它后归档)。\
只把真正冗余的放进组里;彼此不同的技能不要硬凑。名字必须原样来自清单。\
只输出一个 JSON 对象,不要解释、不要代码围栏:\n\
{\"groups\":[{\"canonical\":\"保留的技能名\",\"duplicates\":[\"并入并归档的技能名\"]}]}\n\
没有可合并的就输出 {\"groups\":[]}。";

/// 合并/演化 system：给定一个已有技能正文和一份新证据,产出改进后的同名技能(升版本)。
/// 若原技能是多轮/迭代方法论,务必保留其分阶段结构、迭代循环与决策点(勿压成线性清单)。
pub const MERGE_SYSTEM: &str = "你是技能演化器。已有一个技能,又观察到相关的新做法。\
把两者合并成一份**改进版**技能,保留原优点、补充新步骤、去重。\
若该技能本质是多轮/迭代方法论,务必保留其分阶段结构、迭代循环与决策点,不要压成一次做完的线性清单。\
只输出一个 JSON 对象,不要解释、不要围栏:\n\
{\"name\":\"沿用原 kebab-case 名\",\"description\":\"一句话说明(必填,非空)\",\"when_to_use\":\"何时用\",\
\"body\":\"改进后的 markdown 操作手册,首行保留 `运行方式：…` 标记\"}";

/// 起草提示：给模型工具序列 + 真实操作转录(已脱敏,可空),要它产出技能字段。
pub fn build_draft_prompt(p: &MinedPattern, transcript: &[String]) -> String {
    let steps = p.steps.join(" → ");
    let mut s = format!(
        "主人在 {} 个不同会话里反复做了这套 {} 步操作(共 {} 次):\n{}\n\n",
        p.distinct_sessions,
        p.steps.len(),
        p.count,
        steps
    );
    if !transcript.is_empty() {
        let turns = transcript.len();
        s.push_str(&format!(
            "这是其中一次的实际操作过程(已脱敏,共 {turns} 条往返;据此提炼可复用的做法,不要照抄一次性细节):\n"
        ));
        for r in transcript.iter().take(160) {
            s.push_str("- ");
            s.push_str(r);
            s.push('\n');
        }
        s.push('\n');
        if turns >= 8 {
            s.push_str(
                "上面的往返较多,说明这是一个需多轮收集/探索/迭代才能做好的任务——\
请按 system 要求写成分阶段方法论并保留迭代与决策点,不要压成一次做完的清单。\n\n",
            );
        }
    }
    s.push_str("把它固化成一个可复用技能。按 system 要求只输出 JSON。");
    s
}

/// 评审提示：给模型草稿 + 来源套路。
pub fn build_critic_prompt(d: &DraftOutput, p: &MinedPattern) -> String {
    format!(
        "技能草稿:\nname: {}\ndescription: {}\nwhen_to_use: {}\nbody:\n{}\n\n来源:主人在 {} 个会话重复了 {} 次。\n按 system 要求只输出 JSON 裁决。",
        d.name,
        d.description,
        d.when_to_use.as_deref().unwrap_or(""),
        d.body,
        p.distinct_sessions,
        p.count
    )
}

/// 合并提示：给模型已有技能正文 + 新证据,要它产出改进版。
pub fn build_merge_prompt(existing_body: &str, draft: &DraftOutput, p: &MinedPattern) -> String {
    format!(
        "已有技能正文:\n{}\n\n新观察到的相关做法(步骤: {}):\n{}\n\n请合并成改进版(沿用原名),按 system 要求只输出 JSON。",
        existing_body,
        p.steps.join(" → "),
        draft.body
    )
}

/// 判重提示：给模型新草稿字段 + 已有技能清单(名字/做什么/何时用)。
pub fn build_dedup_prompt(draft: &DraftOutput, existing: &[SkillDigest]) -> String {
    let mut s = format!(
        "新技能草稿:\nname: {}\ndescription: {}\nwhen_to_use: {}\n\n已有技能清单:\n",
        draft.name,
        draft.description,
        draft.when_to_use.as_deref().unwrap_or("")
    );
    for d in existing {
        s.push_str(&format!(
            "- name: {} | 做什么: {} | 何时用: {}\n",
            d.name,
            d.description,
            d.when_to_use.as_deref().unwrap_or("")
        ));
    }
    s.push_str("\n按 system 要求只输出 JSON 裁决。");
    s
}

/// 解析判重器输出。
pub fn parse_dedup_output(raw: &str) -> Result<DedupVerdict, String> {
    let cleaned = extract_json_object(raw).ok_or_else(|| "no JSON object found in dedup output".to_owned())?;
    serde_json::from_str(cleaned).map_err(|e| format!("invalid dedup JSON: {e}"))
}

/// 整合巡检提示：列出全部已有技能(名字/做什么/何时用),要模型聚出冗余组。
pub fn build_consolidate_prompt(existing: &[SkillDigest]) -> String {
    let mut s = String::from("伙伴已有的技能清单:\n");
    for d in existing {
        s.push_str(&format!(
            "- name: {} | 做什么: {} | 何时用: {}\n",
            d.name,
            d.description,
            d.when_to_use.as_deref().unwrap_or("")
        ));
    }
    s.push_str("\n请找出语义冗余的技能并按 system 要求只输出 JSON 分组。");
    s
}

/// 解析整合器输出（容忍围栏/散文）。
pub fn parse_consolidate_output(raw: &str) -> Result<ConsolidatePlan, String> {
    let cleaned = extract_json_object(raw).ok_or_else(|| "no JSON object found in consolidate output".to_owned())?;
    serde_json::from_str(cleaned).map_err(|e| format!("invalid consolidate JSON: {e}"))
}

/// 解析起草器输出（容忍围栏/散文）。
pub fn parse_draft_output(raw: &str) -> Result<DraftOutput, String> {
    let cleaned = extract_json_object(raw).ok_or_else(|| "no JSON object found in draft output".to_owned())?;
    serde_json::from_str(cleaned).map_err(|e| format!("invalid draft JSON: {e}"))
}

/// 解析评审器输出。
pub fn parse_critic_output(raw: &str) -> Result<CriticVerdict, String> {
    let cleaned = extract_json_object(raw).ok_or_else(|| "no JSON object found in critic output".to_owned())?;
    serde_json::from_str(cleaned).map_err(|e| format!("invalid critic JSON: {e}"))
}

/// 抽最外层 `{...}`（与 `crate::prompt::extract_json_object` 同语义）。
fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(&raw[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_fenced_draft() {
        let plain = r#"{"name":"weekly-report","description":"汇总周报","when_to_use":"周五","body":"步骤:\n1. 收集"}"#;
        let d = parse_draft_output(plain).unwrap();
        assert_eq!(d.name, "weekly-report");
        assert_eq!(d.description, "汇总周报");

        let fenced = format!("好的：\n```json\n{plain}\n```\n以上。");
        let d2 = parse_draft_output(&fenced).unwrap();
        assert_eq!(d2.name, "weekly-report");
    }

    #[test]
    fn empty_description_draft_still_parses() {
        // 解析层不拒空 description（由 create_skill/critic 后续拒绝），仅保证可解析。
        let d = parse_draft_output(r#"{"name":"x","description":"","body":"y"}"#).unwrap();
        assert_eq!(d.description, "");
    }

    #[test]
    fn malformed_draft_errors() {
        assert!(parse_draft_output("not json at all").is_err());
        assert!(parse_draft_output(r#"{"name": }"#).is_err());
    }

    #[test]
    fn parses_critic_verdict() {
        let approve = parse_critic_output(r#"{"approve":true,"reason":"通用"}"#).unwrap();
        assert!(approve.approve);
        let reject = parse_critic_output("裁决如下 {\"approve\":false} 完毕").unwrap();
        assert!(!reject.approve);
        // 缺字段走 serde default → approve=false
        let missing = parse_critic_output(r#"{"reason":"x"}"#).unwrap();
        assert!(!missing.approve);
    }

    #[test]
    fn build_prompts_include_steps() {
        let p = MinedPattern {
            signature: "grep\u{1f}read".into(),
            steps: vec!["grep".into(), "read".into()],
            count: 4,
            distinct_sessions: 3,
            example_event_ids: vec![],
            anchor: Default::default(),
        };
        let dp = build_draft_prompt(&p, &["在仓库里查 TODO".to_string()]);
        assert!(dp.contains("grep → read"));
        assert!(dp.contains("3 个不同会话"));
        let d = DraftOutput { name: "x".into(), description: "d".into(), when_to_use: None, body: "b".into() };
        let cp = build_critic_prompt(&d, &p);
        assert!(cp.contains("name: x"));
    }

    #[test]
    fn draft_prompt_flags_multi_round_when_transcript_is_long() {
        let p = MinedPattern {
            signature: "grep\u{1f}read".into(),
            steps: vec!["grep".into(), "read".into()],
            count: 4,
            distinct_sessions: 3,
            example_event_ids: vec![],
            anchor: Default::default(),
        };
        // 短转录：不追加多轮提示。
        let short: Vec<String> = (0..3).map(|i| format!("第 {i} 步")).collect();
        let dp_short = build_draft_prompt(&p, &short);
        assert!(dp_short.contains("共 3 条往返"));
        assert!(!dp_short.contains("分阶段方法论"));
        // 长转录（≥8 条往返）：追加多轮方法论提示，并如实报告往返数。
        let long: Vec<String> = (0..12).map(|i| format!("第 {i} 步")).collect();
        let dp_long = build_draft_prompt(&p, &long);
        assert!(dp_long.contains("共 12 条往返"));
        assert!(dp_long.contains("分阶段方法论"));
    }

    #[test]
    fn parses_plain_and_fenced_dedup() {
        let hit = parse_dedup_output(r#"{"duplicate_of":"market-briefing","reason":"同一类事"}"#).unwrap();
        assert_eq!(hit.duplicate_of.as_deref(), Some("market-briefing"));
        let fenced = "裁决:\n```json\n{\"duplicate_of\":null}\n```\n完毕。";
        let miss = parse_dedup_output(fenced).unwrap();
        assert!(miss.duplicate_of.is_none());
        // 缺字段 → serde default → duplicate_of=None
        let empty = parse_dedup_output(r#"{"reason":"x"}"#).unwrap();
        assert!(empty.duplicate_of.is_none());
        assert!(parse_dedup_output("not json").is_err());
    }

    #[test]
    fn build_dedup_prompt_lists_draft_and_existing() {
        let d = DraftOutput {
            name: "a-share-analysis".into(),
            description: "分析 A 股市场".into(),
            when_to_use: Some("盘后".into()),
            body: "b".into(),
        };
        let existing = vec![SkillDigest {
            name: "market-briefing".into(),
            description: "汇总市场信息".into(),
            when_to_use: None,
        }];
        let dp = build_dedup_prompt(&d, &existing);
        assert!(dp.contains("a-share-analysis"));
        assert!(dp.contains("market-briefing"));
        assert!(dp.contains("汇总市场信息"));
    }

    #[test]
    fn parses_plain_and_fenced_consolidate() {
        let plan = parse_consolidate_output(
            r#"{"groups":[{"canonical":"market-briefing","duplicates":["a-share-analysis","market-scan"]}]}"#,
        )
        .unwrap();
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].canonical, "market-briefing");
        assert_eq!(plan.groups[0].duplicates, vec!["a-share-analysis", "market-scan"]);
        // 围栏 + 散文
        let fenced = "规划:\n```json\n{\"groups\":[]}\n```\n完毕。";
        assert!(parse_consolidate_output(fenced).unwrap().groups.is_empty());
        // 缺字段 → serde default → 空
        assert!(parse_consolidate_output(r#"{}"#).unwrap().groups.is_empty());
        assert!(parse_consolidate_output("not json").is_err());
    }

    #[test]
    fn build_consolidate_prompt_lists_all_existing() {
        let existing = vec![
            SkillDigest { name: "market-briefing".into(), description: "汇总市场信息".into(), when_to_use: None },
            SkillDigest { name: "a-share-analysis".into(), description: "分析 A 股".into(), when_to_use: Some("盘后".into()) },
        ];
        let cp = build_consolidate_prompt(&existing);
        assert!(cp.contains("market-briefing"));
        assert!(cp.contains("a-share-analysis"));
        assert!(cp.contains("分析 A 股"));
    }
}
