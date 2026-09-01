    use super::activities::build_activities_prompt;
    use super::blueprint::validate_blueprint;
    use super::completer::complete_with_timeout;
    use super::lesson::{build_lesson_document_prompt, validate_lesson, validate_lesson_document};
    use super::parser::{strip_code_fences, strip_markdown_fences};
    use super::*;
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
            3,
            4,
            &[("lessons/liquidity.md".into(), "# Liquidity\nText".into())],
        );
        assert!(prompt.contains("--- FILE: lessons/liquidity.md ---"));
        assert!(prompt.contains("Requested domain label: trading"));
        assert!(prompt.contains("exactly 3 modules and 4 lessons per module"));
        assert!(BLUEPRINT_SYSTEM.contains("untrusted source material"));
        assert!(BLUEPRINT_SYSTEM.contains("Never invent paths"));
    }

    #[test]
    fn lesson_document_standard_is_flexible_and_long_form() {
        // Required core sections.
        for section in ["1. 描述 (Description)", "2. 例子 (Examples)", "3. 验证 (Verification)"] {
            assert!(
                LESSON_DOCUMENT_STANDARD.contains(section),
                "missing required section: {section}"
            );
        }
        // Optional sections are offered, not mandated.
        assert!(LESSON_DOCUMENT_STANDARD.contains("Optional sections"));
        assert!(LESSON_DOCUMENT_STANDARD.contains("Custom sections"));
        // Keywords pair every term with a one-line digest, never bare words.
        assert!(LESSON_DOCUMENT_STANDARD.contains("term: one-line digest"));
        assert!(LESSON_DOCUMENT_STANDARD.contains("Never list a bare keyword"));
        // A length floor is enforced.
        assert!(LESSON_DOCUMENT_STANDARD.contains("1000-1500 characters"));
        // The rigid seven-section rule is gone.
        assert!(!LESSON_DOCUMENT_STANDARD.contains("exactly these seven sections"));
        assert!(!LESSON_DOCUMENT_STANDARD.contains("IN THIS ORDER"));
    }

    #[test]
    fn retry_prompts_never_shrink_output() {
        // Regression guard: retries must ask for corrections, never smaller
        // JSON — the old "smaller" instruction caused thin summaries.
        assert!(!BLUEPRINT_SYSTEM.contains("smaller"));
        assert!(!LESSON_SYSTEM.contains("smaller"));
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
    fn blueprint_validator_rejects_unknown_paths_and_cycles() {
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
                    prerequisites: vec!["b".into()],
                },
                ConceptPack {
                    key: "b".into(),
                    title: "B".into(),
                    description: String::new(),
                    prerequisites: vec!["a".into()],
                },
            ],
            modules: vec![BlueprintModule {
                title: "M".into(),
                description: String::new(),
                lessons: vec![BlueprintLesson {
                    title: "L".into(),
                    purpose: String::new(),
                    concepts: vec!["a".into()],
                    source: Some(SourceSpan {
                        path: "real.md".into(),
                        start: None,
                        end: None,
                    }),
                }],
            }],
        };
        assert!(
            validate_blueprint(&blueprint, &samples, 1, 1).is_err(),
            "prerequisite cycle must be rejected"
        );

        let mut bad = blueprint.clone();
        bad.modules[0].lessons[0].source = Some(SourceSpan {
            path: "invented.md".into(),
            start: None,
            end: None,
        });
        assert!(
            validate_blueprint(&bad, &samples, 1, 1).is_err(),
            "unsampled source path must be rejected"
        );

        let mut sized = blueprint.clone();
        sized.concepts[0].prerequisites = Vec::new();
        sized.concepts[1].prerequisites = Vec::new();
        assert!(
            validate_blueprint(&sized, &samples, 2, 1).is_err(),
            "module count mismatch must be rejected"
        );
    }

    #[test]
    fn lesson_validation_enforces_length_and_objective_activities() {
        let blueprint = Blueprint {
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
            modules: Vec::new(),
        };
        let module = BlueprintModule {
            title: "M".into(),
            description: String::new(),
            lessons: Vec::new(),
        };
        let lesson = BlueprintLesson {
            title: "L".into(),
            purpose: String::new(),
            concepts: vec!["a".into()],
            source: Some(SourceSpan {
                path: "real.md".into(),
                start: None,
                end: None,
            }),
        };

        let short = LessonOutput {
            summary: "太短了。".repeat(50),
            estimated_minutes: 10,
            activities: vec![
                ActivityPack {
                    kind: ActivityKind::SingleChoice,
                    prompt: "q1".into(),
                    options: vec!["A".into(), "B".into(), "C".into()],
                    answer: json!("A"),
                    explanation: "e".into(),
                    concepts: vec!["a".into()],
                    distractors: Vec::new(),
                },
                ActivityPack {
                    kind: ActivityKind::TrueFalse,
                    prompt: "q2".into(),
                    options: Vec::new(),
                    answer: json!(true),
                    explanation: "e".into(),
                    concepts: vec!["a".into()],
                    distractors: Vec::new(),
                },
                ActivityPack {
                    kind: ActivityKind::Reflection,
                    prompt: "q3".into(),
                    options: Vec::new(),
                    answer: json!(null),
                    explanation: String::new(),
                    concepts: vec!["a".into()],
                    distractors: Vec::new(),
                },
            ],
        };
        assert!(
            validate_lesson(&short, &blueprint, &module, &lesson).is_err(),
            "short summary must be rejected"
        );

        let long_summary = long_document();
        let mut good = short.clone();
        good.summary = long_summary;
        assert!(
            validate_lesson(&good, &blueprint, &module, &lesson).is_ok(),
            "long summary with 3 activities must pass"
        );

        let mut no_objective = good.clone();
        no_objective.activities = vec![
            ActivityPack {
                kind: ActivityKind::Reflection,
                prompt: "r1".into(),
                options: Vec::new(),
                answer: json!(null),
                explanation: String::new(),
                concepts: vec!["a".into()],
                distractors: Vec::new(),
            },
            ActivityPack {
                kind: ActivityKind::Reflection,
                prompt: "r2".into(),
                options: Vec::new(),
                answer: json!(null),
                explanation: String::new(),
                concepts: vec!["a".into()],
                distractors: Vec::new(),
            },
            ActivityPack {
                kind: ActivityKind::Reflection,
                prompt: "r3".into(),
                options: Vec::new(),
                answer: json!(null),
                explanation: String::new(),
                concepts: vec!["a".into()],
                distractors: Vec::new(),
            },
        ];
        assert!(
            validate_lesson(&no_objective, &blueprint, &module, &lesson).is_err(),
            "objective activity floor must be enforced"
        );

        let mut bad_answer = good.clone();
        bad_answer.activities[0].answer = json!("D");
        assert!(
            validate_lesson(&bad_answer, &blueprint, &module, &lesson).is_err(),
            "single_choice answer outside options must be rejected"
        );
    }

    #[test]
    fn reflection_rules_cap_questions_and_never_cross_lessons() {
        // The lesson-stage standard requires reflection questions to test ALL
        // of the lesson's concepts, prefers exactly one, and caps the count
        // at three.
        assert!(LESSON_SYSTEM.contains("test ALL of the lesson's concepts"));
        assert!(LESSON_SYSTEM.contains("Never bind concepts of other lessons"));
        assert!(LESSON_SYSTEM.contains("prefer exactly 1"));

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
        let module = BlueprintModule {
            title: "M".into(),
            description: String::new(),
            lessons: Vec::new(),
        };
        let lesson = BlueprintLesson {
            title: "L".into(),
            purpose: String::new(),
            concepts: vec!["a".into()],
            source: Some(SourceSpan {
                path: "real.md".into(),
                start: None,
                end: None,
            }),
        };
        let summary = long_document();
        let objective = vec![
            ActivityPack {
                kind: ActivityKind::SingleChoice,
                prompt: "q1".into(),
                options: vec!["A".into(), "B".into(), "C".into()],
                answer: json!("A"),
                explanation: "e".into(),
                concepts: vec!["a".into()],
                distractors: Vec::new(),
            },
            ActivityPack {
                kind: ActivityKind::TrueFalse,
                prompt: "q2".into(),
                options: Vec::new(),
                answer: json!(true),
                explanation: "e".into(),
                concepts: vec!["a".into()],
                distractors: Vec::new(),
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
        };

        // Up to three reflections pass when one question cannot cover all of
        // the lesson's concepts.
        let mut at_cap = LessonOutput {
            summary: summary.clone(),
            estimated_minutes: 10,
            activities: objective.clone(),
        };
        at_cap.activities.push(reflection("r1"));
        at_cap.activities.push(reflection("r2"));
        at_cap.activities.push(reflection("r3"));
        assert!(validate_lesson(&at_cap, &blueprint, &module, &lesson).is_ok());

        // A fourth reflection exceeds the cap.
        let mut over_cap = at_cap.clone();
        over_cap.activities.push(reflection("r4"));
        assert!(validate_lesson(&over_cap, &blueprint, &module, &lesson).is_err());

        // Reflections never cross lessons: binding another lesson's concept
        // is rejected, objective activities stay lesson-bound as before.
        let mut cross_lesson = at_cap.clone();
        cross_lesson.activities[2].concepts = vec!["b".into()];
        assert!(validate_lesson(&cross_lesson, &blueprint, &module, &lesson).is_err());
        let mut objective_cross = at_cap.clone();
        objective_cross.activities[0].concepts = vec!["b".into()];
        assert!(validate_lesson(&objective_cross, &blueprint, &module, &lesson).is_err());
    }

    #[test]
    fn fill_in_blank_rules_pin_blank_answers_and_distractors() {
        // The lesson-stage standard embeds the fill-in-the-blank design rules:
        // a single ___ blank, 1-3 convergent answers, near-synonym distractors.
        assert!(LESSON_SYSTEM.contains("\"___\" blank"));
        assert!(LESSON_SYSTEM.contains("1-3 equivalent accepted answers"));
        assert!(LESSON_SYSTEM.contains("test the relationship before the name"));
        assert!(LESSON_SYSTEM.contains("near-synonym distractors"));

        let blueprint = Blueprint {
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
            modules: Vec::new(),
        };
        let module = BlueprintModule {
            title: "M".into(),
            description: String::new(),
            lessons: Vec::new(),
        };
        let lesson = BlueprintLesson {
            title: "L".into(),
            purpose: String::new(),
            concepts: vec!["a".into()],
            source: Some(SourceSpan {
                path: "real.md".into(),
                start: None,
                end: None,
            }),
        };
        let summary = long_document();
        let base = LessonOutput {
            summary: summary.clone(),
            estimated_minutes: 10,
            activities: vec![
                ActivityPack {
                    kind: ActivityKind::SingleChoice,
                    prompt: "q1".into(),
                    options: vec!["A".into(), "B".into(), "C".into()],
                    answer: json!("A"),
                    explanation: "e".into(),
                    concepts: vec!["a".into()],
                    distractors: Vec::new(),
                },
                ActivityPack {
                    kind: ActivityKind::Reflection,
                    prompt: "r1".into(),
                    options: Vec::new(),
                    answer: json!(null),
                    explanation: String::new(),
                    concepts: vec!["a".into()],
                    distractors: Vec::new(),
                },
                ActivityPack {
                    kind: ActivityKind::FillInBlank,
                    prompt: "A vector has ___ and direction.".into(),
                    options: Vec::new(),
                    answer: json!(["magnitude"]),
                    explanation: "e".into(),
                    concepts: vec!["a".into()],
                    distractors: vec!["length".into(), "norm".into()],
                },
            ],
        };
        assert!(validate_lesson(&base, &blueprint, &module, &lesson).is_ok());

        let mut no_blank = base.clone();
        no_blank.activities[2].prompt = "A vector is a quantity.".into();
        assert!(validate_lesson(&no_blank, &blueprint, &module, &lesson).is_err());

        let mut wrong_answer = base.clone();
        wrong_answer.activities[2].answer = json!("magnitude");
        assert!(validate_lesson(&wrong_answer, &blueprint, &module, &lesson).is_err());

        let mut empty_answers = base.clone();
        empty_answers.activities[2].answer = json!([]);
        assert!(validate_lesson(&empty_answers, &blueprint, &module, &lesson).is_err());

        let mut too_many_answers = base.clone();
        too_many_answers.activities[2].answer = json!(["a", "b", "c", "d"]);
        assert!(validate_lesson(&too_many_answers, &blueprint, &module, &lesson).is_err());

        let mut no_distractors = base.clone();
        no_distractors.activities[2].distractors = vec![" ".into()];
        assert!(validate_lesson(&no_distractors, &blueprint, &module, &lesson).is_err());
    }

    #[test]
    fn generated_request_keeps_selected_knowledge_base() {
        let id = KnowledgeBaseId::new();
        let request = GenerateCourseRequest {
            course_kind: crate::models::CourseKind::Traditional,
            knowledge_base_id: Some(id.clone()),
            description: None,
            domain: None,
            provider_id: None,
            model: None,
            module_count: 3,
            lessons_per_module: 3,
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
    fn document_stage_prompt_is_json_free_and_activities_prompt_embeds_document() {
        // The document stage must never ask for JSON — the whole point of the
        // split is that long-form text is emitted as plain Markdown. The
        // activity stage prompt must embed the finished document so questions
        // verify exactly what was written.
        assert!(LESSON_DOCUMENT_SYSTEM.contains("No JSON"));
        let blueprint = lesson_test_blueprint();
        let module = &blueprint.modules[0];
        let lesson = &module.lessons[0];
        let document_prompt =
            build_lesson_document_prompt(&blueprint, &[], module, lesson, 0, 0, 1, None, "# Real");
        assert!(document_prompt.contains("Write the lesson document now."));
        assert!(!document_prompt.contains("JSON"));
        let activities_prompt =
            build_activities_prompt(&blueprint, lesson, "## 描述\n正文", "# Real");
        assert!(activities_prompt.contains("--- DOCUMENT START ---\n## 描述\n正文\n--- DOCUMENT END ---"));
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

    #[tokio::test]
    async fn generate_lesson_splits_document_and_activities_calls() {
        // Regression guard for the two-stage split: the document stage gets
        // the plain-Markdown system prompt and no JSON parse; the activity
        // stage gets the small-JSON system prompt plus the finished document
        // in its prompt.
        let document = long_document() + "\n下一课将继续深化这一主题。";
        let activities = r#"{
          "estimated_minutes": 15,
          "activities": [
            {"kind": "single_choice", "prompt": "q1", "options": ["A", "B", "C"], "answer": "A", "explanation": "e", "concepts": ["a"]},
            {"kind": "true_false", "prompt": "q2", "options": [], "answer": true, "explanation": "e", "concepts": ["a"]},
            {"kind": "reflection", "prompt": "q3", "options": [], "answer": null, "explanation": "", "concepts": ["a"]}
          ]
        }"#;
        let blueprint = lesson_test_blueprint();
        let module = &blueprint.modules[0];
        let lesson = &module.lessons[0];
        let completer = ScriptedCompleter::new(vec![document.clone(), activities.to_owned()]);
        let output = generate_lesson(
            &completer, None, &blueprint, &[], module, lesson, 0, 0, 1, None, "# Real",
        )
        .await
        .unwrap();
        assert_eq!(output.summary, document);
        assert_eq!(output.estimated_minutes, 15);
        assert_eq!(output.activities.len(), 3);

        let calls = completer.calls();
        assert_eq!(calls.len(), 2, "one document call + one activities call");
        assert_eq!(calls[0].0, LESSON_DOCUMENT_SYSTEM);
        assert_eq!(calls[1].0, LESSON_SYSTEM);
        assert!(
            calls[1].1.contains(&document),
            "activities prompt must embed the finished document"
        );
    }

    #[tokio::test]
    async fn generate_lesson_retries_each_stage_independently() {
        // Each stage has its own retry budget: a short document is rejected
        // and regenerated, then a weak activities JSON is rejected and
        // regenerated — four calls total for one lesson.
        let document = long_document() + "\n下一课将继续深化这一主题。";
        let good_activities = r#"{
          "estimated_minutes": 15,
          "activities": [
            {"kind": "single_choice", "prompt": "q1", "options": ["A", "B", "C"], "answer": "A", "explanation": "e", "concepts": ["a"]},
            {"kind": "true_false", "prompt": "q2", "options": [], "answer": true, "explanation": "e", "concepts": ["a"]},
            {"kind": "reflection", "prompt": "q3", "options": [], "answer": null, "explanation": "", "concepts": ["a"]}
          ]
        }"#;
        let completer = ScriptedCompleter::new(vec![
            "太短了。".to_owned(),
            document.clone(),
            r#"{"estimated_minutes": 15, "activities": []}"#.to_owned(),
            good_activities.to_owned(),
        ]);
        let blueprint = lesson_test_blueprint();
        let module = &blueprint.modules[0];
        let lesson = &module.lessons[0];
        let output = generate_lesson(
            &completer, None, &blueprint, &[], module, lesson, 0, 0, 1, None, "# Real",
        )
        .await
        .unwrap();
        assert_eq!(output.summary, document);
        assert_eq!(output.activities.len(), 3);
        let calls = completer.calls();
        assert_eq!(
            calls.len(),
            4,
            "two document attempts + two activities attempts"
        );
        assert!(
            calls[1].1.contains("rejected"),
            "document retry must carry the validation error"
        );
        assert!(
            calls[3].1.contains("rejected"),
            "activities retry must carry the validation error"
        );
    }
