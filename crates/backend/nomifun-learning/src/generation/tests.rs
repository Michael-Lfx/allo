    use super::activities::validate_lesson_activities;
    use super::completer::complete_with_timeout;
    use super::lesson::{
        build_activities_prompt, build_section_rewrite_prompt, enforce_grounding_budget,
        render_grounding_fragments, section_body_system, validate_lesson_document,
        GROUNDING_PRIMARY_MAX_BYTES, GROUNDING_SUPPLEMENT_MAX_BYTES, GROUNDING_TOTAL_MAX_BYTES,
    };
    use super::parser::{strip_code_fences, strip_markdown_fences};
    use super::*;
    use crate::lesson_draft::{ExcerptRole, LessonExcerpt};
    use crate::models::{SectionOutline, validate_section_outline, ComplexityTier, SectionKind, SectionPack};
    use nomifun_common::KnowledgeBaseId;
    use serde_json::json;

    /// A document long enough to pass the length floor with all three
    /// required sections present in order.
    fn long_document() -> String {
        format!(
            "## 描述\n{}\n## 例子\n{}\n## 验证\n{}",
            "这是描述正文，说明本课讲什么。".repeat(80),
            "这是例子正文，带步骤和数字。".repeat(80),
            "请回答自检问题验证理解。".repeat(80)
        )
    }
    #[test]
    fn blueprint_prompt_marks_samples_and_keeps_exact_paths() {
        let prompt = build_blueprint_prompt(
            "ICT",
            "Trading course",
            Some("trading"),
            None,
            &[("lessons/liquidity.md".into(), "# Liquidity\nText".into())],
        );
        assert!(prompt.contains("--- FILE: lessons/liquidity.md ---"));
        assert!(prompt.contains("Requested domain label: trading"));
        assert!(prompt.contains("Course size is yours to decide"));
        assert!(!prompt.contains("Learner goal"), "no goal → no goal block");
        assert!(BLUEPRINT_SYSTEM.contains("untrusted source material"));
        assert!(BLUEPRINT_SYSTEM.contains("Never invent paths"));
    }

    /// t1：知识库流带目标时，蓝图提示词把目标文本作为方向锚点与标题口径
    /// 交给模型（整体文本即目标）。
    #[test]
    fn blueprint_prompt_carries_the_learner_goal() {
        let prompt = build_blueprint_prompt(
            "ICT",
            "Trading course",
            None,
            Some("零基础学 Python 数据分析"),
            &[("a.md".into(), "# A".into())],
        );
        assert!(prompt.contains("Learner goal"), "{prompt}");
        assert!(prompt.contains("零基础学 Python 数据分析"), "{prompt}");
    }

    #[test]
    fn retry_prompts_never_shrink_output() {
        // Regression guard: retries must ask for corrections, never smaller
        // JSON — the old "smaller" instruction caused thin summaries.
        assert!(!BLUEPRINT_SYSTEM.contains("smaller"));
        assert!(!LESSON_SYSTEM.contains("smaller"));
    }

    /// t2 丰富度条款在提示词面：objective 题必带证据回引；choice 题解析逐
    /// 选项辨析；节正文篇幅纪律 = 档位预算是展开下限不是上限。
    #[test]
    fn richness_clauses_are_present_in_generation_prompts() {
        assert!(LESSON_SYSTEM.contains("\"evidence\""));
        assert!(LESSON_SYSTEM.contains("walk EVERY option"));
        assert!(LESSON_ACTIVITY_SYSTEM.contains("\"evidence\""));
        assert!(LESSON_ACTIVITY_SYSTEM.contains("verbatim quote"));
        let standard = section_body_system(crate::models::TeachingStyle::Standard);
        assert!(
            standard.contains("THE BUDGET IS THE FLOOR OF A FULLY-WORKED SECTION, NOT A CEILING"),
            "prose budget must be framed as an expansion floor"
        );
    }

    #[test]
    fn section_outline_guardrails_block_direction_extremes() {
        let section = |key: &str| SectionPack {
            section_key: key.into(),
            kind: SectionKind::Concept,
            title: format!("概念：{key}"),
            points: String::new(),
            visual: "公式".into(),
            body_md: String::new(),
        };
        // Every valid outline closes with exactly one practice section.
        let with_practice = |mut sections: Vec<SectionPack>| {
            sections.push(SectionPack {
                section_key: format!("s{}", sections.len() + 1),
                kind: SectionKind::Practice,
                title: "练习：巩固".into(),
                points: String::new(),
                visual: String::new(),
                body_md: String::new(),
            });
            sections
        };
        // Empty outline.
        let empty = SectionOutline { tier: None, sections: Vec::new() };
        assert!(validate_section_outline(&empty).is_err());
        // Hard cap at 8.
        let nine: Vec<SectionPack> = (1..=8).map(|index| section(&format!("s{index}"))).collect();
        let over = SectionOutline { tier: Some(ComplexityTier::Mid), sections: with_practice(nine) };
        assert!(validate_section_outline(&over).is_err());
        // High tier with only 2 sections is directionally wrong.
        let two = SectionOutline {
            tier: Some(ComplexityTier::High),
            sections: with_practice(vec![section("s1")]),
        };
        assert!(validate_section_outline(&two).is_err());
        // Low tier tolerates overrun up to 7 (guardrails only block
        // direction-level extremes), 8+ is the hard cap.
        let six = SectionOutline {
            tier: Some(ComplexityTier::Low),
            sections: with_practice((1..=5).map(|index| section(&format!("s{index}"))).collect()),
        };
        assert!(validate_section_outline(&six).is_ok());
        // Mid tier with 3 sections passes; duplicate keys fail.
        let three = SectionOutline {
            tier: Some(ComplexityTier::Mid),
            sections: with_practice(vec![section("s1"), section("s2")]),
        };
        assert!(validate_section_outline(&three).is_ok());
        let duplicate = SectionOutline {
            tier: Some(ComplexityTier::Mid),
            sections: vec![section("s1"), section("s1")],
        };
        assert!(validate_section_outline(&duplicate).is_err());

        // The closing practice section is mandatory: no practice at all, or
        // practice placed before the end, or two practice sections all fail.
        let no_practice = SectionOutline {
            tier: Some(ComplexityTier::Mid),
            sections: vec![section("s1"), section("s2")],
        };
        assert!(validate_section_outline(&no_practice).is_err());
        let practice = SectionPack {
            section_key: "sp".into(),
            kind: SectionKind::Practice,
            title: "练习：巩固".into(),
            points: String::new(),
            visual: String::new(),
            body_md: String::new(),
        };
        let practice_first = SectionOutline {
            tier: Some(ComplexityTier::Mid),
            sections: vec![practice.clone(), section("s1")],
        };
        assert!(validate_section_outline(&practice_first).is_err());
        let two_practices = SectionOutline {
            tier: Some(ComplexityTier::Mid),
            sections: vec![section("s1"), practice.clone(), practice],
        };
        assert!(validate_section_outline(&two_practices).is_err());
    }

    #[test]
    fn section_body_validation_enforces_kind_rules() {
        let prose = "这是一个用于测试的完整段落，覆盖本节要点并且足够长。".repeat(20);
        let visual_prose = format!("$$\\vec{{a}}\\cdot\\vec{{b}} = a_x b_x + a_y b_y$$\n{prose}");
        // Concept with a promised visual delivers it: passes.
        let concept = SectionPack {
            section_key: "s1".into(),
            kind: SectionKind::Concept,
            title: "概念：向量".into(),
            points: String::new(),
            visual: "公式".into(),
            body_md: visual_prose.clone(),
        };
        assert!(concept.validate_body().is_ok());
        // Prose-only concept fails the visual-first gate...
        let prose_only = SectionPack { body_md: prose.clone(), ..concept.clone() };
        assert!(prose_only.validate_body().is_err());
        // ...unless the outline explicitly declared 无.
        let text_exempt = SectionPack { visual: "无".into(), ..prose_only.clone() };
        assert!(text_exempt.validate_body().is_ok());
        // Too short fails.
        let short = SectionPack { body_md: "太短。".into(), ..concept.clone() };
        assert!(short.validate_body().is_err());
        // ### sub-headings are banned inside a section.
        let subheaded = SectionPack { body_md: format!("{visual_prose}\n### 小标题\n{prose}"), ..concept.clone() };
        assert!(subheaded.validate_body().is_err());
        // Demo without a visualization block fails; with one passes.
        let demo_plain = SectionPack { kind: SectionKind::Demo, body_md: prose.clone(), ..concept.clone() };
        assert!(demo_plain.validate_body().is_err());
        let demo_visual = SectionPack {
            kind: SectionKind::Demo,
            body_md: format!("旁注。\n```svg\n<svg viewBox=\"0 0 10 10\"></svg>\n```\n{prose}"),
            ..concept.clone()
        };
        assert!(demo_visual.validate_body().is_ok());
        // Practice stays a guidance stub; a full question set is rejected.
        let practice_ok = SectionPack {
            kind: SectionKind::Practice,
            body_md: "本节目标：能用向量加法解决位移合成问题；作答时先写出两段位移，再按首尾相接的规则合成，注意方向相反时先取差。".into(),
            ..concept.clone()
        };
        assert!(practice_ok.validate_body().is_ok());
        let practice_overgrown = SectionPack {
            kind: SectionKind::Practice,
            body_md: prose,
            ..concept
        };
        assert!(practice_overgrown.validate_body().is_err());
    }

    #[tokio::test]
    async fn complete_times_out_on_a_hung_model_call() {
        // A stalled LLM endpoint must surface a `Timeout` instead of leaving
        // the job stuck in `lessons` forever with no error.
        struct HungCompleter;
        #[async_trait::async_trait]
        impl LearningCompleter for HungCompleter {
            async fn complete(
                &self,
                _model_override: Option<(&str, &str)>,
                _system: &str,
                _user: &str,
                _max_tokens: u32,
            ) -> Result<String, AppError> {
                std::future::pending().await
            }
        }
        let completer = HungCompleter;
        let error = complete_with_timeout(
            &completer,
            None,
            "system",
            "user",
            4096,
            std::time::Duration::from_millis(50),
        )
        .await
        .expect_err("a hung call must time out");
        assert!(
            matches!(error, AppError::Timeout(_)),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn parser_accepts_fenced_json_and_rejects_non_json() {
        let raw = r#"```json
        {
          "title": "Course",
          "modules": [{"title": "M", "lessons": [{"title": "L"}]}]
        }
        ```"#;
        let blueprint: Blueprint = parse_json_object(raw).unwrap();
        assert_eq!(blueprint.title, "Course");
        assert_eq!(blueprint.modules.len(), 1);
        assert!(parse_json_object::<Blueprint>("not json").is_err());
    }

    #[test]
    fn lesson_parse_tolerates_null_strings_and_lists() {
        // LLMs habitually emit explicit nulls (the reflection answer is null
        // by design); when one lands on a string/list field the parse must
        // degrade instead of failing the whole lesson, leaving validation to
        // judge the degraded values like any other weak output.
        let raw = r#"{
          "summary": "s",
          "estimated_minutes": null,
          "activities": [
            {
              "kind": "reflection",
              "prompt": "p",
              "options": null,
              "answer": null,
              "explanation": null,
              "distractors": null
            }
          ]
        }"#;
        let output: LessonOutput = parse_json_object(raw).expect("nulls must degrade, not fail");
        assert_eq!(output.summary, "s");
        assert_eq!(output.estimated_minutes, 10);
        let activity = &output.activities[0];
        assert_eq!(activity.prompt, "p");
        assert!(activity.options.is_empty());
        assert!(activity.answer.is_null());
        assert_eq!(activity.explanation, "");
        assert!(activity.distractors.is_empty());
    }

    #[test]
    fn lesson_parse_still_rejects_wrong_primitive_types() {
        // Tolerance covers null only — a number where a string belongs is a
        // different mistake and must still fail loudly so the retry fires.
        let raw = r#"{"summary": "s", "activities": [{"kind": "reflection", "prompt": 42}]}"#;
        assert!(parse_json_object::<LessonOutput>(raw).is_err());
    }

    #[test]
    fn parser_tolerates_latex_commands_and_raw_control_chars() {
        // LaTeX backslashes are not valid JSON escapes (`\a`, `\{`, `\m`)
        // and raw newlines are invalid inside JSON strings; both must be
        // repaired while the literal text is preserved.
        let raw = "{\"title\": \"集合论\",\n  \"description\": \"公式 $\\alpha + \\beta$，分数 \\frac{a}{b}，集合 \\{x \\mid x > 0\\}，\n换行说明\",\n  \"modules\": []}";
        let blueprint: Blueprint = parse_json_object(raw).unwrap();
        assert_eq!(blueprint.title, "集合论");
        assert!(blueprint.description.contains(r"\alpha"));
        assert!(blueprint.description.contains(r"\frac{a}{b}"));
        assert!(blueprint.description.contains(r"\{x \mid x > 0\}"));
        assert!(blueprint.description.contains("\n换行说明"));
    }

    #[test]
    fn parser_skips_braces_inside_strings_and_prose() {
        // A string value ending in `}` and prose with stray braces must not
        // truncate or poison the candidate object.
        let raw = r#"请按 {要求} 输出：{"title": "集合 {1,2,3}", "modules": []} 完成（见 } 处）"#;
        let blueprint: Blueprint = parse_json_object(raw).unwrap();
        assert_eq!(blueprint.title, "集合 {1,2,3}");
        assert!(blueprint.modules.is_empty());
    }

    #[test]
    fn parser_keeps_escaped_math_in_lesson_summary() {
        // `\frac` would otherwise parse as a form-feed escape; the repaired
        // summary must keep the literal LaTeX command.
        let raw = r#"{"summary": "能量 \frac{1}{2}mv^2，\begin{matrix}...\end{matrix}", "estimated_minutes": 10, "activities": []}"#;
        let output: LessonOutput = parse_json_object(raw).unwrap();
        assert!(output.summary.contains(r"\frac{1}{2}mv^2"));
        assert!(output.summary.contains(r"\begin{matrix}"));
        assert_eq!(output.estimated_minutes, 10);
    }

    #[test]
    fn activity_stage_enforces_count_floors_and_ai_graded_cap() {
        let objective = vec![
            ActivityPack {
                kind: ActivityKind::SingleChoice,
                prompt: "q1".into(),
                options: vec!["A".into(), "B".into(), "C".into()],
                answer: json!("A"),
                explanation: "e".into(),
                distractors: Vec::new(),
                tol: None,
                section_key: None,
                difficulty: None,
                evidence: Some("「q1 的证据回引」".into()),
            },
            ActivityPack {
                kind: ActivityKind::TrueFalse,
                prompt: "q2".into(),
                options: Vec::new(),
                answer: json!(true),
                explanation: "e".into(),
                distractors: Vec::new(),
                tol: None,
                section_key: None,
                difficulty: None,
                evidence: Some("「q2 的证据回引」".into()),
            },
        ];
        let reflection = |prompt: &str| ActivityPack {
            kind: ActivityKind::Reflection,
            prompt: prompt.into(),
            options: Vec::new(),
            answer: json!(null),
            explanation: String::new(),
            distractors: Vec::new(),
            tol: None,
            section_key: None,
                difficulty: None,
evidence: None,
};

        // Up to three AI-graded questions pass (validate_shape gate each).
        let mut at_cap = objective.clone();
        at_cap.push(reflection("r1"));
        at_cap.push(reflection("r2"));
        at_cap.push(reflection("r3"));
        for activity in &at_cap {
            activity.validate_shape((3, 5), true).unwrap();
        }
        assert!(validate_lesson_activities(&at_cap).is_ok());

        // A fourth AI-graded question exceeds the cap.
        let mut over_cap = at_cap.clone();
        over_cap.push(reflection("r4"));
        assert!(validate_lesson_activities(&over_cap).is_err());

        // Objective floors: dropping one objective question breaks the
        // minimum objective count.
        let mut under_floor = at_cap.clone();
        under_floor.remove(0);
        assert!(validate_lesson_activities(&under_floor).is_err());
    }

    /// 证据回引硬门（t2）：objective 题缺/空 evidence = 违约（重试反馈点名）；
    /// reflection/open_question 不要求。
    #[test]
    fn activity_stage_requires_evidence_on_objective_questions() {
        let mut bare = ActivityPack {
            kind: ActivityKind::SingleChoice,
            prompt: "q".into(),
            options: vec!["A".into(), "B".into(), "C".into()],
            answer: json!("A"),
            explanation: "e".into(),
            distractors: Vec::new(),
            tol: None,
            section_key: None,
            difficulty: None,
            evidence: None,
        };
        let reflection = ActivityPack {
            kind: ActivityKind::Reflection,
            prompt: "r".into(),
            options: Vec::new(),
            answer: json!(null),
            explanation: String::new(),
            distractors: Vec::new(),
            tol: None,
            section_key: None,
            difficulty: None,
            evidence: None,
        };
        let mut set = vec![bare.clone(), bare.clone(), reflection];
        let missing = validate_lesson_activities(&set).unwrap_err();
        assert!(missing.contains("evidence"), "{missing}");
        bare.evidence = Some("  ".into());
        set[0] = bare.clone();
        let blank = validate_lesson_activities(&set).unwrap_err();
        assert!(blank.contains("evidence"), "blank evidence also rejected: {blank}");
        bare.evidence = Some("「证据回引」".into());
        set[0] = bare.clone();
        set[1] = bare;
        assert!(validate_lesson_activities(&set).is_ok());
    }

    #[test]
    fn fill_in_blank_rules_pin_blank_answers_and_distractors() {
        // The lesson-stage standard embeds the fill-in-the-blank design rules:
        // a single ___ blank, 1-3 convergent answers, near-synonym distractors.
        assert!(LESSON_SYSTEM.contains("\"___\" blank"));
        assert!(LESSON_SYSTEM.contains("1-3 equivalent accepted answers"));
        assert!(LESSON_SYSTEM.contains("near-synonym traps"));

        let base = ActivityPack {
            kind: ActivityKind::FillInBlank,
            prompt: "A vector has ___ and direction.".into(),
            options: Vec::new(),
            answer: json!(["magnitude"]),
            explanation: "e".into(),
            distractors: vec!["length".into(), "norm".into()],
            tol: None,
            section_key: None,
                difficulty: None,
evidence: None,
};
        assert!(base.validate_shape((3, 5), true).is_ok());

        let no_blank = ActivityPack { prompt: "A vector is a quantity.".into(), ..base.clone() };
        assert!(no_blank.validate_shape((3, 5), true).is_err());

        let wrong_answer = ActivityPack { answer: json!("magnitude"), ..base.clone() };
        assert!(wrong_answer.validate_shape((3, 5), true).is_err());

        let empty_answers = ActivityPack { answer: json!([]), ..base.clone() };
        assert!(empty_answers.validate_shape((3, 5), true).is_err());

        let too_many_answers = ActivityPack { answer: json!(["a", "b", "c", "d"]), ..base.clone() };
        assert!(too_many_answers.validate_shape((3, 5), true).is_err());

        // Generation requires distractors; manual authoring does not.
        let no_distractors = ActivityPack { distractors: vec![" ".into()], ..base.clone() };
        assert!(no_distractors.validate_shape((3, 5), true).is_err());
        assert!(no_distractors.validate_shape((2, 5), false).is_ok());
    }

    #[test]
    fn generated_request_keeps_selected_knowledge_base() {
        let id = KnowledgeBaseId::new();
        let request = GenerateCourseRequest {
            course_kind: crate::models::CourseKind::Traditional,
            proficiency_statement: None,
            learner_basis: None,
            teaching_style: None,
            knowledge_base_id: Some(id.clone()),
            description: None,
            domain: None,
            provider_id: None,
            model: None,
            mode: crate::models::CourseGenerationMode::OnDemand,
            goal: None,
            knowledge_base_ids: Vec::new(),
        };
        assert_eq!(request.knowledge_base_id, Some(id));
    }

    /// A minimal blueprint with one module and one lesson.
    fn lesson_test_blueprint() -> Blueprint {
        Blueprint {
            title: "C".into(),
            description: String::new(),
            domain: String::new(),
            version: 1,
            modules: vec![BlueprintModule {
                title: "M".into(),
                description: String::new(),
                lessons: vec![BlueprintLesson {
                    title: "L".into(),
                    purpose: "p".into(),
                    source: Some(SourceSpan {
                        path: "real.md".into(),
                        start: None,
                        end: None,
                        kb_id: None,
                    }),
                    sources: Vec::new(),
                    anchor: None,
                }],
            }],
        }
    }

    /// A two-module × two-lesson blueprint for the outline-tree / adjacent
    /// context renderers: global lesson order 一→二→三→四, the second lesson
    /// carries a sampled source.
    fn two_by_two_blueprint() -> Blueprint {
        let lesson = |title: &str, purpose: &str, source: Option<&str>| BlueprintLesson {
            title: title.into(),
            purpose: purpose.into(),
            source: source.map(|path| SourceSpan {
                path: path.into(),
                start: None,
                end: None,
                kb_id: None,
            }),
            sources: Vec::new(),
            anchor: None,
        };
        Blueprint {
            title: "C".into(),
            description: String::new(),
            domain: String::new(),
            version: 1,
            modules: vec![
                BlueprintModule {
                    title: "模块一".into(),
                    description: String::new(),
                    lessons: vec![
                        lesson("第一课", "目标一", None),
                        lesson("第二课", "目标二", Some("docs/two.md")),
                    ],
                },
                BlueprintModule {
                    title: "模块二".into(),
                    description: String::new(),
                    lessons: vec![
                        lesson("第三课", "目标三", None),
                        lesson("第四课", "目标四", None),
                    ],
                },
            ],
        }
    }

    /// The outline tree lists EVERY lesson of the course (global numbering)
    /// and marks exactly the current one — the model's anti-duplication map.
    #[test]
    fn outline_tree_lists_every_lesson_and_marks_the_current_one() {
        let blueprint = two_by_two_blueprint();
        let tree = build_outline_tree(&blueprint, 1, 0);
        assert!(tree.contains("模块 1/2：模块一"));
        assert!(tree.contains("模块 2/2：模块二"));
        assert!(tree.contains("  1. 第一课 — 目标一"));
        assert!(tree.contains("  3. 第三课 — 目标三（本课时）"));
        assert!(!tree.contains("目标四（本课时）"));
        assert!(!tree.contains("  5."));
    }

    /// Adjacent lessons: prev/next titles + purposes; kb-flow excerpts are
    /// truncated at the hard budget; the description flow has no excerpt lines.
    #[test]
    fn adjacent_context_names_neighbors_and_truncates_excerpts() {
        let blueprint = two_by_two_blueprint();
        // 1800 chars > the 1000-char budget; the tail marker must not survive.
        let sample = format!("{}尾部标记", "第二课原文。".repeat(300));
        let samples = vec![("docs/two.md".to_owned(), sample)];
        let context = build_adjacent_context(&blueprint, &samples, 1, 0);
        assert!(context.contains("相邻课时参考"));
        assert!(context.contains("上一课时「第二课」— 目标二"));
        assert!(context.contains("下一课时「第四课」— 目标四"));
        assert!(context.contains("原文摘录（节选）"));
        assert!(context.contains("第二课原文"));
        assert!(!context.contains("尾部标记"), "excerpt must be truncated");

        // Description flow (no samples): titles and purposes only.
        let context = build_adjacent_context(&blueprint, &[], 1, 0);
        assert!(context.contains("上一课时「第二课」"));
        assert!(!context.contains("原文摘录"));
    }

    /// Returns canned responses in order while recording every call.
    struct ScriptedCompleter {
        script: std::sync::Mutex<Vec<String>>,
        calls: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl ScriptedCompleter {
        fn new(script: Vec<String>) -> Self {
            Self {
                script: std::sync::Mutex::new(script),
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl LearningCompleter for ScriptedCompleter {
        async fn complete(
            &self,
            _model_override: Option<(&str, &str)>,
            system: &str,
            user: &str,
            _max_tokens: u32,
        ) -> Result<String, AppError> {
            self.calls
                .lock()
                .unwrap()
                .push((system.to_owned(), user.to_owned()));
            Ok(self.script.lock().unwrap().remove(0))
        }
    }

    /// t3 多片段 grounding：主引用在前标角色与文件，检索补充在后带库归属与
    /// 命中章节——片段块是三处提示词共用的那一段。
    #[test]
    fn grounding_fragments_render_roles_and_provenance() {
        let excerpts = vec![
            LessonExcerpt {
                kb_id: None,
                kb_name: None,
                heading: None,
                ..LessonExcerpt::text_only("docs/two.md", "第二课原文。")
            },
            LessonExcerpt {
                path: "notes/聚类.md".into(),
                text: "K-means 的簇数 k 需要预先指定。".into(),
                kb_id: Some("kb-1".into()),
                kb_name: Some("数学笔记".into()),
                heading: Some("聚类分析要点".into()),
                role: ExcerptRole::Supplement,
            },
        ];
        let rendered = render_grounding_fragments(&excerpts);
        assert!(rendered.contains("主引用（1/2）文件 docs/two.md"), "{rendered}");
        assert!(rendered.contains("第二课原文。"), "{rendered}");
        assert!(
            rendered.contains("补充片段（2/2）文件 notes/聚类.md（知识库 数学笔记） 章节「聚类分析要点」"),
            "{rendered}"
        );
        assert!(rendered.contains("K-means 的簇数 k 需要预先指定。"), "{rendered}");
    }

    /// t3 预算守卫：primary 截到 24KB、补充单片段 8KB、合计 40KB，截断处写入
    /// 标记（模型看得见"后面还有"，不是静默丢弃）；预算内的片段逐字不动。
    /// 超预算的片段按词条相关度选摘——中段的相关事实不因落在预算外而丢。
    #[test]
    fn grounding_budget_truncates_and_marks_over_budget_fragments() {
        let primary = LessonExcerpt::text_only("docs/big.md", &"主".repeat(30 * 1024 / 3));
        let supplement = |name: &str, bytes: usize| LessonExcerpt {
            path: format!("notes/{name}.md"),
            text: "补".repeat(bytes / 3),
            kb_id: Some("kb-1".into()),
            kb_name: None,
            heading: None,
            role: ExcerptRole::Supplement,
        };
        let kept = enforce_grounding_budget(
            vec![
                primary,
                supplement("a", 10 * 1024),
                supplement("b", 10 * 1024),
                supplement("c", 10 * 1024),
            ],
            &[],
        );
        assert!(!kept.is_empty());
        assert_eq!(kept[0].role, ExcerptRole::Primary);
        assert!(
            kept[0].text.len() <= GROUNDING_PRIMARY_MAX_BYTES,
            "primary fits its budget: {}",
            kept[0].text.len()
        );
        assert!(kept[0].text.len() > GROUNDING_PRIMARY_MAX_BYTES - 200, "primary uses its budget");
        assert!(kept[0].text.contains("截断"), "truncation is marked in the text");
        let total: usize = kept.iter().map(|excerpt| excerpt.text.len()).sum();
        assert!(total <= GROUNDING_TOTAL_MAX_BYTES, "total budget holds: {total}");
        for excerpt in kept.iter().skip(1) {
            assert!(
                excerpt.text.len() <= GROUNDING_SUPPLEMENT_MAX_BYTES,
                "each supplement fits its budget: {}",
                excerpt.text.len()
            );
        }
        // 预算用尽后的片段被丢弃（不是截成空壳）。
        assert!(kept.len() < 4, "later fragments drop once the total budget is spent");
        assert!(
            kept.iter().all(|excerpt| !excerpt.text.trim().is_empty()),
            "no empty stub fragments"
        );

        // 预算内的片段原样交付：不加标记、不改一个字。
        let small = enforce_grounding_budget(
            vec![
                LessonExcerpt::text_only("a.md", "短文。"),
                {
                    let mut excerpt = supplement("b", 3 * 1024);
                    excerpt.text = "补充短文。".into();
                    excerpt
                },
            ],
            &[],
        );
        assert_eq!(small.len(), 2);
        assert_eq!(small[0].text, "短文。");
        assert_eq!(small[1].text, "补充短文。");
    }

    /// t3 超预算截断按词条相关度选摘（与 t2 采样同一把尺）：大文件中段的
    /// 相关事实不因落在预算之外而丢——头部噪声被让位，事实段落进来；无命中
    /// 词条时回退文件头，摘录绝不为空。
    #[test]
    fn grounding_budget_keeps_mid_file_relevant_facts_when_truncating() {
        let filler = "这是与任何词条都无关的填充段落，用来把相关事实推到大文件的中段。\n\n";
        let mut big = String::from("# 期权入门\n\n开头噪声标记：本节只讲名词解释。\n\n");
        for _ in 0..200 {
            big.push_str(filler);
        }
        big.push_str("## 权利义务不对称\n\n关键事实标记：买方持有权利、卖方承担义务。\n\n");
        for _ in 0..200 {
            big.push_str(filler);
        }
        assert!(big.len() > GROUNDING_PRIMARY_MAX_BYTES, "fixture must exceed the primary budget");

        let terms = vec![
            "权利".to_string(),
            "义务".to_string(),
            "期权".to_string(),
        ];
        let kept = enforce_grounding_budget(
            vec![LessonExcerpt::text_only("docs/大文件.md", &big)],
            &terms,
        );
        assert_eq!(kept.len(), 1);
        let text = &kept[0].text;
        assert!(
            text.contains("关键事实标记：买方持有权利、卖方承担义务"),
            "中段相关事实必须留下：{}",
            text.chars().take(200).collect::<String>()
        );
        assert!(text.len() <= GROUNDING_PRIMARY_MAX_BYTES, "stays inside the budget");
        assert!(text.contains("截断"), "truncation is marked");

        // 无命中词条：回退文件头，且绝不产出空片段。
        let head_only = enforce_grounding_budget(
            vec![LessonExcerpt::text_only("docs/大文件.md", &big)],
            &["quaternion".to_string()],
        );
        assert_eq!(head_only.len(), 1);
        assert!(
            head_only[0].text.starts_with("# 期权入门"),
            "无命中回退文件头：{}",
            head_only[0].text.chars().take(80).collect::<String>()
        );
        assert!(head_only[0].text.contains("截断"));
    }

    #[test]
    fn section_prompts_carry_task_manifest_and_grounding() {
        // The outline prompt embeds the outline tree, the concepts and asks
        // for the section-manifest JSON; the body prompt carries the section
        // task, the manifest and the previous body for coherence; the
        // activities prompt embeds every finished section body.
        let blueprint = lesson_test_blueprint();
        let module = &blueprint.modules[0];
        let lesson = &module.lessons[0];
        let grounding = render_grounding_fragments(&[LessonExcerpt::text_only("real.md", "# Real")]);
        let outline_prompt = super::lesson::build_section_outline_prompt(
            &blueprint, module, lesson, 0, 0, 1, &grounding,
        );
        assert!(outline_prompt.contains("Plan the section outline JSON now."));
        assert!(outline_prompt.contains("Cited grounding fragments"), "{outline_prompt}");
        assert!(outline_prompt.contains("--- 主引用（1/1）文件 real.md ---"), "{outline_prompt}");
        assert!(outline_prompt.contains("# Real"), "{outline_prompt}");

        let manifest = vec![SectionPack {
            section_key: "s1".into(),
            kind: SectionKind::Concept,
            title: "概念：向量".into(),
            points: "什么是向量".into(),
            visual: "公式".into(),
            body_md: String::new(),
        }];
        let planned = &manifest[0];
        let body_prompt = super::lesson::build_section_body_prompt(
            &blueprint,
            lesson,
            &grounding,
            planned,
            0,
            &manifest,
            None,
            Some("下一课"),
            ComplexityTier::Mid,
        );
        assert!(body_prompt.contains("## 本节任务"));
        assert!(body_prompt.contains("节类型：概念"));
        assert!(body_prompt.contains("Write this section's body now."));
        assert!(body_prompt.contains("主引用（1/1）文件 real.md"), "{body_prompt}");

        let finished = SectionPack {
            body_md: "向量的定义……".into(),
            ..manifest[0].clone()
        };
        let activities_prompt = build_activities_prompt(
            &blueprint,
            lesson,
            ComplexityTier::Mid,
            &manifest,
            &[finished.clone()],
            &grounding,
        );
        assert!(activities_prompt.contains("Complexity tier: mid"));
        assert!(activities_prompt.contains("--- SECTION s1 [概念] 概念：向量 ---"));
        assert!(activities_prompt.contains("Design the activity JSON now."));
        assert!(activities_prompt.contains("主引用（1/1）文件 real.md"), "{activities_prompt}");
    }

    #[test]
    fn document_validation_enforces_required_sections_in_order() {
        assert!(validate_lesson_document(&long_document()).is_ok());

        let missing = long_document().replace("\n## 例子\n", "\n");
        let error = validate_lesson_document(&missing).unwrap_err();
        assert!(error.contains("## 例子"), "missing middle section: {error}");

        // 例子 before 描述 breaks the required order.
        let wrong_order = format!(
            "## 例子\n{}\n## 描述\n{}\n## 验证\n{}",
            "这是例子正文。".repeat(300),
            "这是描述正文。".repeat(300),
            "这是验证正文。".repeat(300)
        );
        assert!(validate_lesson_document(&wrong_order).is_err());

        let short = "## 描述\n短。";
        let error = validate_lesson_document(short).unwrap_err();
        assert!(error.contains("non-whitespace characters"));
    }

    #[test]
    fn strip_code_fences_cuts_wrapper_only() {
        let wrapped = "```jsxgraph\nboard.create('point', [1, 2]);\n```";
        assert_eq!(strip_code_fences(wrapped), "board.create('point', [1, 2]);");
        assert_eq!(strip_code_fences("plain body"), "plain body");
    }

    #[test]
    fn parser_repairs_trailing_commas() {
        // Trailing commas before `}`/`]` are a habitual model mistake; they
        // must be repaired string-aware so commas inside string values stay.
        let raw = r#"{"title": "集合 {1, 2}", "modules": [{"title": "M", "lessons": [],},],}"#;
        let blueprint: Blueprint = parse_json_object(raw).unwrap();
        assert_eq!(blueprint.title, "集合 {1, 2}");
        assert_eq!(blueprint.modules.len(), 1);
        assert_eq!(blueprint.modules[0].title, "M");
        assert!(blueprint.modules[0].lessons.is_empty());
    }

    #[test]
    fn document_strip_cuts_preface_fences_and_trailing_prose() {
        let raw = "Here is the lesson you asked for:\n```markdown\n## 描述\n正文第一行。\n## 例子\n示例。\n## 验证\n问题。\n```\nHope this helps!";
        let doc = strip_markdown_fences(raw);
        assert!(!doc.contains("Here is the lesson"));
        assert!(!doc.contains("```"));
        assert!(!doc.contains("Hope this helps"));
        assert!(doc.starts_with("## 描述"));
        assert!(doc.ends_with("问题。"));
    }

    #[test]
    fn document_strip_keeps_paired_figure_fences() {
        // Paired ```svg / ```jsxgraph blocks are document content, even when
        // the document's final lines sit inside (or right after) them.
        let raw = "## 描述\n正文。\n```svg\n<svg viewBox=\"0 0 10 10\"></svg>\n```\n## 例子\n示例。\n```jsxgraph\nboard.create('point', [1, 2]);\n```\n下一课见。";
        let doc = strip_markdown_fences(raw);
        assert!(doc.contains("```svg"));
        assert!(doc.contains("```jsxgraph"));
        assert!(doc.ends_with("下一课见。"));
    }

    #[test]
    fn document_strip_cuts_trailing_prose_after_wrapper_fence_with_figures() {
        // Wrapper detection must survive internal fence pairs: the leftover
        // wrapper half is the last fence seen while outside any block.
        let raw = "## 描述\n正文。\n```svg\n<svg></svg>\n```\n## 验证\n问题。\n```\nEnjoy!";
        let doc = strip_markdown_fences(raw);
        assert!(doc.contains("```svg"));
        assert!(doc.ends_with("问题。"));
        assert!(!doc.contains("Enjoy!"));
    }

    fn outline_json() -> String {
        r#"{
          "tier": "mid",
          "sections": [
            {"section_key": "s1", "kind": "concept", "title": "概念：向量", "points": "什么是向量", "visual": "公式"},
            {"section_key": "s2", "kind": "practice", "title": "练习：向量辨析", "points": "统一练习轮"}
          ]
        }"#
        .into()
    }

    fn practice_body() -> String {
        // practice 的质检门:40-250 字,只写能力目标与作答引导。
        "## 练习：向量辨析\n本节目标：给你一组量词与场景，请判断哪些是向量、哪些是标量，         并对每个判断说明理由；作答时先独立判断，再对照答案复盘易混点。"
            .into()
    }

    fn section_body(title: &str) -> String {
        // 概念/例题节的可视化门要求至少一个可视化块:fixture 带一段展示公式。
        let prose = "这是一个用于测试的完整段落，覆盖本节要点并且足够长。".repeat(20);
        format!("## {title}\n$$\\vec{{v}} = (v_x, v_y)$$\n{prose}")
    }

    fn activities_json() -> String {
        r#"{
          "est": 20,
          "activities": [
            {"kind": "single_choice", "prompt": "q1", "options": ["A", "B", "C"], "answer": "A", "explanation": "e", "evidence": "「q1 的证据回引」", "section_key": "s1", "difficulty": 1},
            {"kind": "numeric", "prompt": "q2", "options": [], "answer": 9.8, "tol": 0.1, "explanation": "e", "evidence": "「q2 的证据回引」", "section_key": "s1", "difficulty": 2},
            {"kind": "multi_choice", "prompt": "q3", "options": ["速度", "质量", "位移"], "answer": ["速度", "位移"], "explanation": "e", "evidence": "「q3 的证据回引」", "section_key": "s1", "difficulty": 2},
            {"kind": "open_question", "prompt": "q4", "options": [], "answer": null, "explanation": "", "section_key": "general", "difficulty": 3}
          ]
        }"#
        .into()
    }

    #[tokio::test]
    async fn generate_lesson_runs_outline_sections_then_activities() {
        // Regression guard for the three-stage split (ADR-0002): one call
        // plans the manifest, one call per section writes its body, and one
        // final call writes every section-bound question.
        let body1 = section_body("概念：向量");
        let body2 = practice_body();
        let blueprint = lesson_test_blueprint();
        let module = &blueprint.modules[0];
        let lesson = &module.lessons[0];
        let completer = ScriptedCompleter::new(vec![
            outline_json(),
            body1.clone(),
            body2.clone(),
            activities_json(),
        ]);
        let output = generate_lesson(
            &completer, None, &blueprint, module, lesson, 0, 0, 1, None, "# Real",
            crate::models::TeachingStyle::Standard,
        )
        .await
        .unwrap();
        assert_eq!(output.sections.len(), 2);
        assert_eq!(output.sections[0].section_key, "s1");
        assert_eq!(output.sections[0].kind, SectionKind::Concept);
        assert_eq!(output.sections[1].kind, SectionKind::Practice);
        assert_eq!(output.sections[1].body_md, practice_body());
        // The flat summary is assembled from the sections (dual-read).
        assert!(output.summary.contains("## 概念：向量"));
        assert!(output.summary.contains("## 练习：向量辨析"));
        assert_eq!(output.estimated_minutes, 20);
        assert_eq!(output.activities.len(), 4);
        assert_eq!(output.activities[3].section_key.as_deref(), Some("general"));

        let calls = completer.calls();
        assert_eq!(calls.len(), 4, "outline + 2 sections + activities");
        assert!(calls[0].0.contains("course designer"));
        assert!(calls[1].0.contains("course writer"));
        assert!(
            calls[2].1.contains("前一节已生成正文"),
            "section 2 must receive section 1's body for coherence"
        );
        assert!(
            calls[3].1.contains("--- SECTION s1 [概念] 概念：向量 ---"),
            "activities prompt must embed the finished section bodies"
        );
    }

    #[tokio::test]
    async fn generate_lesson_retries_each_stage_independently() {
        // Each stage carries its own retry: a bad outline is corrected, a
        // failing section body gets the positioned error, and a weak
        // activities JSON is regenerated.
        let blueprint = lesson_test_blueprint();
        let module = &blueprint.modules[0];
        let lesson = &module.lessons[0];
        let completer = ScriptedCompleter::new(vec![
            r#"{"tier": "high", "sections": []}"#.into(),
            outline_json(),
            "太短。".into(),
            section_body("概念：向量"),
            practice_body(),
            r#"{"est": 20, "activities": []}"#.into(),
            activities_json(),
        ]);
        let output = generate_lesson(
            &completer, None, &blueprint, module, lesson, 0, 0, 1, None, "# Real",
            crate::models::TeachingStyle::Standard,
        )
        .await
        .unwrap();
        assert_eq!(output.sections.len(), 2);
        let calls = completer.calls();
        assert_eq!(calls.len(), 7, "outline x2 + section x2 + section + activities x2");
        assert!(calls[1].1.contains("rejected"), "outline retry carries the budget error");
        assert!(calls[3].1.contains("rejected"), "section retry carries the positioned error");
        assert!(calls[6].1.contains("rejected"), "activities retry carries the shape error");
    }

    #[tokio::test]
    async fn teaching_style_switches_the_section_writer_prompt() {
        let blueprint = lesson_test_blueprint();
        let module = &blueprint.modules[0];
        let lesson = &module.lessons[0];
        let completer = ScriptedCompleter::new(vec![
            r#"{"tier": "low", "sections": [{"section_key": "s1", "kind": "concept", "title": "概念：向量", "points": "p", "visual": "公式"}, {"section_key": "s2", "kind": "practice", "title": "练习：向量辨析", "points": "p"}]}"#.into(),
            section_body("概念：向量"),
            practice_body(),
            activities_json(),
        ]);
        generate_lesson(
            &completer, None, &blueprint, module, lesson, 0, 0, 1, None, "# Real",
            crate::models::TeachingStyle::Socratic,
        )
        .await
        .unwrap();
        let calls = completer.calls();
        assert!(
            calls[1].0.contains("SOCRATIC"),
            "the socratic style must swap the section writer prompt"
        );
    }


/// 学习建议只在其存在时进入重写提示词（ADR-0007），且永远在末行
/// "Write this section's body now." 之前。
#[test]
fn section_rewrite_prompt_carries_learner_feedback() {
    let planned = SectionPack {
        section_key: "s1".into(),
        kind: SectionKind::Concept,
        title: "概念一".into(),
        points: String::new(),
        visual: "公式".into(),
        body_md: String::new(),
    };
    let manifest = vec![SectionPack {
        section_key: "s1".into(),
        kind: SectionKind::Concept,
        title: "概念一".into(),
        points: String::new(),
        visual: "公式".into(),
        body_md: String::new(),
    }];
    let base = build_section_rewrite_prompt(
        "Course", "Lesson", "purpose", &planned, 0, &manifest, None, None, None, "",
        ComplexityTier::Mid, None,
    );
    let with_feedback = build_section_rewrite_prompt(
        "Course", "Lesson", "purpose", &planned, 0, &manifest, None, None, None, "",
        ComplexityTier::Mid, Some("  多举一个日常例子  "),
    );
    assert!(!base.contains("学习者修改意见"));
    assert!(with_feedback.contains("## 学习者修改意见"));
    assert!(with_feedback.contains("多举一个日常例子"));
    assert!(with_feedback.ends_with("Write this section's body now."));
    // 空白建议与无建议等价：不得产生空的意见块。
    let blank = build_section_rewrite_prompt(
        "Course", "Lesson", "purpose", &planned, 0, &manifest, None, None, None, "",
        ComplexityTier::Mid, Some("   "),
    );
    assert!(!blank.contains("学习者修改意见"));
}

/// t1：目标即课程名——知识库流带目标时，装配出的课程包标题是目标文本
/// （折叠单行 + 限长），而不是模型的自由命名；无目标时保持模型标题。
#[test]
fn assemble_outline_pack_uses_goal_as_course_title() {
    let blueprint = |title: &str| Blueprint {
        title: title.into(),
        description: "d".into(),
        domain: "general".into(),
        version: 1,
        modules: Vec::new(),
    };
    let request = |goal: Option<&str>| GenerateCourseRequest {
        course_kind: crate::models::CourseKind::Traditional,
        teaching_style: None,
        knowledge_base_id: Some(KnowledgeBaseId::new()),
        description: None,
        domain: None,
        provider_id: None,
        model: None,
        mode: crate::models::CourseGenerationMode::OnDemand,
        goal: goal.map(ToOwned::to_owned),
        proficiency_statement: None,
        learner_basis: None,
        knowledge_base_ids: Vec::new(),
    };

    let pack = assemble_outline_pack(&blueprint("模型自定名"), &request(Some("零基础学 Python 数据分析")));
    assert_eq!(pack.title, "零基础学 Python 数据分析");

    let legacy = assemble_outline_pack(&blueprint("模型自定名"), &request(None));
    assert_eq!(legacy.title, "模型自定名");
}

/// 多知识库（t4）：`validate_blueprint` 的 source 校验语义不变——引用必须
/// 来自**已采样路径**；多库下路径带 `{库名}/rel` 命名空间，同名文件靠前缀
/// 区分，未被采样的路径（含别的库的前缀）照旧被拒。
#[test]
fn blueprint_validation_keeps_the_sampled_path_contract_under_namespaces() {
    let blueprint = |path: &str| Blueprint {
        title: "C".into(),
        description: "d".into(),
        domain: "general".into(),
        version: 1,
        modules: vec![BlueprintModule {
            title: "M".into(),
            description: String::new(),
            lessons: vec![BlueprintLesson {
                title: "L".into(),
                purpose: "p".into(),
                source: Some(SourceSpan {
                    path: path.into(),
                    start: None,
                    end: None,
                    kb_id: None,
                }),
                sources: Vec::new(),
                anchor: None,
            }],
        }],
    };
    let samples = vec![
        ("金融笔记/docs/intro.md".to_owned(), "# 金融".to_owned()),
        ("课程笔记/docs/intro.md".to_owned(), "# 课程".to_owned()),
    ];
    assert!(validate_blueprint(&blueprint("课程笔记/docs/intro.md"), &samples).is_ok());
    let error = validate_blueprint(&blueprint("别的库/docs/intro.md"), &samples).unwrap_err();
    assert!(error.contains("unsampled source path"), "{error}");
}

// ── k04：题面相似度防雷同机械门（trigram 三口径 0.7） ──

fn activity_with_prompt(prompt: &str) -> ActivityPack {
    ActivityPack {
        kind: ActivityKind::TrueFalse,
        prompt: prompt.into(),
        options: Vec::new(),
        answer: json!(true),
        explanation: "e".into(),
        distractors: Vec::new(),
        tol: None,
        section_key: None,
        difficulty: None,
        evidence: Some("「证据回引」".into()),
    }
}

/// ①同题换数：归一化后不相等、trigram 命中——新门红（拒），旧门绿（放过）。
#[test]
fn stem_similarity_rejects_same_question_with_new_numbers_that_the_legacy_gate_missed() {
    let existing = "小明有3个苹果，又买了5个，现在一共有多少个苹果？";
    let regenerated = "小明有4个苹果，又买了6个，现在一共有多少个苹果？";
    let score = crate::learning_graph::defnear::stem_similarity(existing, regenerated);
    assert!(
        score >= crate::learning_graph::defnear::STEM_SIMILARITY_THRESHOLD,
        "换数题应命中三口径门，实际 {score:.3}"
    );
    let pool = vec![existing.to_owned()];
    assert!(similarity_hit(regenerated, &pool).is_some(), "新门红：换数题被拦");
    assert!(
        legacy_duplicate_hit(regenerated, &pool).is_none(),
        "旧门绿：换数题原本漏过（升级有效性的反证）"
    );
}

/// ②异考法绿：同主题不同考法不误伤（相似度低于阈值）。
#[test]
fn stem_similarity_keeps_different_question_angles_apart() {
    let pool = vec!["极限的定义是什么？".to_owned()];
    assert!(
        similarity_hit("用极限证明函数在一点连续需要哪些步骤？", &pool).is_none(),
        "异考法不命中"
    );
    assert!(
        crate::learning_graph::defnear::stem_similarity("极限的定义是什么？", "用极限证明函数在一点连续需要哪些步骤？")
            < crate::learning_graph::defnear::STEM_SIMILARITY_THRESHOLD
    );
}

/// ③池装饰剥除：`### `/`- ` 等 Markdown 装饰不稀释包含命中（归一化剥标点）。
#[test]
fn stem_similarity_strips_pool_decoration() {
    let plain = "下列关于导数的说法正确的是？";
    for decorated in [
        "### 下列关于导数的说法正确的是？",
        "- 下列关于导数的说法正确的是？",
        "**下列关于导数的说法正确的是？**",
    ] {
        let score = crate::learning_graph::defnear::stem_similarity(plain, decorated);
        assert!(score >= 0.999, "{decorated} → {score:.3}");
    }
}

/// ④题级丢弃：批内 3 题撞 1 → 剔 1 留 2，留痕带命中对与分值；批内兄弟
/// 也互查（第二道与第一道雷同同样被剔）。
#[test]
fn filter_novel_activities_drops_duplicates_item_by_item_with_trace() {
    let batch = vec![
        activity_with_prompt("小明有3个苹果，又买了5个，现在一共有多少个苹果？"),
        activity_with_prompt("小明有4个苹果，又买了6个，现在一共有多少个苹果？"),
        activity_with_prompt("简述导数的几何意义。"),
    ];
    let (kept, dropped) = filter_novel_activities(batch, &[]);
    assert_eq!(kept.len(), 2, "撞题被剔，其余留下");
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].score, dropped[0].score.max(0.7));
    assert!(dropped[0].prompt.contains("4个苹果"), "留痕指名被剔题面");
    assert!(dropped[0].hit_prompt.contains("3个苹果"), "留痕指名命中题面");

    // 既有池（跨批）+ 批内互查：池中已有题面同样执法。
    let existing = vec!["简述导数的几何意义。".to_owned()];
    let (kept, dropped) = filter_novel_activities(
        vec![activity_with_prompt("简述导数的几何意义是什么？")],
        &existing,
    );
    assert!(kept.is_empty());
    assert_eq!(dropped.len(), 1);
}

/// ⑤off 臂（旧门）对同一批：换数题原样放过（旧行为臂可复现）。
#[test]
fn legacy_gate_lets_the_number_swapped_question_through() {
    let pool = vec!["小明有3个苹果，又买了5个，现在一共有多少个苹果？".to_owned()];
    assert!(
        legacy_duplicate_hit("小明有4个苹果，又买了6个，现在一共有多少个苹果？", &pool)
            .is_none()
    );
}

/// ⑥地板破 → 整稿回灌带指名：三题地板、batch 全撞池 → 重试消息逐条点名
/// 丢弃条目（ScriptedCompleter 第二答仍坏则如实报错，消息含指名）。
#[tokio::test]
async fn floor_break_feeds_the_dropped_items_back_by_name() {
    let pool_question = "小明有3个苹果，又买了5个，现在一共有多少个苹果？";
    let dup = activity_with_prompt("小明有4个苹果，又买了6个，现在一共有多少个苹果？");
    let response = json!({
        "estimated_minutes": 15,
        "activities": [dup.clone(), dup.clone(), dup.clone()],
    });
    let completer = ScriptedCompleter::new(vec![
        response.to_string(),
        response.to_string(),
    ]);
    let error = super::activities::generate_lesson_activities(
        &completer,
        None,
        &format!("Existing questions:\n1. [true_false] {pool_question}"),
    )
    .await
    .expect_err("全撞 + 地板破 = 如实失败");
    assert!(error.contains("相似度"), "回灌消息带命中分值：{error}");
    assert!(
        error.contains("expected at least 3"),
        "地板破的原始理由在场：{error}"
    );
    assert!(
        error.contains("4个苹果"),
        "回灌消息逐条指名被剔题面：{error}"
    );
    assert_eq!(completer.calls().len(), 2, "恰好两次尝试（既有回路）");
}

/// ⑦平手确定性：同分取先入池者。
#[test]
fn similarity_hit_ties_take_the_first_pool_entry() {
    let pool = vec![
        "计算长方形的面积，已知长和宽。".to_owned(),
        "计算长方形面积已知长和宽".to_owned(),
    ];
    let (hit, _) = similarity_hit("计算长方形的面积已知长和宽", &pool).expect("命中");
    assert_eq!(hit, pool[0], "平手取先入池者");
}

/// 旧课时包兼容：开关判定只认 off/0/false（默认开）。
#[test]
fn qb_similarity_switch_values_are_strict() {
    for value in ["off", "OFF", "0", "false", " False "] {
        assert!(super::activities::is_qb_similarity_disabled_value(value));
    }
    for value in ["on", "1", "true", "", "disable"] {
        assert!(!super::activities::is_qb_similarity_disabled_value(value));
    }
}
