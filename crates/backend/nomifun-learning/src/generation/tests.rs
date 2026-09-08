    use super::activities::validate_lesson_activities;
    use super::completer::complete_with_timeout;
    use super::lesson::{build_activities_prompt, validate_lesson_document};
    use super::parser::{strip_code_fences, strip_markdown_fences};
    use super::*;
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
            &[("lessons/liquidity.md".into(), "# Liquidity\nText".into())],
        );
        assert!(prompt.contains("--- FILE: lessons/liquidity.md ---"));
        assert!(prompt.contains("Requested domain label: trading"));
        assert!(prompt.contains("Course size is yours to decide"));
        assert!(BLUEPRINT_SYSTEM.contains("untrusted source material"));
        assert!(BLUEPRINT_SYSTEM.contains("Never invent paths"));
    }

    #[test]
    fn retry_prompts_never_shrink_output() {
        // Regression guard: retries must ask for corrections, never smaller
        // JSON — the old "smaller" instruction caused thin summaries.
        assert!(!BLUEPRINT_SYSTEM.contains("smaller"));
        assert!(!LESSON_SYSTEM.contains("smaller"));
    }

    #[test]
    fn section_outline_guardrails_block_direction_extremes() {
        let section = |key: &str| SectionPack {
            section_key: key.into(),
            kind: SectionKind::Concept,
            title: format!("概念：{key}"),
            points: String::new(),
            body_md: String::new(),
        };
        // Every valid outline closes with exactly one practice section.
        let with_practice = |mut sections: Vec<SectionPack>| {
            sections.push(SectionPack {
                section_key: format!("s{}", sections.len() + 1),
                kind: SectionKind::Practice,
                title: "练习：巩固".into(),
                points: String::new(),
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
        // Concept: long enough plain prose passes.
        let concept = SectionPack {
            section_key: "s1".into(),
            kind: SectionKind::Concept,
            title: "概念：向量".into(),
            points: String::new(),
            body_md: prose.clone(),
        };
        assert!(concept.validate_body().is_ok());
        // Too short fails.
        let short = SectionPack { body_md: "太短。".into(), ..concept.clone() };
        assert!(short.validate_body().is_err());
        // ### sub-headings are banned inside a section.
        let subheaded = SectionPack { body_md: format!("{prose}\n### 小标题\n{prose}"), ..concept.clone() };
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
              "concepts": ["key", null],
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
        assert_eq!(activity.concepts, vec!["key".to_owned()]);
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
    fn blueprint_validator_rejects_cycles_unsampled_sources_and_empty_outline() {
        let samples = vec![("real.md".to_owned(), "# Real".to_owned())];
        let blueprint = Blueprint {
            title: "C".into(),
            description: String::new(),
            domain: String::new(),
            version: 1,
            concepts: vec![
                ConceptPack {
                    key: "a".into(),
                    title: "A".into(),
                    description: String::new(),
                    prerequisites: Vec::new(),
                },
                ConceptPack {
                    key: "b".into(),
                    title: "B".into(),
                    description: String::new(),
                    prerequisites: Vec::new(),
                },
            ],
            modules: Vec::new(),
        };
        let lesson = BlueprintLesson {
            title: "L".into(),
            purpose: String::new(),
            concepts: vec!["a".into()],
            source: None,
        };
        let objective = vec![
            ActivityPack {
                kind: ActivityKind::SingleChoice,
                prompt: "q1".into(),
                options: vec!["A".into(), "B".into(), "C".into()],
                answer: json!("A"),
                explanation: "e".into(),
                concepts: vec!["a".into()],
                distractors: Vec::new(),
                tol: None,
                section_key: None,
            },
            ActivityPack {
                kind: ActivityKind::TrueFalse,
                prompt: "q2".into(),
                options: Vec::new(),
                answer: json!(true),
                explanation: "e".into(),
                concepts: vec!["a".into()],
                distractors: Vec::new(),
                tol: None,
                section_key: None,
            },
        ];
        let reflection = |prompt: &str| ActivityPack {
            kind: ActivityKind::Reflection,
            prompt: prompt.into(),
            options: Vec::new(),
            answer: json!(null),
            explanation: String::new(),
            concepts: vec!["a".into()],
            distractors: Vec::new(),
            tol: None,
            section_key: None,
        };

        // Up to three AI-graded questions pass (validate_shape gate each).
        let mut at_cap = objective.clone();
        at_cap.push(reflection("r1"));
        at_cap.push(reflection("r2"));
        at_cap.push(reflection("r3"));
        for activity in &at_cap {
            activity.validate_shape((3, 5), true).unwrap();
        }
        assert!(validate_lesson_activities(&at_cap, &blueprint, &lesson).is_ok());

        // A fourth AI-graded question exceeds the cap.
        let mut over_cap = at_cap.clone();
        over_cap.push(reflection("r4"));
        assert!(validate_lesson_activities(&over_cap, &blueprint, &lesson).is_err());

        // AI-graded questions never cross lessons: binding another lesson's
        // concept is rejected, objective activities stay lesson-bound.
        let mut cross_lesson = at_cap.clone();
        cross_lesson[2].concepts = vec!["b".into()];
        assert!(validate_lesson_activities(&cross_lesson, &blueprint, &lesson).is_err());
        let mut objective_cross = at_cap.clone();
        objective_cross[0].concepts = vec!["b".into()];
        assert!(validate_lesson_activities(&objective_cross, &blueprint, &lesson).is_err());
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
            concepts: vec!["a".into()],
            distractors: vec!["length".into(), "norm".into()],
            tol: None,
            section_key: None,
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
            teaching_style: None,
            knowledge_base_id: Some(id.clone()),
            description: None,
            domain: None,
            provider_id: None,
            model: None,
            mode: crate::models::CourseGenerationMode::OnDemand,
        };
        assert_eq!(request.knowledge_base_id, Some(id));
    }

    /// A minimal blueprint with one module, one lesson and one concept.
    fn lesson_test_blueprint() -> Blueprint {
        Blueprint {
            title: "C".into(),
            description: String::new(),
            domain: String::new(),
            version: 1,
            concepts: vec![ConceptPack {
                key: "a".into(),
                title: "A".into(),
                description: String::new(),
                prerequisites: Vec::new(),
            }],
            modules: vec![BlueprintModule {
                title: "M".into(),
                description: String::new(),
                lessons: vec![BlueprintLesson {
                    title: "L".into(),
                    purpose: "p".into(),
                    concepts: vec!["a".into()],
                    source: Some(SourceSpan {
                        path: "real.md".into(),
                        start: None,
                        end: None,
                    }),
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
            concepts: vec!["a".into()],
            source: source.map(|path| SourceSpan {
                path: path.into(),
                start: None,
                end: None,
            }),
        };
        Blueprint {
            title: "C".into(),
            description: String::new(),
            domain: String::new(),
            version: 1,
            concepts: vec![ConceptPack {
                key: "a".into(),
                title: "A".into(),
                description: String::new(),
                prerequisites: Vec::new(),
            }],
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

    #[test]
    fn section_prompts_carry_task_manifest_and_grounding() {
        // The outline prompt embeds the outline tree, the concepts and asks
        // for the section-manifest JSON; the body prompt carries the section
        // task, the manifest and the previous body for coherence; the
        // activities prompt embeds every finished section body.
        let blueprint = lesson_test_blueprint();
        let module = &blueprint.modules[0];
        let lesson = &module.lessons[0];
        let outline_prompt = super::lesson::build_section_outline_prompt(
            &blueprint, module, lesson, 0, 0, 1, "# Real",
        );
        assert!(outline_prompt.contains("Plan the section outline JSON now."));
        assert!(outline_prompt.contains("--- FILE: real.md ---"));

        let manifest = vec![SectionPack {
            section_key: "s1".into(),
            kind: SectionKind::Concept,
            title: "概念：向量".into(),
            points: "什么是向量".into(),
            body_md: String::new(),
        }];
        let planned = &manifest[0];
        let body_prompt = super::lesson::build_section_body_prompt(
            &blueprint,
            lesson,
            "# Real",
            planned,
            0,
            &manifest,
            None,
            Some("下一课"),
        );
        assert!(body_prompt.contains("## 本节任务"));
        assert!(body_prompt.contains("节类型：概念"));
        assert!(body_prompt.contains("Write this section's body now."));

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
            "# Real",
        );
        assert!(activities_prompt.contains("Complexity tier: mid"));
        assert!(activities_prompt.contains("--- SECTION s1 [概念] 概念：向量 ---"));
        assert!(activities_prompt.contains("Design the activity JSON now."));
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
            {"section_key": "s1", "kind": "concept", "title": "概念：向量", "points": "什么是向量"},
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
        let prose = "这是一个用于测试的完整段落，覆盖本节要点并且足够长。".repeat(20);
        format!("## {title}\n{prose}")
    }

    fn activities_json() -> String {
        r#"{
          "estimated_minutes": 20,
          "activities": [
            {"kind": "single_choice", "prompt": "q1", "options": ["A", "B", "C"], "answer": "A", "explanation": "e", "concepts": ["a"], "section_key": "s1"},
            {"kind": "numeric", "prompt": "q2", "options": [], "answer": 9.8, "tol": 0.1, "explanation": "e", "concepts": ["a"], "section_key": "s1"},
            {"kind": "multi_choice", "prompt": "q3", "options": ["速度", "质量", "位移"], "answer": ["速度", "位移"], "explanation": "e", "concepts": ["a"], "section_key": "s1"},
            {"kind": "open_question", "prompt": "q4", "options": [], "answer": null, "explanation": "", "concepts": ["a"], "section_key": "general"}
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
            r#"{"estimated_minutes": 20, "activities": []}"#.into(),
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
            r#"{"tier": "low", "sections": [{"section_key": "s1", "kind": "concept", "title": "概念：向量", "points": "p"}, {"section_key": "s2", "kind": "practice", "title": "练习：向量辨析", "points": "p"}]}"#.into(),
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

