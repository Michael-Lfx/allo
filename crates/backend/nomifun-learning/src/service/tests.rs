    use super::checkin::local_wall_clock_utc_ms;
    use super::course::{recommend_next_lesson, validate_pack};
    use super::progress::evaluate;
    use super::*;
    use crate::models::{ActivityPack, LessonPack, ModulePack};
    use nomifun_api_types::WebSocketMessage;
    use serde_json::json;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};


    #[derive(Default)]
    struct NoopBroadcaster;

    impl nomifun_realtime::UserEventSink for NoopBroadcaster {
        fn send_to_user(&self, _user_id: &str, _event: WebSocketMessage<serde_json::Value>) {}
    }

    /// Placeholder completer: job-start tests never reach the LLM, but
    /// `set_generation_dependencies` requires a completer value.
    struct UnusedCompleter;

    #[async_trait::async_trait]
    impl LearningCompleter for UnusedCompleter {
        async fn complete(
            &self,
            _model_override: Option<(&str, &str)>,
            _system: &str,
            _user: &str,
            _max_tokens: u32,
        ) -> Result<String, nomifun_common::AppError> {
            Err(nomifun_common::AppError::Internal(
                "job tests do not invoke the completer".into(),
            ))
        }
    }

    async fn job_test_service() -> (LearningService, Arc<KnowledgeService>, nomifun_common::UserId) {
        let data_dir = tempfile::tempdir().unwrap();
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let owner = nomifun_common::UserId::parse(&owner_id).unwrap();
        let knowledge_service = Arc::new(KnowledgeService::new(
            Arc::new(nomifun_db::SqliteKnowledgeRepository::new(
                database.pool().clone(),
            )),
            data_dir.path(),
            nomifun_knowledge::KnowledgeEventEmitter::new(
                Arc::new(NoopBroadcaster),
                Arc::from(owner_id),
            ),
        ));
        let learning_service = LearningService::new(database.pool().clone());
        learning_service.set_generation_dependencies(
            knowledge_service.clone(),
            Arc::new(UnusedCompleter),
        );
        (learning_service, knowledge_service, owner)
    }

    async fn generation_request(
        knowledge_service: &KnowledgeService,
    ) -> GenerateCourseRequest {
        let base = knowledge_service
            .quick_create_base(Some("Math"), None, "blank", None, None, None, None)
            .await
            .unwrap();
        GenerateCourseRequest {
            course_kind: crate::models::CourseKind::Traditional,
            proficiency_statement: None,
            learner_basis: None,
            teaching_style: None,
            knowledge_base_id: Some(base.base.knowledge_base_id.parse().unwrap()),
            description: None,
            domain: None,
            provider_id: None,
            model: None,
            mode: crate::models::CourseGenerationMode::OnDemand,
            goal: None,
            knowledge_base_ids: Vec::new(),
        }
    }

    #[tokio::test]
    async fn generation_slot_rejects_duplicate_and_releases() {
        let (service, _knowledge_service, owner_id) = job_test_service().await;
        // The slot is per (user, source key): the second submit for the same
        // key is refused with a conflict while the first is in flight.
        let _guard = service
            .acquire_generation_slot(&owner_id, "kb:math".into())
            .unwrap();
        // unwrap_err needs `Debug` on the guard; match keeps the test free
        // of a derive that only exists for this one assertion.
        let error = match service.acquire_generation_slot(&owner_id, "kb:math".into()) {
            Err(error) => error,
            Ok(_) => panic!("expected a conflict for the duplicate slot"),
        };
        assert!(
            matches!(error, AppError::Conflict(_)),
            "expected a conflict for the duplicate slot, got {error}"
        );
        // Another user's submission on the same source is not blocked.
        let other_user = nomifun_common::UserId::new();
        let other_guard = service
            .acquire_generation_slot(&other_user, "kb:math".into())
            .unwrap();
        drop(other_guard);
        // Dropping the guard releases the slot: the same source can be
        // generated again immediately (a failed or cancelled generation
        // never wedges the next submit).
        drop(_guard);
        service
            .acquire_generation_slot(&owner_id, "kb:math".into())
            .unwrap();
    }

    fn valid_pack() -> CoursePack {
        CoursePack {
            title: "Linear Algebra".into(),
teaching_style: crate::models::TeachingStyle::Standard,
            description: "A small generic course".into(),
            domain: "mathematics".into(),
            source_kb_id: None,
            version: 1,
            modules: vec![ModulePack {
                title: "Foundations".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    est: 10,
                    source: None,
                    sources: Vec::new(),
                    activities: vec![ActivityPack {
            difficulty: None,
                        kind: ActivityKind::TrueFalse,
                        prompt: "A vector has magnitude and direction.".into(),
                        options: Vec::new(),
                        answer: Value::Bool(true),
                        explanation: "That is the geometric definition.".into(),
                    distractors: Vec::new(),
                    tol: None,
                    section_key: None,
                    }],
                    sections: Vec::new(),
                }],
            }],
        }
    }

    /// Records the generation contexts it receives — the engine-path contract
    /// test: the service must pre-render the outline tree and the adjacent
    /// lesson reference into `LessonGenerationContext`.
    struct RecordingLessonEngine {
        contexts: Mutex<Vec<LessonGenerationContext>>,
    }

    #[async_trait::async_trait]
    impl LessonContentAgentEngine for RecordingLessonEngine {
        async fn generate(
            &self,
            _user_id: &UserId,
            context: &LessonGenerationContext,
            _model_override: Option<(&str, &str)>,
        ) -> Result<LessonOutput, nomifun_common::AppError> {
            self.contexts.lock().unwrap().push(context.clone());
            Ok(LessonOutput {
                summary: "## 描述\n占位\n## 例子\n占位\n## 验证\n占位\n".into(),
                estimated_minutes: 10,
                activities: Vec::new(),
                sections: Vec::new(),
                degraded_keys: Vec::new(),
            })
        }

        async fn resume(
            &self,
            _user_id: &UserId,
            _draft_id: &str,
            context: &LessonGenerationContext,
            _model_override: Option<(&str, &str)>,
        ) -> Result<LessonOutput, nomifun_common::AppError> {
            self.generate(_user_id, context, _model_override).await
        }
    }

    /// Engine-path contract: the service pre-renders the full outline tree
    /// (current lesson marked) and the adjacent-lesson reference (kb-flow
    /// excerpt truncated at the budget) into the context handed to the engine.
    #[tokio::test]
    async fn lesson_context_carries_outline_tree_and_adjacent_reference() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool()).await.unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());

        let lesson = |title: &str, purpose: &str, source: Option<SourceSpan>| LessonPack {
            title: title.into(),
            summary: String::new(),
            purpose: purpose.into(),
            est: 10,
            source,
            sources: Vec::new(),
            activities: Vec::new(),
            sections: Vec::new(),
        };
        let pack = CoursePack {
            title: "上下文课程".into(),
teaching_style: crate::models::TeachingStyle::Standard,
            description: "两模块两课时".into(),
            domain: "math".into(),
            source_kb_id: None,
            version: 1,
            modules: vec![
                ModulePack {
                    title: "模块一".into(),
                    description: String::new(),
                    lessons: vec![
                        lesson("第一课", "目标一", None),
                        lesson("第二课", "目标二", Some(SourceSpan {
                            path: "docs/two.md".into(),
                            start: None,
                            end: None,
                            kb_id: None,
                        })),
                    ],
                },
                ModulePack {
                    title: "模块二".into(),
                    description: String::new(),
                    lessons: vec![
                        lesson("第三课", "目标三", None),
                        lesson("第四课", "目标四", None),
                    ],
                },
            ],
        };
        let blueprint_lesson = |title: &str, purpose: &str, source: Option<SourceSpan>| crate::generation::BlueprintLesson {
            title: title.into(),
            purpose: purpose.into(),
            source,
            sources: Vec::new(),
        };
        let blueprint_module = |title: &str,
                                lessons: Vec<crate::generation::BlueprintLesson>|
         crate::generation::BlueprintModule {
            title: title.into(),
            description: String::new(),
            lessons,
        };
        let blueprint = Blueprint {
            title: "上下文课程".into(),
            description: "两模块两课时".into(),
            domain: "math".into(),
            version: 1,
            modules: vec![
                blueprint_module("模块一", vec![
                    blueprint_lesson("第一课", "目标一", None),
                    blueprint_lesson("第二课", "目标二", Some(SourceSpan {
                        path: "docs/two.md".into(),
                        start: None,
                        end: None,
                        kb_id: None,
                    })),
                ]),
                blueprint_module("模块二", vec![
                    blueprint_lesson("第三课", "目标三", None),
                    blueprint_lesson("第四课", "目标四", None),
                ]),
            ],
        };
        let samples = vec![(
            "docs/two.md".to_owned(),
            format!("{}尾部标记", "第二课原文。".repeat(300)),
        )];
        let detail = service
            .import_course_outline(
                pack,
                OutlineSnapshot {
                    blueprint_json: serde_json::to_string(&blueprint).unwrap(),
                    samples_json: serde_json::to_string(&samples).unwrap(),
                    learning_goal: String::new(),
                    samples_meta_json: None,
                    source_kb_ids: Vec::new(),
                },
            )
            .await
            .unwrap();
        let lesson_id = detail.modules[1].lessons[0].id.clone();

        let engine = Arc::new(RecordingLessonEngine {
            contexts: Mutex::new(Vec::new()),
        });
        service.set_lesson_engine(engine.clone());
        service
            .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
            .await
            .unwrap();

        let contexts = engine.contexts.lock().unwrap();
        assert_eq!(contexts.len(), 1);
        let context = &contexts[0];
        assert!(context.outline_tree.contains("模块 2/2：模块二"));
        assert!(context.outline_tree.contains("  3. 第三课 — 目标三（本课时）"));
        assert!(context.outline_tree.contains("  1. 第一课 — 目标一"));
        assert!(context.adjacent_context.contains("上一课时「第二课」— 目标二"));
        assert!(context.adjacent_context.contains("下一课时「第四课」— 目标四"));
        assert!(context.adjacent_context.contains("第二课原文"));
        assert!(
            !context.adjacent_context.contains("尾部标记"),
            "the adjacent excerpt must be truncated at the budget"
        );
    }

    #[test]
    fn pack_validation_rejects_missing_title_and_empty_modules() {
        let mut pack = valid_pack();
        pack.title = "   ".into();
        let error = validate_pack(&pack).unwrap_err();
        assert!(error.to_string().contains("title is required"));

        let mut pack = valid_pack();
        pack.modules.clear();
        let error = validate_pack(&pack).unwrap_err();
        assert!(error.to_string().contains("at least one module"));
    }

    #[test]
    fn builtin_evaluator_does_not_trust_client_scores() {
        let config = StoredActivityConfig {
            options: Vec::new(),
            matches: Vec::new(),
            answer: Value::Bool(true),
            explanation: "source-backed explanation".into(),
            distractors: Vec::new(),
            tol: None,
            difficulty: None,
        };
        let (score, _) = evaluate(ActivityKind::TrueFalse, &config, &Value::Bool(false)).unwrap();
        assert_eq!(score, 0.0);
    }

    #[test]
    fn recommendation_prefers_in_progress_then_first_unsatisfied() {
        let lesson = |id: LearningLessonId, status: LessonStatus| LessonView {
            id,
            title: String::new(),
            summary: String::new(),
            purpose: String::new(),
            position: 0,
            est: 10,
            generated: true,
            source: None,
            sources: Vec::new(),
            status,
            activities: Vec::new(),
            sections: Vec::new(),
        };
        let done_id = LearningLessonId::new();
        let in_progress_id = LearningLessonId::new();
        let fresh_id = LearningLessonId::new();
        let modules = vec![ModuleView {
            id: LearningModuleId::new(),
            title: "Module".into(),
            description: String::new(),
            position: 0,
            lessons: vec![
                lesson(done_id.clone(), LessonStatus::Completed),
                lesson(in_progress_id.clone(), LessonStatus::InProgress),
                lesson(fresh_id, LessonStatus::NotStarted),
            ],
        }];
        // 进行中的课时优先于任何后续课时。
        assert_eq!(recommend_next_lesson(&modules), Some(in_progress_id));
        // 没有进行中时取第一个未满足（未完成且未跳过）的课时：跳过不计。
        let skipped_id = LearningLessonId::new();
        let unsatisfied_id = LearningLessonId::new();
        let modules = vec![ModuleView {
            id: LearningModuleId::new(),
            title: "Module".into(),
            description: String::new(),
            position: 0,
            lessons: vec![
                lesson(done_id.clone(), LessonStatus::Completed),
                lesson(skipped_id, LessonStatus::Skipped),
                lesson(unsatisfied_id.clone(), LessonStatus::NotStarted),
            ],
        }];
        assert_eq!(recommend_next_lesson(&modules), Some(unsatisfied_id));
    }

    #[tokio::test]
    async fn imports_enrolls_and_seeds_review_queue() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let course = service.import_course(valid_pack()).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        assert_eq!(
            detail.next_lesson_id.as_ref(),
            Some(&detail.modules[0].lessons[0].id)
        );
        let diagnostic = service
            .diagnostic_plan(&course.course.id, &user_id, 10)
            .await
            .unwrap();
        assert_eq!(diagnostic.items.len(), 1);
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        let lesson_id = detail.modules[0].lessons[0].id.clone();
        let result = service
            .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
            .await
            .unwrap();
        assert!(result.passed);
        service
            .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
            .await
            .unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        assert_eq!(detail.next_lesson_id, None);
        // Completing the lesson admits its concepts into the review queue
        // (immediately-due seed), but the seed must not count as a review:
        // counts stay at zero until the learner actually uses the queue.
        let (count, reviews): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(review_count), 0) FROM learning_review_items",
        )
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(reviews, 0);
    }

    #[tokio::test]
    async fn practice_flows_join_implicitly_without_explicit_enroll() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let course = service.import_course(valid_pack()).await.unwrap();
        let course_id = &course.course.id;
        // No explicit enroll anywhere: opening the detail must create the
        // enrollment so diagnostics, attempts and progress writes all work.
        let detail = service.course_detail(course_id, Some(&user_id)).await.unwrap();
        assert!(detail.enrollment_id.is_some());
        let diagnostic = service.diagnostic_plan(course_id, &user_id, 10).await.unwrap();
        assert_eq!(diagnostic.items.len(), 1);
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        let lesson_id = detail.modules[0].lessons[0].id.clone();
        let result = service
            .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
            .await
            .unwrap();
        assert!(result.passed);
        service
            .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
            .await
            .unwrap();
        // A second detail read must reuse the same enrollment (idempotent).
        let again = service.course_detail(course_id, Some(&user_id)).await.unwrap();
        assert_eq!(again.enrollment_id, detail.enrollment_id);
    }

    #[tokio::test]
    async fn question_entries_aligns_states_with_review_queue() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());

        // One lesson with one objective question per lesson: completing
        // lesson A seeds a review item for its own question A1, lesson B is
        // never touched.
        let shared = ActivityPack {
                        difficulty: None,
            kind: ActivityKind::TrueFalse,
            prompt: String::new(),
            options: Vec::new(),
            answer: Value::Bool(true),
            explanation: String::new(),
            distractors: Vec::new(),
            tol: None,
            section_key: None,
        };
        let pack = CoursePack {
            title: "Shared Concepts".into(),
teaching_style: crate::models::TeachingStyle::Standard,
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            modules: vec![ModulePack {
                title: "Module".into(),
                description: String::new(),
                lessons: vec![
                    LessonPack {
                        title: "Lesson A".into(),
                        summary: String::new(),
                        purpose: String::new(),
                        est: 10,
                        source: None,
                        sources: Vec::new(),
                        activities: vec![ActivityPack {
                        difficulty: None,
                            prompt: "A1".into(),
                            ..shared.clone()
                        }],
                        sections: Vec::new(),
                    },
                    LessonPack {
                        title: "Lesson B".into(),
                        summary: String::new(),
                        purpose: String::new(),
                        est: 10,
                        source: None,
                        sources: Vec::new(),
                        activities: vec![ActivityPack {
                        difficulty: None,
                            prompt: "A2".into(),
                            ..shared
                        }],
                        sections: Vec::new(),
                    },
                ],
            }],
        };
        let course = service.import_course(pack).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        let lesson_a = &detail.modules[0].lessons[0];
        service
            .update_lesson_progress(&lesson_a.id, &user_id, LessonStatus::Completed)
            .await
            .unwrap();

        fn state_of<'a>(entries: &'a [QuestionEntry], prompt: &str) -> Option<&'a str> {
            entries
                .iter()
                .find(|entry| entry.prompt.as_deref() == Some(prompt))
                .map(|entry| entry.state.as_str())
        }
        let entries = service
            .question_entries(&user_id, None, None, None)
            .await
            .unwrap();
        // Lesson B is not completed: A2 must not claim a queue state even
        // though its activity has no review item (only A1 was seeded).
        assert_eq!(state_of(&entries, "A1"), Some("new"));
        assert_eq!(state_of(&entries, "A2"), Some("unlearned"));

        // Simulate an overdue, already-reviewed concept: lesson A's row turns
        // due while lesson B's row stays unlearned, so the question-manager
        // counts agree with the review queue.
        sqlx::query("UPDATE learning_review_items SET review_count = 1, due_at = ?")
            .bind(now_ms() - 1000)
            .execute(database.pool())
            .await
            .unwrap();
        let entries = service
            .question_entries(&user_id, None, None, None)
            .await
            .unwrap();
        assert_eq!(state_of(&entries, "A1"), Some("due"));
        assert_eq!(state_of(&entries, "A2"), Some("unlearned"));

        // The queue itself serves exactly the completed lesson's question.
        let due = service
            .due_reviews(&user_id, 30, &[], true, false, &[])
            .await
            .unwrap();
       assert_eq!(due.len(), 1);
        assert_eq!(due[0].question.prompt, "A1");
    }

    /// A reflection-only pack: AI grading is judged against the question and
    /// the lesson topic (title + summary), never a concept list (ADR-0009).
    fn reflection_pack() -> CoursePack {
        CoursePack {
            title: "Reflective Learning".into(),
teaching_style: crate::models::TeachingStyle::Standard,
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            modules: vec![ModulePack {
                title: "Foundations".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    est: 10,
                    source: None,
                    sources: Vec::new(),
                    activities: vec![ActivityPack {
            difficulty: None,
                        kind: ActivityKind::Reflection,
                        prompt: "Explain what a vector is.".into(),
                        options: Vec::new(),
                        answer: Value::Null,
                        explanation: "A vector has magnitude and direction.".into(),
                    distractors: Vec::new(),
                    tol: None,
                    section_key: None,
                    }],
                    sections: Vec::new(),
                }],
            }],
        }
    }

    /// Scripted `LearningCompleter` recording calls, the last user message
    /// and the last explicit `(provider_id, model)` override; `fail` makes
    /// every call error out so fallback paths can be exercised.
    struct ScriptedCompleter {
        reply: String,
        fail: bool,
        calls: AtomicUsize,
        last_user: Mutex<Option<String>>,
        last_override: Mutex<Option<(String, String)>>,
    }

    impl ScriptedCompleter {
        fn new(reply: impl Into<String>, fail: bool) -> Arc<Self> {
            Arc::new(Self {
                reply: reply.into(),
                fail,
                calls: AtomicUsize::new(0),
                last_user: Mutex::new(None),
                last_override: Mutex::new(None),
            })
        }
    }

    #[async_trait::async_trait]
    impl LearningCompleter for ScriptedCompleter {
        async fn complete(
            &self,
            model_override: Option<(&str, &str)>,
            _system: &str,
            user: &str,
            _max_tokens: u32,
        ) -> Result<String, AppError> {
            self.record(model_override, user)
        }

        /// 生长管线（教练批/概念评审）走 json 接缝（g4）：脚本 completer 同
        /// 一口径回包。
        async fn complete_json_with_thinking(
            &self,
            model_override: Option<(&str, &str)>,
            _system: &str,
            user: &str,
            _max_tokens: u32,
            _temperature: Option<f32>,
        ) -> Result<String, AppError> {
            self.record(model_override, user)
        }
    }

    impl ScriptedCompleter {
        fn record(
            &self,
            model_override: Option<(&str, &str)>,
            user: &str,
        ) -> Result<String, AppError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            *self.last_user.lock().unwrap() = Some(user.to_owned());
            *self.last_override.lock().unwrap() =
                model_override.map(|(id, model)| (id.to_owned(), model.to_owned()));
            if self.fail {
                return Err(AppError::Internal("model unavailable".into()));
            }
            Ok(self.reply.clone())
        }
    }

    /// 按序出稿的 completer（二轮 t1 重试机制测试用）：逐调用消费脚本，
    /// 末项无限重复（评审回包 = 最后一稿的合法 JSON，problems 缺省为空即
    /// 通过）。记录每次调用的 user 装配（断言重试反馈带定向指令）。
    struct SequencedCompleter {
        replies: std::sync::Mutex<Vec<String>>,
        calls: AtomicUsize,
        prompts: std::sync::Mutex<Vec<String>>,
    }

    impl SequencedCompleter {
        fn new(replies: Vec<String>) -> Arc<Self> {
            Arc::new(Self {
                replies: std::sync::Mutex::new(replies),
                calls: AtomicUsize::new(0),
                prompts: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn call_count(&self) -> usize {
            self.calls.load(AtomicOrdering::SeqCst)
        }

        fn prompt(&self, index: usize) -> Option<String> {
            self.prompts.lock().unwrap().get(index).cloned()
        }

        fn record(&self, user: &str) -> Result<String, AppError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.prompts.lock().unwrap().push(user.to_owned());
            let mut replies = self.replies.lock().unwrap();
            match replies.len() {
                0 => Err(AppError::Internal("empty script".into())),
                1 => Ok(replies[0].clone()),
                _ => Ok(replies.remove(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl LearningCompleter for SequencedCompleter {
        async fn complete(
            &self,
            _model_override: Option<(&str, &str)>,
            _system: &str,
            user: &str,
            _max_tokens: u32,
        ) -> Result<String, AppError> {
            self.record(user)
        }

        async fn complete_json_with_thinking(
            &self,
            _model_override: Option<(&str, &str)>,
            _system: &str,
            user: &str,
            _max_tokens: u32,
            _temperature: Option<f32>,
        ) -> Result<String, AppError> {
            self.record(user)
        }
    }

    /// LLM 调用进行中触发全局取消（mid-call 真中断测试）：取消先到，
    /// wrapper 丢弃回包——内层恰被调一次、零落库。
    struct CancelMidCallCompleter {
        service: LearningService,
        calls: AtomicUsize,
        reply: String,
    }

    #[async_trait::async_trait]
    impl LearningCompleter for CancelMidCallCompleter {
        async fn complete(
            &self,
            _model_override: Option<(&str, &str)>,
            _system: &str,
            _user: &str,
            _max_tokens: u32,
        ) -> Result<String, AppError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.service.cancel_generation();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(self.reply.clone())
        }

        async fn complete_json_with_thinking(
            &self,
            _model_override: Option<(&str, &str)>,
            _system: &str,
            _user: &str,
            _max_tokens: u32,
            _temperature: Option<f32>,
        ) -> Result<String, AppError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.service.cancel_generation();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(self.reply.clone())
        }
    }

    async fn reflection_service_with_completer(
        completer: Arc<ScriptedCompleter>,
    ) -> (LearningService, nomifun_db::SqlitePool, UserId, LearningActivityId) {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        *service.course_completer.write().unwrap() = Some(completer);
        let course = service.import_course(reflection_pack()).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        (service, database.pool().clone(), user_id, activity_id)
    }

    #[tokio::test]
    async fn reflection_ai_grading_uses_model_reply() {
        let completer = ScriptedCompleter::new(
            "{\"score\":0.75,\"feedback\":\"## 评价\\n方向正确，但推导不完整。\"}",
            false,
        );
        let (service, pool, user_id, activity_id) =
            reflection_service_with_completer(completer.clone()).await;
        let result = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("A vector is a quantity with magnitude and direction.".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.score, 0.75);
        assert!(result.passed);
        assert!(result.feedback.contains("方向正确"));
        assert_eq!(completer.calls.load(AtomicOrdering::SeqCst), 1);
        // The grading prompt carries the answer AND the lesson topic
        // (title/summary) — never a concept list (ADR-0009).
        let user = completer.last_user.lock().unwrap().clone().unwrap();
        assert!(user.contains("Explain what a vector is."));
        assert!(user.contains("A vector is a quantity"));
        assert!(user.contains("Vectors"));
        assert!(user.contains("Lesson topic"));
        let (score, feedback): (f64, String) =
            sqlx::query_as("SELECT score, feedback FROM learning_attempts WHERE activity_id = ?")
                .bind(activity_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(score, 0.75);
        assert_eq!(feedback, result.feedback);
        // Empty answers are rejected before any model call, exactly like the
        // rule-based evaluator.
        let error = service
            .submit_attempt(&activity_id, &user_id, Value::String("   ".into()), None, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("must not be empty"));
        assert_eq!(completer.calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[tokio::test]
    async fn reflection_ai_grading_tolerates_fenced_reply() {
        // Models habitually wrap the grading JSON in Markdown fences (or add
        // prose around it). The bare parser used to reject the whole reply,
        // so every answer silently degraded to "non-empty passes" — the
        // fenced reply must now parse and drive the score.
        let completer = ScriptedCompleter::new(
            "```json\n{\"score\":0.4,\"feedback\":\"## 评价\\n方向正确，但缺少关键步骤。\"}\n```",
            false,
        );
        let (service, pool, user_id, activity_id) =
            reflection_service_with_completer(completer.clone()).await;
        let result = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("A vector has magnitude.".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.score, 0.4);
        assert!(!result.passed);
        assert!(result.feedback.contains("方向正确"));
        assert_eq!(completer.calls.load(AtomicOrdering::SeqCst), 1);
        // The AI score, not the non-empty fallback, is persisted.
        let (score,): (f64,) =
            sqlx::query_as("SELECT score FROM learning_attempts WHERE activity_id = ?")
                .bind(activity_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(score, 0.4);
    }

    #[tokio::test]
    async fn reflection_ai_grading_failures_surface_errors() {
        // AI grading is authoritative: a model call error, an unparseable
        // reply, or a missing completer must surface as an error instead of
        // silently degrading to "every non-empty answer passes".
        let failing = ScriptedCompleter::new(String::new(), true);
        let (service, _, user_id, activity_id) =
            reflection_service_with_completer(failing.clone()).await;
        let error = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("Vectors have magnitude and direction.".into()),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("model unavailable"));
        assert_eq!(failing.calls.load(AtomicOrdering::SeqCst), 1);

        // Unparseable reply: same surfaced error.
        let bad_reply = ScriptedCompleter::new("not json at all", false);
        *service.course_completer.write().unwrap() = Some(bad_reply.clone());
        let error = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("still a non-empty answer".into()),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("unparseable answer grading reply"));
        assert_eq!(bad_reply.calls.load(AtomicOrdering::SeqCst), 1);

        // No completer configured at all: surfaced error, not a pass.
        *service.course_completer.write().unwrap() = None;
        let error = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("plain answer".into()),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn reflection_ai_grading_forwards_explicit_model() {
        let completer = ScriptedCompleter::new(
            r#"{"score":0.6,"feedback":"ok"}"#,
            false,
        );
        let (service, _, user_id, activity_id) =
            reflection_service_with_completer(completer.clone()).await;
        let provider_id = ProviderId::new();
        service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("an answer".into()),
                Some(provider_id.clone()),
                Some("gpt-test".into()),
            )
            .await
            .unwrap();
        assert_eq!(
            *completer.last_override.lock().unwrap(),
            Some((provider_id.into_string(), "gpt-test".into()))
        );
        // Without a pair the default complete() path is used.
        service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("another answer".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(*completer.last_override.lock().unwrap(), None);
    }

    #[tokio::test]
    async fn objective_attempts_never_touch_the_completer() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let completer = ScriptedCompleter::new(String::new(), true);
        *service.course_completer.write().unwrap() = Some(completer.clone());
        let course = service.import_course(valid_pack()).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        let result = service
            .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
            .await
            .unwrap();
        assert!(result.passed);
        assert_eq!(result.feedback, "That is the geometric definition.");
        assert_eq!(completer.calls.load(AtomicOrdering::SeqCst), 0);
    }

    /// A fill-in-the-blank pack mirroring `valid_pack`: the blank sits at a
    /// relationship-critical spot, the accepted answer list tolerates case
    /// and whitespace variance, and the near-synonym distractor must never
    /// pass grading.
    fn fill_in_blank_pack() -> CoursePack {
        CoursePack {
            title: "Linear Algebra".into(),
            description: "A small generic course".into(),
            domain: "mathematics".into(),
            source_kb_id: None,
            version: 1,
            modules: vec![ModulePack {
                title: "Foundations".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    est: 10,
                    source: None,
                    sources: Vec::new(),
                    activities: vec![ActivityPack {
            difficulty: None,
                        kind: ActivityKind::FillInBlank,
                        prompt: "A vector has ___ and direction.".into(),
                        options: Vec::new(),
                        answer: json!(["magnitude"]),
                        explanation: "That is the geometric definition.".into(),
                        distractors: vec!["length".into()],
                        tol: None,
                        section_key: None,
                    }],
                    sections: Vec::new(),
                }],
            }],
            teaching_style: crate::models::TeachingStyle::Standard,
        }
    }

    #[tokio::test]
    async fn fill_in_blank_attempts_grade_against_accepted_answers() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let course = service.import_course(fill_in_blank_pack()).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        // The imported config keeps the near-synonym distractor so the blank
        // is graded against the accepted answers only, never the trap.
        let (config_json,): (String,) = sqlx::query_as(
            "SELECT config_json FROM learning_activities WHERE activity_id = ?",
        )
        .bind(activity_id.as_str())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let config: StoredActivityConfig = serde_json::from_str(&config_json).unwrap();
        assert_eq!(config.distractors, vec!["length"]);
        // Exact match passes; surrounding whitespace and case are ignored.
        let result = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("  Magnitude ".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(result.passed);
        assert_eq!(result.score, 1.0);
        // The near-synonym distractor is NOT an accepted answer: it fails,
        // which is exactly the fine discrimination the blank demands.
        let result = service
            .submit_attempt(&activity_id, &user_id, Value::String("length".into()), None, None)
            .await
            .unwrap();
        assert!(!result.passed);
        assert_eq!(result.score, 0.0);
        // Empty and non-string responses are rejected outright.
        let error = service
            .submit_attempt(&activity_id, &user_id, Value::String("   ".into()), None, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("must not be empty"));
        let error = service
            .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("must be a string"));
        // Once the lesson is completed, the blank joins the review queue as
        // an objective question like single choice and true/false.
        service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("magnitude".into()),
                None,
                None,
            )
            .await
            .unwrap();
        service
            .update_lesson_progress(
                &detail.modules[0].lessons[0].id,
                &user_id,
                LessonStatus::Completed,
            )
            .await
            .unwrap();
        let course_id = course.course.id.clone();
        // Fresh cards are due on the next review day, so the queue is empty
        // right after completion; roll the schedule forward to serve it.
        make_all_due(&service, &user_id).await;
        let due = service
            .due_reviews(&user_id, 30, &[course_id.clone()], true, false, &[])
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].question.prompt, "A vector has ___ and direction.");
        // The blank counts towards the course's due-review badge and shows up
        // in the question manager like any other objective question.
        let detail = service
            .course_detail(&course_id, Some(&user_id))
            .await
            .unwrap();
        assert_eq!(detail.due_review_count, 1);
        let entries = service
            .question_entries(&user_id, None, None, None)
            .await
            .unwrap();
        assert!(
            entries
                .iter()
                .any(|entry| entry.question_kind == Some(ActivityKind::FillInBlank)),
            "fill-in-the-blank activity must appear in the question manager"
        );
    }

    #[tokio::test]
    async fn review_queue_seeds_one_item_per_objective_question() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let pack = CoursePack {
            title: "Mixed".into(),
teaching_style: crate::models::TeachingStyle::Standard,
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            modules: vec![ModulePack {
                title: "Module".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    est: 10,
                    source: None,
                    sources: Vec::new(),
                    activities: vec![
                        ActivityPack {
                            difficulty: None,
                            kind: ActivityKind::SingleChoice,
                            prompt: "Which term names the size of a vector?".into(),
                            options: vec![
                                "magnitude".into(),
                                "speed".into(),
                                "velocity".into(),
                            ],
                            answer: json!("magnitude"),
                            explanation: String::new(),
                            distractors: Vec::new(),
                            tol: None,
                            section_key: None,
                        },
                        ActivityPack {
                            difficulty: None,
                            kind: ActivityKind::FillInBlank,
                            prompt: "A vector has ___ and direction.".into(),
                            options: Vec::new(),
                            answer: json!(["magnitude"]),
                            explanation: String::new(),
                            distractors: vec!["length".into()],
                            tol: None,
                            section_key: None,
                        },
                    ],
                    sections: Vec::new(),
                }],
            }],
        };
        let course = service.import_course(pack).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        let lesson_id = detail.modules[0].lessons[0].id.clone();
        service
            .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
            .await
            .unwrap();
        // Completing the lesson seeds one review item per objective question:
        // each card carries its own id, its own activity and its own schedule.
        // New cards surface on the next review day, not immediately.
        let scheduled = service
            .due_reviews(&user_id, 30, &[], false, false, &[])
            .await
            .unwrap();
        assert_eq!(scheduled.len(), 2);
        let due_now = service
            .due_reviews(&user_id, 30, &[], true, false, &[])
            .await
            .unwrap();
        assert_eq!(due_now.len(), 0, "fresh cards are not due the same day");
        make_all_due(&service, &user_id).await;
        let due = service
            .due_reviews(&user_id, 30, &[], true, false, &[])
            .await
            .unwrap();
        assert_eq!(due.len(), 2);
        assert_ne!(due[0].id, due[1].id);
        assert!(due.iter().all(|card| card.question.activity_id.is_some()));
        assert!(due.iter().any(|card| card.question.kind == ActivityKind::SingleChoice));
        assert!(due.iter().any(|card| card.question.kind == ActivityKind::FillInBlank));
        // The answer is graded against the item's own question: the blank's
        // item judges the blank, no card selection is needed.
        let blank = due
            .iter()
            .find(|card| card.question.kind == ActivityKind::FillInBlank)
            .unwrap();
        let result = service
            .answer_review(
                &blank.id,
                &user_id,
                Value::String("Magnitude".into()),
                false,
                None,
            )
            .await
            .unwrap();
        assert!(result.correct);
        // Rating one question advances only its own schedule: the sibling
        // item stays due, so the curves are fully independent.
        let choice = due
            .iter()
            .find(|card| card.question.kind == ActivityKind::SingleChoice)
            .unwrap();
        let before: (i64,) = sqlx::query_as(
            "SELECT due_at FROM learning_review_items WHERE review_item_id = ?",
        )
        .bind(blank.id.as_str())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let rated = service
            .rate_review(&choice.id, &user_id, ReviewRating::Good)
            .await
            .unwrap();
        assert!(rated.due_at > now_ms());
        let after: (i64,) = sqlx::query_as(
            "SELECT due_at FROM learning_review_items WHERE review_item_id = ?",
        )
        .bind(blank.id.as_str())
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(before.0, after.0, "rating one card must not move its sibling");
    }

    #[tokio::test]
    async fn custom_questions_persist_config_and_join_the_review_queue() {
        // 自建题契约：config 落库、进题库、按同一判卷器判分；填空题走 038
        // 的三题型宽口径 CHECK（迁移 066 重建必须保留）。
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let request = CreateCustomQuestionRequest {
            kind: ActivityKind::TrueFalse,
            prompt: "A vector has magnitude and direction.".into(),
            options: vec!["true".into(), "false".into()],
            answer: json!(true),
            explanation: "That is the geometric definition.".into(),
            distractors: Vec::new(),
        };
        let question_id = service
            .create_custom_question(&user_id, request)
            .await
            .unwrap();
        let (kind, config_json): (String, String) = sqlx::query_as(
            "SELECT kind, config_json FROM learning_custom_questions WHERE custom_question_id = ?",
        )
        .bind(&question_id)
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(kind, "true_false");
        let config: StoredActivityConfig = serde_json::from_str(&config_json).unwrap();
        assert_eq!(config.answer, json!(true));
        assert_eq!(config.options, vec!["true", "false"]);
        // The custom card joins the orphan queue due on the next review day
        // and is graded by the same rule-based evaluator.
        let before_due = service
            .due_reviews(&user_id, 30, &[], true, true, &[])
            .await
            .unwrap();
        assert_eq!(before_due.len(), 0, "fresh cards are not due the same day");
        make_all_due(&service, &user_id).await;
        let due = service
            .due_reviews(&user_id, 30, &[], true, true, &[])
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
        let result = service
            .answer_custom_review(&question_id, &user_id, Value::Bool(true), false)
            .await
            .unwrap();
        assert!(result.correct);
        // fill_in_blank 自建题在重建后的表上照常落库（066 曾收窄 CHECK，
        // 会让 038 之后合法的填空题必然 CHECK 失败——回归钉子）。
        let blank_id = service
            .create_custom_question(
                &user_id,
                CreateCustomQuestionRequest {
                    kind: ActivityKind::FillInBlank,
                    prompt: "Fill the blank: a vector has magnitude and ___.".into(),
                    options: Vec::new(),
                    answer: json!(["direction"]),
                    explanation: String::new(),
                    distractors: vec!["trap".into()],
                },
            )
            .await
            .unwrap();
        let (blank_kind, blank_config): (String, String) = sqlx::query_as(
            "SELECT kind, config_json FROM learning_custom_questions WHERE custom_question_id = ?",
        )
        .bind(&blank_id)
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(blank_kind, "fill_in_blank");
        let blank_config: StoredActivityConfig = serde_json::from_str(&blank_config).unwrap();
        assert_eq!(blank_config.distractors, vec!["trap".to_owned()]);
        // Payload validation rejects missing blanks, non-array answers and
        // the reflection kind, mirroring the generated side. These rejections
        // fire before any insert (pure validator checks).
        let invalid = |prompt: &str, answer: Value| CreateCustomQuestionRequest {
            kind: ActivityKind::FillInBlank,
            prompt: prompt.into(),
            options: Vec::new(),
            answer,
            explanation: String::new(),
            distractors: vec!["trap".into()],
        };
        let error = service
            .create_custom_question(&user_id, invalid("no blank here", json!(["x"])))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("___"));
        let error = service
            .create_custom_question(&user_id, invalid("A ___ blank.", json!("x")))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("JSON array"));
        let error = service
            .create_custom_question(
                &user_id,
                CreateCustomQuestionRequest {
                    kind: ActivityKind::Reflection,
                    prompt: "Reflect.".into(),
                    options: Vec::new(),
                    answer: Value::Null,
                    explanation: String::new(),
                    distractors: Vec::new(),
                },
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("AI-graded"));
    }

    async fn checkin_test_service() -> (LearningService, UserId) {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        (service, user_id)
    }

    async fn insert_review_event(service: &LearningService, user_id: &UserId, at: i64) {
        sqlx::query(
            "INSERT INTO learning_review_events (event_id, user_id, source, item_id, created_at) \
             VALUES (?, ?, 'course', ?, ?)",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(generate_id())
        .bind(at)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }

    /// 历史：j05 起打卡按 XP 口径（`checkin_today_locks_on_xp_gauge`），
    /// `learning.dailyCheckinGoal` 计数口径退役，其设置助手一并移除。

    async fn insert_due_custom_question(service: &LearningService, user_id: &UserId) {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO learning_custom_questions \
             (custom_question_id, user_id, kind, prompt, config_json, \
              due_at, stability_days, difficulty, review_count, lapse_count, \
              last_reviewed_at, created_at, updated_at) \
             VALUES (?, ?, 'true_false', 'p', '{\"options\":[\"true\",\"false\"],\"answer\":true,\"explanation\":\"\",\"matches\":[],\"distractors\":[]}', ?, 0, 5.0, 0, 0, NULL, ?, ?)",
        )
        .bind(LearningReviewItemId::new().into_string())
        .bind(user_id.as_str())
        .bind(now - 1000)
        .bind(now)
        .bind(now)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }

    /// Inserts a custom question due at an exact timestamp (for calendar
    /// due-bucket tests).
    async fn insert_custom_question_due_at(
        service: &LearningService,
        user_id: &UserId,
        due_at: i64,
    ) {
        sqlx::query(
            "INSERT INTO learning_custom_questions \
             (custom_question_id, user_id, kind, prompt, config_json, \
              due_at, stability_days, difficulty, review_count, lapse_count, \
              last_reviewed_at, created_at, updated_at) \
             VALUES (?, ?, 'true_false', 'p', '{\"options\":[\"true\",\"false\"],\"answer\":true,\"explanation\":\"\",\"matches\":[],\"distractors\":[]}', ?, 0, 5.0, 0, 0, NULL, ?, ?)",
        )
        .bind(LearningReviewItemId::new().into_string())
        .bind(user_id.as_str())
        .bind(due_at)
        .bind(due_at)
        .bind(due_at)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }

    /// YYYYMMDD integer for a local calendar date (review_day format).
    fn day_number(date: chrono::NaiveDate) -> i64 {
        i64::from(date.year()) * 10_000 + i64::from(date.month()) * 100 + i64::from(date.day())
    }

    async fn checkin_rows(service: &LearningService, user_id: &UserId) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_checkins WHERE user_id = ?")
            .bind(user_id.as_str())
            .fetch_one(service.pool_for_tests())
            .await
            .unwrap()
    }

    /// Seeds a completed check-in row for a specific review day (as the
    /// locking logic would, with a snapshot reviewed_count of 1).
    async fn insert_checkin(service: &LearningService, user_id: &UserId, review_day: i64) {
        sqlx::query(
            "INSERT INTO learning_checkins \
             (checkin_id, user_id, review_day, goal, reviewed_count, completed_at) \
             VALUES (?, ?, ?, 15, 1, ?)",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(review_day)
        .bind(now_ms())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }

    /// Seeds one course with one module and lesson, enrolled by `user_id`;
    /// returns (course_id, lesson_id, enrollment_id).
    async fn seed_course_with_lesson(
        service: &LearningService,
        user_id: &UserId,
        created_at: i64,
    ) -> (String, String, String) {
        let now = now_ms();
        let course_id = LearningCourseId::new().into_string();
        let module_id = LearningModuleId::new().into_string();
        let lesson_id = LearningLessonId::new().into_string();
        let enrollment_id = LearningEnrollmentId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, created_at, updated_at) \
             VALUES (?, 'Calendar test course', '', 'general', 1, ?, ?)",
        )
        .bind(&course_id)
        .bind(created_at)
        .bind(now)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
             VALUES (?, ?, 'Module', '', 0)",
        )
        .bind(&module_id)
        .bind(&course_id)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_lessons (lesson_id, module_id, title, summary, position) \
             VALUES (?, ?, 'Lesson', '', 0)",
        )
        .bind(&lesson_id)
        .bind(&module_id)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_enrollments \
             (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&enrollment_id)
        .bind(user_id.as_str())
        .bind(&course_id)
        .bind(now)
        .bind(now)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        (course_id, lesson_id, enrollment_id)
    }

    /// Marks a lesson as completed at `at` (started one minute earlier to
    /// satisfy the progress CHECK constraint).
    async fn complete_lesson(
        service: &LearningService,
        enrollment_id: &str,
        lesson_id: &str,
        at: i64,
    ) {
        sqlx::query(
            "INSERT INTO learning_lesson_progress \
             (enrollment_id, lesson_id, status, started_at, completed_at, updated_at) \
             VALUES (?, ?, 'completed', ?, ?, ?)",
        )
        .bind(enrollment_id)
        .bind(lesson_id)
        .bind(at - 60_000)
        .bind(at)
        .bind(at)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }

    /// Rolls every card of the user to "due right now". Fresh cards enter
    /// the queue due on the next review day; tests that exercise answering
    /// call this to fast-forward past the rollover.
    async fn make_all_due(service: &LearningService, user_id: &UserId) {
        let now = now_ms();
        sqlx::query(
            "UPDATE learning_review_items SET due_at = ? WHERE review_item_id IN \
             (SELECT r.review_item_id FROM learning_review_items r \
              JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
              WHERE e.user_id = ?)",
        )
        .bind(now - 1000)
        .bind(user_id.as_str())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        sqlx::query("UPDATE learning_custom_questions SET due_at = ? WHERE user_id = ?")
            .bind(now - 1000)
            .bind(user_id.as_str())
            .execute(service.pool_for_tests())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn checkin_empty_queue_without_review_stays_open() {
        let (service, user_id) = checkin_test_service().await;
        // 收窄后的语义：零复习 + 空队列只是初始状态，绝不锁定“完成”。
        let status = service.checkin_today(&user_id).await.unwrap();
        assert_eq!(status.reviewed_count, 0);
        assert_eq!(status.due_count, 0);
        assert!(!status.completed, "no review action must never complete the day");
        assert_eq!(checkin_rows(&service, &user_id).await, 0);
    }

    /// j05 起锁定的唯一开关是今日 XP 达标；清空队列不再是完成条件
    /// （打卡计数口径退役）。
    #[tokio::test]
    async fn checkin_ignores_queue_state_and_counts_only_xp() {
        let (service, user_id) = checkin_test_service().await;
        // 队列已清空但没有 XP：不完成。
        let status = service.checkin_today(&user_id).await.unwrap();
        assert_eq!(status.due_count, 0);
        assert!(!status.completed, "an empty queue without XP is not a check-in");
        // 一笔 0 分结算（错答推进）也不完成：净入账仍为 0。
        sqlx::query(
            "INSERT INTO learning_xp_events \
             (event_id, user_id, ts, course_id, node_id, kind, earned_xp, xp_bonus, basis, ratio, origin, anchor) \
             VALUES (?, ?, ?, '0190f5fe-7c00-7a00-8abc-222222222222', '0190f5fe-7c00-7a00-8abc-333333333333', \
                     'review-completed', 0, 0, 'partial', 0.0, 'review-course', 'review:smoke:1')",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(now_ms())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        let status = service.checkin_today(&user_id).await.unwrap();
        assert_eq!(status.reviewed_count, 0);
        assert!(!status.completed, "a zero-xp day never completes");
    }

    #[tokio::test]
    async fn checkin_stays_locked_after_new_due_cards() {
        let (service, user_id) = checkin_test_service().await;
        // 先把日目标降到 1 并落一笔 XP 结算（j05 XP 口径锁定）。
        sqlx::query(
            "INSERT INTO client_preferences (key, value, updated_at) \
             VALUES ('learning.dailyXpGoal', '1', ?) \
             ON CONFLICT(key) DO UPDATE SET value = '1', updated_at = excluded.updated_at",
        )
        .bind(now_ms())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_xp_events \
             (event_id, user_id, ts, course_id, node_id, kind, earned_xp, xp_bonus, basis, ratio, origin, anchor) \
             VALUES (?, ?, ?, '0190f5fe-7c00-7a00-8abc-222222222222', '0190f5fe-7c00-7a00-8abc-333333333333', \
                     'node-completed', 12, 2, 'full', NULL, 'lesson-progress', 'node:smoke')",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(now_ms())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        let first = service.checkin_today(&user_id).await.unwrap();
        assert!(first.completed);
        // A card arriving later stays in the queue as extra work but does not
        // reopen the locked day, and the lock row is not duplicated.
        insert_due_custom_question(&service, &user_id).await;
        let second = service.checkin_today(&user_id).await.unwrap();
        assert!(second.completed);
        assert_eq!(second.due_count, 1);
        assert_eq!(checkin_rows(&service, &user_id).await, 1);
    }

    #[tokio::test]
    async fn checkin_rolls_over_on_new_review_day() {
        let (service, user_id) = checkin_test_service().await;
        // Lock yesterday's review day; today must still be open.
        let now = now_ms();
        let tz = SchedulerSettings::default().tz_offset_minutes;
        let yesterday_start = review_day_start_utc(now, tz) - 86_400_000;
        let yesterday = review_day_number(yesterday_start, tz);
        sqlx::query(
            "INSERT INTO learning_checkins \
             (checkin_id, user_id, review_day, goal, reviewed_count, completed_at) \
             VALUES (?, ?, ?, 15, 0, ?)",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(yesterday)
        .bind(now)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        // A due card today keeps the day open (an empty queue without review
        // would no longer complete it either).
        insert_due_custom_question(&service, &user_id).await;
        let status = service.checkin_today(&user_id).await.unwrap();
        assert_eq!(status.review_day, review_day_number(now, tz));
        assert_ne!(status.review_day, yesterday);
        assert_eq!(status.due_count, 1);
        assert!(!status.completed, "a new review day starts unchecked");
    }

    #[tokio::test]
    async fn calendar_buckets_by_review_day_and_tz() {
        let (service, user_id) = checkin_test_service().await;
        let tz = 480;
        // UTC 2026-08-01 06:00：tz=+480 视图下本地 8 月 1 日 14:00 → 复习日 20260801；
        // tz=-300 视图下本地 8 月 1 日 01:00（02:00 日界线前）→ 复习日 20260731，
        // 不出现在 8 月视图中。同一时刻在两种时区下归属不同复习日。
        let at = local_wall_clock_utc_ms(2026, 8, 1, 1, -300).unwrap();
        insert_review_event(&service, &user_id, at).await;
        let stats = service.calendar_stats(&user_id, tz, 2026, Some(8)).await.unwrap();
        assert_eq!(stats.year, 2026);
        assert_eq!(stats.month, Some(8));
        assert_eq!(stats.tz_offset, 480);
        assert_eq!(stats.days.len(), 31, "month view must zero-fill every day");
        assert_eq!(stats.days.first().unwrap().review_day, 20260801);
        assert_eq!(stats.days.last().unwrap().review_day, 20260831);
        let day = stats.days.iter().find(|d| d.review_day == 20260801).unwrap();
        assert_eq!(day.reviewed_count, 1);
        // 同一事件、另一时区（UTC-5）：本地 8 月 1 日 01:00 → 复习日 20260731，
        // 不出现在 8 月视图中。
        let west = service.calendar_stats(&user_id, -300, 2026, Some(8)).await.unwrap();
        let west_day = west.days.iter().find(|d| d.review_day == 20260801).unwrap();
        assert_eq!(west_day.reviewed_count, 0);
    }

    #[tokio::test]
    async fn calendar_buckets_due_count_by_review_day() {
        let (service, user_id) = checkin_test_service().await;
        let tz = SchedulerSettings::default().tz_offset_minutes;
        let now = now_ms();
        let today = review_day_number(now, tz);
        let ymd = chrono::NaiveDate::from_ymd_opt(
            (today / 10_000) as i32,
            ((today / 100) % 100) as u32,
            (today % 100) as u32,
        )
        .unwrap();
        // 过期卡片（due 早于今天）滚入今天：与复习横幅到期队列同口径
        insert_custom_question_due_at(&service, &user_id, now - 60_000).await;
        // 明天 03:00 与后天 04:00（本地）到期的卡片分别归各自复习日
        let tomorrow_ymd = ymd.succ();
        let day_after_ymd = ymd.succ().succ();
        let tomorrow_start = local_wall_clock_utc_ms(
            tomorrow_ymd.year(),
            tomorrow_ymd.month(),
            tomorrow_ymd.day(),
            2,
            tz,
        )
        .unwrap();
        let day_after_start = local_wall_clock_utc_ms(
            day_after_ymd.year(),
            day_after_ymd.month(),
            day_after_ymd.day(),
            2,
            tz,
        )
        .unwrap();
        insert_custom_question_due_at(&service, &user_id, tomorrow_start + 3_600_000).await;
        insert_custom_question_due_at(&service, &user_id, day_after_start + 4_360_000).await;

        let year = i64::from(ymd.year());
        let stats = service
            .calendar_stats(&user_id, tz, year, None)
            .await
            .unwrap();
        let today_day = stats.days.iter().find(|d| d.review_day == today).unwrap();
        assert_eq!(today_day.due_count, 1, "overdue cards roll into today");
        for (label, expected) in [
            ("tomorrow", day_number(tomorrow_ymd)),
            ("day after", day_number(day_after_ymd)),
        ] {
            if let Some(d) = stats.days.iter().find(|d| d.review_day == expected) {
                assert_eq!(d.due_count, 1, "{label} due must bucket to its review day");
            }
        }
        assert!(
            stats
                .days
                .iter()
                .filter(|d| {
                    d.review_day != today
                        && d.review_day != day_number(tomorrow_ymd)
                        && d.review_day != day_number(day_after_ymd)
                })
                .all(|d| d.due_count == 0),
            "days without due cards must be zero"
        );
    }

    #[tokio::test]
    async fn calendar_year_view_zero_fills_every_day() {
        let (service, user_id) = checkin_test_service().await;
        let stats = service.calendar_stats(&user_id, 480, 2026, None).await.unwrap();
        assert_eq!(stats.month, None);
        assert_eq!(stats.days.len(), 365, "year view must cover the whole year");
        assert_eq!(stats.days.first().unwrap().review_day, 20260101);
        assert_eq!(stats.days.last().unwrap().review_day, 20261231);
        assert!(stats.days.iter().all(|d| d.reviewed_count == 0 && !d.checkin_completed));
    }

    #[tokio::test]
    async fn calendar_details_scope_lessons_to_user_and_bucket_courses() {
        let (service, user_id) = checkin_test_service().await;
        let tz = 480;
        let created_at = local_wall_clock_utc_ms(2026, 8, 10, 10, tz).unwrap();
        let completed_at = local_wall_clock_utc_ms(2026, 8, 11, 10, tz).unwrap();
        let (course_id, lesson_id, enrollment_id) =
            seed_course_with_lesson(&service, &user_id, created_at).await;
        complete_lesson(&service, &enrollment_id, &lesson_id, completed_at).await;
        // 另一用户的进度不应混入本用户的课时明细；但课程创建是全局目录聚合
        // （不过滤用户），两个用户的课程都应出现在创建明细中。
        let other_user = UserId::new();
        let (_, other_lesson_id, other_enrollment_id) =
            seed_course_with_lesson(&service, &other_user, created_at).await;
        complete_lesson(&service, &other_enrollment_id, &other_lesson_id, completed_at).await;
        let stats = service.calendar_stats(&user_id, tz, 2026, Some(8)).await.unwrap();
        let created_day = stats.days.iter().find(|d| d.review_day == 20260810).unwrap();
        assert_eq!(created_day.created_courses.len(), 2, "course catalog is global");
        assert!(
            created_day.created_courses.iter().any(|c| c.course_id == course_id),
            "own course present"
        );
        assert!(
            created_day
                .created_courses
                .iter()
                .any(|c| c.title == "Calendar test course"),
            "catalog titles present"
        );
        let completed_day = stats.days.iter().find(|d| d.review_day == 20260811).unwrap();
        assert_eq!(completed_day.completed_lessons.len(), 1, "other users' progress must not leak");
        assert_eq!(completed_day.completed_lessons[0].lesson_id, lesson_id);
        assert_eq!(completed_day.completed_lessons[0].title, "Lesson");
    }

    #[tokio::test]
    async fn calendar_streak_stops_at_gap_and_zero_without_today() {
        let (service, user_id) = checkin_test_service().await;
        let tz = SchedulerSettings::default().tz_offset_minutes;
        let anchor = review_day_start_utc(now_ms(), tz) + 3_600_000; // 当天 03:00，属当天复习日
        let today = review_day_number(anchor, tz);
        // 今天 + 前 3 天完成，第 5 天缺失（断）但更早还有 → 从今天往前数 streak = 4。
        for offset in [0_i64, 1, 2, 3, 5] {
            insert_checkin(
                &service,
                &user_id,
                review_day_number(anchor - offset * 86_400_000, tz),
            )
            .await;
        }
        let stats = service
            .calendar_stats(&user_id, tz, today / 10_000, None)
            .await
            .unwrap();
        assert_eq!(stats.streak, 4);
        // 今天未完成（无今天行）→ streak = 0。
        let (service2, user_id2) = checkin_test_service().await;
        insert_checkin(
            &service2,
            &user_id2,
            review_day_number(anchor - 86_400_000, tz),
        )
        .await;
        let stats2 = service2
            .calendar_stats(&user_id2, tz, today / 10_000, None)
            .await
            .unwrap();
        assert_eq!(stats2.streak, 0);
    }

    /// 学习图课程的最小 DB 夹具（ADR-0009 概念网模型）：课程行 +
    /// 隐含模块 + 两个节点（节点A teaches 概念「向量基础」@teach，节点B
    /// assumes 同概念 @know——依赖由概念网折叠，没有先修边）+ 概念登记表行。
    async fn seed_graph_course(
        service: &LearningService,
    ) -> (String, String, String) {
        let pool = service.pool_for_tests();
        let course_id = nomifun_common::LearningCourseId::new();
        let module_id = nomifun_common::LearningModuleId::new();
        let lesson_a = nomifun_common::LearningLessonId::new();
        let lesson_b = nomifun_common::LearningLessonId::new();
        let concept_id = nomifun_common::LearningConceptId::new();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, course_kind, learning_goal, \
              learning_scope, created_at, updated_at) \
             VALUES (?, '图课程', '', 'general', 1, 'learning_graph', '学习目标', '', ?, ?)",
        )
        .bind(course_id.as_str())
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_modules \
             (module_id, course_id, title, description, position) VALUES (?, ?, '学习图', '', 0)",
        )
        .bind(module_id.as_str())
        .bind(course_id.as_str())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
              content_generated) VALUES (?, ?, '节点A', '', '起点', 0, 10, 0)",
        )
        .bind(lesson_a.as_str())
        .bind(module_id.as_str())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
              content_generated) VALUES (?, ?, '节点B', '', '下游', 1, 10, 0)",
        )
        .bind(lesson_b.as_str())
        .bind(module_id.as_str())
        .execute(pool)
        .await
        .unwrap();
        // 概念网：A 教「向量基础」到能教档，B 假定同一概念到知道档。
        sqlx::query(
            "INSERT INTO learning_concept_registry \
             (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
             VALUES (?, '向量基础', '[]', '', ?, ?)",
        )
        .bind(concept_id.as_str())
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        for (lesson, role, tier) in [
            (lesson_a.as_str(), "teaches", "teach"),
            (lesson_b.as_str(), "assumes", "know"),
        ] {
            sqlx::query(
                "INSERT INTO learning_lesson_concepts \
                 (lesson_id, concept_id, role, tier) VALUES (?, ?, ?, ?)",
            )
            .bind(lesson)
            .bind(concept_id.as_str())
            .bind(role)
            .bind(tier)
            .execute(pool)
            .await
            .unwrap();
        }
        (
            course_id.into_string(),
            lesson_a.into_string(),
            lesson_b.into_string(),
        )
    }

    /// 图课程的目录级删除（三轮 g3：账本永远保留，无连带删卡选项）：概念网
    /// 引用随课时行一并保留（复习体系继续引用），课程行消失即可——目录删除
    /// （软删语义，ADR-0009 后续裁定）：课程行留墓碑（名称 +
    /// deleted_at 状态位，遗留数据凭 course_id 归账），课时/模块/概念网边
    /// 物理删除，复习卡/作答/进度/注册保留；概念登记表
    /// 保留；供给侧随之清零，删课重建后新课从真·零基础起步。
    #[tokio::test]
    async fn graph_course_catalog_delete_tombstones_and_purges_content() {
        let (service, _knowledge, owner_id) = job_test_service().await;
        let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
        let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
        // 删除前：种子课时把「向量基础」教到能教档，已教账有供给
        //（taught_ledger 的键是 concept_id——供给侧行按 id 流转）。
        let pool = service.pool_for_tests();
        let concept_id: String = sqlx::query_scalar(
            "SELECT concept_id FROM learning_concept_registry WHERE canonical = '向量基础'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let ledger_before = service.taught_ledger().await.unwrap();
        assert!(ledger_before.contains_key(&concept_id), "{:?}", ledger_before.keys());
        // 删除前的学习记录锚点（注册/卡/进度）行数，删除后逐一核对保留。
        let enrollment_before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_enrollments WHERE course_id = ?",
        )
        .bind(&course_id)
        .fetch_one(pool)
        .await
        .unwrap();
        service.delete_course(&course, &owner_id).await.unwrap();
        // 墓碑：课程行还在，deleted_at 已置位；列表不再出现。
        let (title, deleted_at): (String, Option<i64>) = sqlx::query_as(
            "SELECT title, deleted_at FROM learning_courses WHERE course_id = ?",
        )
        .bind(&course_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(title, "图课程");
        assert!(deleted_at.is_some(), "tombstone must carry the deleted marker");
        let listed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_courses WHERE course_id = ? AND deleted_at IS NULL",
        )
        .bind(&course_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(listed, 0, "tombstoned course must leave the catalog list");
        // 内容删净：课时、概念网边全空。
        let lessons: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_lessons")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(lessons, 0, "course content must be purged on catalog delete");
        let refs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_lesson_concepts")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(refs, 0, "concept web refs go with their lessons");
        // 账本永远保留（三轮 g3）：注册行不删——卡/作答/进度凭它归账。
        let enrollment_after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_enrollments WHERE course_id = ?",
        )
        .bind(&course_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            enrollment_after, enrollment_before,
            "enrollment survives the delete (ledger is never wiped)"
        );
        // 概念登记表保留，但供给已清零——新课不再继承幽灵地基。
        let registry: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(registry, 1, "registry survives the delete");
        let ledger_after = service.taught_ledger().await.unwrap();
        assert!(
            !ledger_after.contains_key(&concept_id),
            "purged lessons must stop supplying coverage: {ledger_after:?}"
        );
    }

    /// 安排面收窄（三轮 g3「现存且启用」）：归档课的卡离开到期队列、恢复后
    /// 回来；队列收窄不拦已出卡的作答提交（历史面如实落账）；概况读数随
    /// enabled 与 due_today 反映。
    #[tokio::test]
    async fn archived_course_cards_leave_the_due_queue_and_come_back_on_restore() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let pack = CoursePack {
            title: "归档课".into(),
            teaching_style: crate::models::TeachingStyle::Standard,
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            modules: vec![ModulePack {
                title: "Module".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    est: 10,
                    source: None,
                    sources: Vec::new(),
                    activities: vec![ActivityPack {
                        difficulty: None,
                        kind: ActivityKind::SingleChoice,
                        prompt: "Pick the size of a vector.".into(),
                        options: vec!["magnitude".into(), "speed".into()],
                        answer: json!("magnitude"),
                        explanation: String::new(),
                        distractors: Vec::new(),
                        tol: None,
                        section_key: None,
                    }],
                    sections: Vec::new(),
                }],
            }],
        };
        let course = service.import_course(pack).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        service
            .update_lesson_progress(&detail.modules[0].lessons[0].id, &user_id, LessonStatus::Completed)
            .await
            .unwrap();
        make_all_due(&service, &user_id).await;
        let before = service
            .due_reviews(&user_id, 30, &[], true, false, &[])
            .await
            .unwrap();
        assert_eq!(before.len(), 1, "卡先在队列");

        // 归档：卡出队、概况 enabled=false；课程本体与作答记录零删零改。
        service
            .set_course_archived(&course.course.id, true)
            .await
            .unwrap();
        let due = service
            .due_reviews(&user_id, 30, &[], true, false, &[])
            .await
            .unwrap();
        assert!(due.is_empty(), "archived course cards leave the queue");
        let listed = service.list_courses(&user_id).await.unwrap();
        let summary = listed.iter().find(|c| c.id == course.course.id).unwrap();
        assert!(!summary.enabled, "summary carries the archived state");
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_review_items")
            .fetch_one(service.pool_for_tests())
            .await
            .unwrap();
        assert_eq!(rows, 1, "archiving deletes nothing");

        // 队列收窄不拦已出卡的作答提交（历史面如实落账）。
        let card = &before[0];
        let rated = service
            .rate_review(&card.id, &user_id, ReviewRating::Good)
            .await
            .unwrap_or_else(|error| panic!("answering an archived course card must work: {error}"));
        assert_eq!(rated.review_count, 1, "the rating landed");

        // 恢复：卡回到队列（评分已把它推到下一间隔，查在队而非查到期），
        // enabled 翻回。
        service
            .set_course_archived(&course.course.id, false)
            .await
            .unwrap();
        let after = service
            .due_reviews(&user_id, 30, &[], false, false, &[])
            .await
            .unwrap();
        assert_eq!(after.len(), 1, "restore returns the card to the queue");
        let listed = service.list_courses(&user_id).await.unwrap();
        assert!(listed.iter().find(|c| c.id == course.course.id).unwrap().enabled);
    }

    /// 改名（三轮 g3）：只动显示名；与现存课撞名 = 人面错误（409）；软删
    /// 墓碑不阻塞新名——删课 A 后，新课可以取回 A 的旧名。
    #[tokio::test]
    async fn rename_moves_title_only_and_tombstones_do_not_block() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let pack = |title: &'static str| CoursePack {
            title: title.into(),
            teaching_style: crate::models::TeachingStyle::Standard,
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            modules: vec![ModulePack {
                title: "Module".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "L".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    est: 10,
                    source: None,
                    sources: Vec::new(),
                    activities: Vec::new(),
                    sections: Vec::new(),
                }],
            }],
        };
        let alpha = service.import_course(pack("Alpha")).await.unwrap();
        let beta = service.import_course(pack("Beta")).await.unwrap();
        service.enroll(&alpha.course.id, &user_id).await.unwrap();

        // 改名生效，学习数据不动。
        service
            .rename_course(&alpha.course.id, "Alpha 改")
            .await
            .unwrap();
        let listed = service.list_courses(&user_id).await.unwrap();
        let renamed = listed.iter().find(|c| c.id == alpha.course.id).unwrap();
        assert_eq!(renamed.title, "Alpha 改");
        assert_eq!(renamed.total_lessons, 1, "rename moves nothing else");

        // 与现存课撞名 = 409。
        let clash = service.rename_course(&alpha.course.id, "Beta").await;
        assert!(matches!(clash, Err(AppError::Conflict(_))), "{clash:?}");

        // 删课 Alpha（墓碑）后，名字可以被取回——墓碑不阻塞新名。
        service.delete_course(&alpha.course.id, &user_id).await.unwrap();
        service
            .rename_course(&beta.course.id, "Alpha")
            .await
            .unwrap_or_else(|error| panic!("tombstone must not block the name: {error}"));
        // 空名拒绝。
        let blank = service.rename_course(&beta.course.id, "   ").await;
        assert!(blank.is_err());
    }

    /// 归档课的安排面动作给人面错误（可恢复），不是 404：手动生长触发
    /// 与结构重跑都报「课程已归档」。
    #[tokio::test]
    async fn archived_graph_course_rejects_arrangement_actions_with_human_error() {
        let (service, _knowledge, owner_id) = job_test_service().await;
        let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
        let user_id = UserId::parse(owner_id).unwrap();
        let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
        service
            .set_course_archived(&course, true)
            .await
            .unwrap();
        let kicked = service
            .kick_growth(&user_id, &course, true, None)
            .await
            .unwrap_err();
        assert!(
            kicked.to_string().contains("课程已归档"),
            "{kicked:?}"
        );
        // 恢复后同一动作放行：生长正常启动（Ok），不再是归档错误。
        service.set_course_archived(&course, false).await.unwrap();
        let kicked = service
            .kick_growth(&user_id, &course, true, None)
            .await
            .unwrap_or_else(|error| panic!("restored course must grow again: {error}"));
        assert!(kicked, "manual growth on a restored course kicks");
    }

    /// 学习图节点的内容生成只走 agent 引擎：未配置引擎时明确 Conflict
    /// （traditional 节点在同样配置下走 fallback，图节点不行）；而幂等
    /// 检查先于类型分流——已生成节点无论课程类型都直接返回现有视图。
    #[tokio::test]
    async fn graph_lesson_generation_requires_engine_and_stays_idempotent() {
        let (service, _knowledge, user) = job_test_service().await;
        let (_course_id, lesson_a, lesson_b) = seed_graph_course(&service).await;
        let request = GenerateLessonRequest {
            provider_id: None,
            model: None,
            feedback: None,
        };

        // 无引擎：图节点生成被拒，且不落任何内容。
        let error = service
            .generate_lesson_content(
                &user,
                &nomifun_common::LearningLessonId::parse(&lesson_b).unwrap(),
                &request,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(error, AppError::Conflict(ref message) if message.contains("agent engine")),
            "unexpected error: {error:?}"
        );
        let generated: i64 = sqlx::query_scalar("SELECT content_generated FROM learning_lessons WHERE lesson_id = ?")
            .bind(&lesson_b)
            .fetch_one(service.pool_for_tests())
            .await
            .unwrap();
        assert_eq!(generated, 0, "a rejected graph lesson must not be persisted");

        // 幂等路径：已生成节点直接返回现有视图，不再触碰引擎。
        sqlx::query("UPDATE learning_lessons SET summary = '## 已有内容', content_generated = 1 WHERE lesson_id = ?")
            .bind(&lesson_a)
            .execute(service.pool_for_tests())
            .await
            .unwrap();
        let view = service
            .generate_lesson_content(
                &user,
                &nomifun_common::LearningLessonId::parse(&lesson_a).unwrap(),
                &request,
            )
            .await
            .unwrap();
        assert_eq!(view.summary, "## 已有内容");
    }

// ==== review log & due-ness gate（迁移 052，ADR-0005 同批）====

/// 全部日志行，按写入序：(source, rating, rating_source)。
async fn review_log_rows(service: &LearningService) -> Vec<(String, i64, String)> {
    sqlx::query_as(
        "SELECT source, rating, rating_source FROM learning_review_log \
         ORDER BY created_at, log_id",
    )
    .fetch_all(service.pool_for_tests())
    .await
    .unwrap()
}

async fn review_log_count(service: &LearningService) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM learning_review_log")
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap()
}

/// 单题课程包：一道单选题（答案 magnitude），用于复习日志与门禁测试。
fn review_log_test_pack() -> CoursePack {
    CoursePack {
        title: "Log".into(),
        teaching_style: crate::models::TeachingStyle::Standard,
        description: String::new(),
        domain: "general".into(),
        source_kb_id: None,
        version: 1,
        modules: vec![ModulePack {
            title: "Module".into(),
            description: String::new(),
            lessons: vec![LessonPack {
                title: "Lesson".into(),
                summary: String::new(),
                purpose: String::new(),
                est: 10,
                source: None,
                sources: Vec::new(),
                activities: vec![ActivityPack {
                    difficulty: None,
                    kind: ActivityKind::SingleChoice,
                    prompt: "Which term names the size of a vector?".into(),
                    options: vec!["magnitude".into(), "speed".into()],
                    answer: json!("magnitude"),
                    explanation: String::new(),
                    distractors: Vec::new(),
                    tol: None,
                    section_key: None,
                }],
                sections: Vec::new(),
            }],
        }],
    }
}

#[tokio::test]
async fn review_log_writes_advances_and_gate_blocks_stale_repeats() {
    let (service, user_id) = checkin_test_service().await;
    let course = service.import_course(review_log_test_pack()).await.unwrap();
    service.enroll(&course.course.id, &user_id).await.unwrap();
    let detail = service
        .course_detail(&course.course.id, Some(&user_id))
        .await
        .unwrap();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();
    // 完成课时只落一条 synthetic 种卡行，不是真实作答。
    assert_eq!(
        review_log_rows(&service).await,
        vec![("course".into(), 0, "synthetic".into())]
    );

    make_all_due(&service, &user_id).await;
    let due = service
        .due_reviews(&user_id, 10, &[], true, false, &[])
        .await
        .unwrap();
    assert_eq!(due.len(), 1);
    let card = due[0].id.clone();

    // 答错：自动 Again 是一次真实推进，落 auto 日志行。
    let wrong = service
        .answer_review(&card, &user_id, json!("speed"), false, Some(4_200_i64))
        .await
        .unwrap();
    assert!(!wrong.correct);
    assert!(wrong.rated.is_some());
    assert!(wrong.advanced);
    assert_eq!(
        review_log_rows(&service).await,
        vec![
            ("course".into(), 0, "synthetic".into()),
            ("course".into(), 1, "auto".into()),
        ]
    );
    // 作答耗时落在 attempt 行上。
    let elapsed: Option<i64> =
        sqlx::query_scalar("SELECT elapsed_ms FROM learning_attempts")
            .fetch_one(service.pool_for_tests())
            .await
            .unwrap();
    assert_eq!(elapsed, Some(4_200));

    // 重学步把卡排到几分钟后：立刻重复作答=未到期的重复——作答照记
    // （计正确率与诊断），但不推进、不落日志、不进打卡。
    let repeat = service
        .answer_review(&card, &user_id, json!("speed"), false, None)
        .await
        .unwrap();
    assert!(!repeat.correct);
    assert!(repeat.rated.is_none());
    assert!(!repeat.advanced);
    assert_eq!(review_log_count(&service).await, 2);
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_review_events WHERE item_id = ?",
    )
    .bind(card.as_str())
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(events, 1, "被门挡下的重复作答不得计入每日打卡");

    // 卡重新到期（模拟几分钟后）：这次作答合法，自评 Good 推进并落
    // self 行，带推进前记忆快照。
    sqlx::query("UPDATE learning_review_items SET due_at = ? WHERE review_item_id = ?")
        .bind(now_ms() - 1000)
        .bind(card.as_str())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    let correct = service
        .answer_review(&card, &user_id, json!("magnitude"), false, None)
        .await
        .unwrap();
    assert!(correct.correct);
    assert!(correct.rated.is_none());
    let rated = service
        .rate_review(&card, &user_id, ReviewRating::Good)
        .await
        .unwrap();
    assert!(rated.advanced);
    let rows = review_log_rows(&service).await;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2], ("course".into(), 3, "self".into()));
    let (stability_before, r_pred): (Option<f64>, Option<f64>) = sqlx::query_as(
        "SELECT stability_before, r_pred FROM learning_review_log \
         WHERE rating_source = 'self'",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert!(stability_before.unwrap() > 0.0, "重学推进携带推进前稳定度");
    assert!(r_pred.unwrap() > 0.0 && r_pred.unwrap() <= 1.0);

    // 过期 UI 数秒后再次自评：未到期且本学习日已推进过 → 门拦下，
    // 排期与日志原样不动。
    let stale = service
        .rate_review(&card, &user_id, ReviewRating::Easy)
        .await
        .unwrap();
    assert!(!stale.advanced);
    assert_eq!(stale.due_at, rated.due_at);
    assert_eq!(stale.stability_days, rated.stability_days);
    assert_eq!(review_log_count(&service).await, 3);
}

#[tokio::test]
async fn due_gate_admits_first_same_day_early_review() {
    let (service, user_id) = checkin_test_service().await;
    let course = service.import_course(review_log_test_pack()).await.unwrap();
    service.enroll(&course.course.id, &user_id).await.unwrap();
    let detail = service
        .course_detail(&course.course.id, Some(&user_id))
        .await
        .unwrap();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();
    // 新卡的 due 在下一个学习日：课程复习会话（due_only=false）会出示
    // 这张未到期卡。当日尚无任何推进 → 首次提前复习放行。
    let due = service
        .due_reviews(&user_id, 10, &[], false, false, &[])
        .await
        .unwrap();
    assert_eq!(due.len(), 1);
    let card = due[0].id.clone();
    let wrong = service
        .answer_review(&card, &user_id, json!("speed"), false, None)
        .await
        .unwrap();
    assert!(!wrong.correct);
    assert!(wrong.advanced);
    assert_eq!(
        review_log_rows(&service).await,
        vec![
            ("course".into(), 0, "synthetic".into()),
            ("course".into(), 1, "auto".into()),
        ]
    );
    // 同日再来的未到期作答 = 已推过又来：被门拦下。
    let repeat = service
        .answer_review(&card, &user_id, json!("speed"), false, None)
        .await
        .unwrap();
    assert!(!repeat.advanced);
    assert_eq!(review_log_count(&service).await, 2);
}

#[tokio::test]
async fn custom_review_log_writes_and_gate_blocks_stale_repeats() {
    let (service, user_id) = checkin_test_service().await;
    // 经服务 API 建题，保证 config_json 满足存储契约（不走测试直插）。
    let _card = service
        .create_custom_question(
            &user_id,
            CreateCustomQuestionRequest {
                kind: ActivityKind::TrueFalse,
                prompt: "FSRS schedules reviews per card.".into(),
                options: vec!["true".into(), "false".into()],
                answer: json!(true),
                explanation: String::new(),
                distractors: Vec::new(),
            },
        )
        .await
        .unwrap();
    make_all_due(&service, &user_id).await;
    let due = service
        .due_reviews(&user_id, 10, &[], true, true, &[])
        .await
        .unwrap();
    assert_eq!(due.len(), 1);
    let card = due[0].id.clone();

    // 答错自动 Again：真实推进，落 custom auto 行。
    let wrong = service
        .answer_custom_review(&card, &user_id, json!("wrong"), false)
        .await
        .unwrap();
    assert!(!wrong.correct);
    assert!(wrong.advanced);
    assert_eq!(
        review_log_rows(&service).await,
        vec![("custom".into(), 1, "auto".into())]
    );

    // 未到期的重复作答：反馈照给（判卷照常），但不推进。
    let repeat = service
        .answer_custom_review(&card, &user_id, json!("wrong"), false)
        .await
        .unwrap();
    assert!(!repeat.correct);
    assert!(!repeat.advanced);
    assert_eq!(review_log_count(&service).await, 1);

    // 重新到期后申报忘记：按答错记，落第二次推进。
    sqlx::query("UPDATE learning_custom_questions SET due_at = ? WHERE custom_question_id = ?")
        .bind(now_ms() - 1000)
        .bind(card.as_str())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    let forgot = service
        .answer_custom_review(&card, &user_id, Value::Null, true)
        .await
        .unwrap();
    assert!(!forgot.correct);
    assert!(forgot.advanced);
    assert_eq!(review_log_count(&service).await, 2);

    // 到期自评与过期重复自评：自评前先把卡拉回到期（forgot 又把排期
    // 推向了未来）。
    sqlx::query("UPDATE learning_custom_questions SET due_at = ? WHERE custom_question_id = ?")
        .bind(now_ms() - 1000)
        .bind(card.as_str())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    let rated = service
        .rate_custom_review(&card, &user_id, ReviewRating::Easy)
        .await
        .unwrap();
    assert!(rated.advanced);
    assert_eq!(review_log_count(&service).await, 3);
    let stale = service
        .rate_custom_review(&card, &user_id, ReviewRating::Good)
        .await
        .unwrap();
    assert!(!stale.advanced);
    assert_eq!(stale.due_at, rated.due_at);
    assert_eq!(review_log_count(&service).await, 3);
}

/// 插入一张带记忆状态、即时到期的自建卡，返回卡 id。
async fn insert_custom_card(
    service: &LearningService,
    user_id: &UserId,
    stability: f64,
    difficulty: f64,
) -> String {
    let id = LearningReviewItemId::new().into_string();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO learning_custom_questions \
         (custom_question_id, user_id, kind, prompt, config_json, \
          due_at, stability_days, difficulty, review_count, lapse_count, \
          last_reviewed_at, created_at, updated_at) \
         VALUES (?, ?, 'true_false', 'p', '{\"options\":[\"true\",\"false\"],\"answer\":true,\"explanation\":\"\",\"matches\":[],\"distractors\":[]}', ?, ?, ?, 1, 0, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id.as_str())
    .bind(now - 1000)
    .bind(stability)
    .bind(difficulty)
    .bind(now - 3 * 86_400_000)
    .bind(now)
    .bind(now)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn due_reviews_rank_by_retrievability_buckets() {
    let (service, user_id) = checkin_test_service().await;
    // 五张已见卡（3 天前推进，稳定度/难度各异）+ 一张未见卡。
    // R：稳定度越低风险越高；同桶内难度低（更易）的先出。
    let low = insert_custom_card(&service, &user_id, 1.0, 5.0).await;
    let mid = insert_custom_card(&service, &user_id, 5.0, 5.0).await;
    let easy = insert_custom_card(&service, &user_id, 10.0, 3.0).await;
    let mid2 = insert_custom_card(&service, &user_id, 30.0, 5.0).await;
    let hard = insert_custom_card(&service, &user_id, 10.0, 8.0).await;
    let unseen = insert_custom_card(&service, &user_id, 1.0, 5.0).await;
    sqlx::query(
        "UPDATE learning_custom_questions SET review_count = 0, last_reviewed_at = NULL \
         WHERE custom_question_id = ?",
    )
    .bind(&unseen)
    .execute(service.pool_for_tests())
    .await
    .unwrap();

    let due = service
        .due_reviews(&user_id, 10, &[], true, true, &[])
        .await
        .unwrap();
    assert_eq!(due.len(), 6);
    let order: Vec<String> = due.iter().map(|card| card.id.as_str().to_string()).collect();
    let pos = |id: &str| order.iter().position(|value| value == id).unwrap();
    // 遗忘风险降序（R 升序）：最不稳定的卡最先。
    assert!(pos(&low) < pos(&mid));
    assert!(due[0].r.unwrap() < due[pos(&mid)].r.unwrap());
    // 高 R 区聚在同一桶时，桶内按难度升序（先易后难）。
    assert!(pos(&easy) < pos(&mid2));
    assert!(pos(&mid2) < pos(&hard));
    // 排序键是五百分点桶：桶序单调不减（桶内按难度，原始 r 可局部倒序）；
    // 未见卡 r 为空且排最后。
    let buckets: Vec<i64> = due
        .iter()
        .filter_map(|card| card.r)
        .map(recall_bucket)
        .collect();
    assert!(buckets.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(due[5].id.as_str(), unseen);
    assert!(due[5].r.is_none());
}

#[tokio::test]
async fn memory_stats_aggregates_retention_calibration_and_load() {
    let (service, user_id) = checkin_test_service().await;
    let now = now_ms();
    let today = review_day_number(now, 480);
    // 手工摆日志：x 当日两推（首推=pass，其后同日重复按口径剔除）、
    // y 单推 fail、z 为 synthetic 排除。期望 True Retention = 1/2；
    // 校准分箱各含 1 条。
    let rows: Vec<(&str, &str, i64, &str, i64, Option<f64>, Option<f64>)> = vec![
        ("course", "x", 3, "self", 3, Some(5.0), Some(0.90)),
        ("course", "x", 1, "auto", 3, Some(5.0), Some(0.90)),
        ("custom", "y", 1, "auto", 7, Some(2.0), Some(0.60)),
        ("course", "z", 0, "synthetic", 0, None, None),
    ];
    for (index, (source, item, rating, rating_source, elapsed, stability_before, r_pred)) in
        rows.into_iter().enumerate()
    {
        sqlx::query(
            "INSERT INTO learning_review_log \
             (log_id, user_id, source, item_id, rating, rating_source, elapsed_days, \
              stability_before, difficulty_before, r_pred, review_day, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(source)
        .bind(item)
        .bind(rating)
        .bind(rating_source)
        .bind(elapsed)
        .bind(stability_before)
        .bind(stability_before.map(|_| 6.0))
        .bind(r_pred)
        .bind(today)
        .bind(now - 100 + index as i64)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }
    // 负载面：一张逾期卡（到期已过）+ 一张三天后的卡；再放一张已见卡
    // 供状态分布计数。
    insert_due_custom_question(&service, &user_id).await;
    insert_custom_question_due_at(&service, &user_id, now + 3 * 86_400_000).await;
    insert_custom_card(&service, &user_id, 2.0, 5.0).await;

    let stats = service.memory_health_stats(&user_id, 480).await.unwrap();
    let retention = stats.true_retention.expect("有真实到期推进即有保留率");
    assert_eq!((retention.passes, retention.fails), (1, 1));
    assert!((retention.rate.unwrap() - 0.5).abs() < 1e-9);

    // 校准：两条计入样本分别落在 0.90 与 0.60 预测桶，实测对比如下。
    let counted: Vec<_> = stats.calibration.iter().filter(|bin| bin.count > 0).collect();
    assert_eq!(counted.len(), 2);
    let pass_bin = counted.iter().find(|bin| bin.actual == Some(1.0)).unwrap();
    let fail_bin = counted.iter().find(|bin| bin.actual == Some(0.0)).unwrap();
    assert!((pass_bin.predicted - 0.90).abs() < 0.051);
    assert!((fail_bin.predicted - 0.60).abs() < 0.051);

    // 遗忘曲线：elapsed=3（pass）与 elapsed=7（fail）落在不同时点。
    assert!(stats
        .forgetting_curve
        .iter()
        .any(|point| point.count == 1 && point.actual == Some(1.0)));
    assert!(stats
        .forgetting_curve
        .iter()
        .any(|point| point.count == 1 && point.actual == Some(0.0)));

    // 负载预报：今天逾期 ≥1，第 3 天桶 ≥1。
    assert!(stats.overdue_count >= 1);
    let due_in_3 = review_day_number(now + 3 * 86_400_000, 480);
    assert!(stats
        .load_forecast
        .iter()
        .any(|day| day.review_day == due_in_3 && day.due_count >= 1));

    // 状态分布：两张自建卡中一张 new（未见）、一张已见。
    let bucket = |key: &str| {
        stats
            .state_distribution
            .iter()
            .find(|bucket| bucket.key == key)
            .unwrap()
            .count
    };
    assert!(bucket("new") >= 1);
    assert!(bucket("young") + bucket("mature") + bucket("master") >= 1);
}

/// 手动编辑节正文测试的种子：一条 traditional 课程（content_generated=1）
/// + 两节已 ready 的内容节 + 一节 practice 节 + 学习者注册。
async fn seed_sectioned_lesson(
    service: &LearningService,
    user_id: &UserId,
) -> (String, String) {
    let now = now_ms();
    let course_id = LearningCourseId::new().into_string();
    let module_id = LearningModuleId::new().into_string();
    let lesson_id = LearningLessonId::new().into_string();
    let enrollment_id = LearningEnrollmentId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, 'Edit test course', '', 'general', 1, ?, ?)",
    )
    .bind(&course_id)
    .bind(now)
    .bind(now)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, 'Module', '', 0)",
    )
    .bind(&module_id)
    .bind(&course_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lessons \
         (lesson_id, module_id, title, summary, position, purpose, estimated_minutes, \
          content_generated) \
         VALUES (?, ?, 'Lesson', 'body one\n\nbody two', 0, '', 10, 1)",
    )
    .bind(&lesson_id)
    .bind(&module_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    for (key, kind, title, position) in [
        ("s1", "concept", "Concept one", 0),
        ("s2", "concept", "Concept two", 1),
        ("s3", "practice", "Practice", 2),
    ] {
        let body = if kind == "practice" {
            String::new()
        } else {
            format!("body {key}")
        };
        sqlx::query(
            "INSERT INTO learning_lesson_sections \
             (section_key, lesson_id, kind, title, points, visual, body_md, status, version, \
              position, created_at, updated_at) \
             VALUES (?, ?, ?, ?, '', '', ?, 'ready', 1, ?, ?, ?)",
        )
        .bind(key)
        .bind(&lesson_id)
        .bind(kind)
        .bind(title)
        .bind(body)
        .bind(position)
        .bind(now)
        .bind(now)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO learning_enrollments \
         (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&enrollment_id)
    .bind(user_id.as_str())
    .bind(&course_id)
    .bind(now)
    .bind(now)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    (course_id, lesson_id)
}

#[tokio::test]
async fn manual_section_edit_updates_body_version_and_summary() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (_course_id, lesson_id) = seed_sectioned_lesson(&service, &owner).await;
    let lesson_id = LearningLessonId::parse(&lesson_id).unwrap();
    let request = crate::models::UpdateLessonSectionBodyRequest {
        body_md: "edited body".into(),
    };
    let view = service
        .update_lesson_section_body(&owner, &lesson_id, "s1", &request)
        .await
        .unwrap_or_else(|error| panic!("update failed: {error}"));
    let section = view
        .sections
        .iter()
        .find(|section| section.section_key == "s1")
        .unwrap();
    assert_eq!(section.body_md, "edited body");
    assert_eq!(section.status, "ready", "manual edit keeps the section ready");
    assert_eq!(section.version, 2, "manual edit bumps the section version");
    // summary 由全部节重新拼装：编辑后的正文进入，其他节不动。
    assert!(view.summary.contains("edited body"));
    assert!(view.summary.contains("body s2"));
    assert!(!view.summary.contains("body s1"), "old body is replaced");
}

#[tokio::test]
async fn manual_section_edit_rejects_practice_blank_and_unknown() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (_course_id, lesson_id) = seed_sectioned_lesson(&service, &owner).await;
    let lesson_id = LearningLessonId::parse(&lesson_id).unwrap();
    // 练习节不开放手动编辑（题目是一等实体）。
    let practice = service
        .update_lesson_section_body(
            &owner,
            &lesson_id,
            "s3",
            &crate::models::UpdateLessonSectionBodyRequest {
                body_md: "nope".into(),
            },
        )
        .await;
    assert!(matches!(practice, Err(AppError::Conflict(_))));
    // 空正文拒绝（清空走不了这条路径）。
    let blank = service
        .update_lesson_section_body(
            &owner,
            &lesson_id,
            "s1",
            &crate::models::UpdateLessonSectionBodyRequest {
                body_md: "   ".into(),
            },
        )
        .await;
    assert!(matches!(blank, Err(AppError::UnprocessableEntity(_))));
    // 未知节 404。
    let unknown = service
        .update_lesson_section_body(
            &owner,
            &lesson_id,
            "s99",
            &crate::models::UpdateLessonSectionBodyRequest {
                body_md: "nope".into(),
            },
        )
        .await;
    assert!(matches!(unknown, Err(AppError::NotFound(_))));
}

// ==== 概念网生长模型（ADR-0009）服务路径测试 ====

/// 就绪判定测试的小夹具：插入一门空学习图课程（课程行 + 隐含模块），
/// 返回 (course_id, module_id)。
async fn insert_graph_course_shell(
    service: &LearningService,
    title: &str,
) -> (String, String) {
    let pool = service.pool_for_tests();
    let course_id = nomifun_common::LearningCourseId::new();
    let module_id = nomifun_common::LearningModuleId::new();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, created_at, updated_at) \
         VALUES (?, ?, '', 'general', 1, 'learning_graph', ?, '', ?, ?)",
    )
    .bind(course_id.as_str())
    .bind(title)
    .bind(title)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_modules \
         (module_id, course_id, title, description, position) VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(module_id.as_str())
    .bind(course_id.as_str())
    .execute(pool)
    .await
    .unwrap();
    (course_id.into_string(), module_id.into_string())
}

/// 就绪判定测试的小夹具：铸入登记表概念，返回 concept_id。
async fn mint_registry_concept(service: &LearningService, canonical: &str) -> String {
    let concept_id = nomifun_common::LearningConceptId::new().into_string();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
         VALUES (?, ?, '[]', '', ?, ?)",
    )
    .bind(&concept_id)
    .bind(canonical)
    .bind(now)
    .bind(now)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    concept_id
}

/// 就绪判定测试的小夹具：插入一个学习节点并挂一条概念网边
/// （role: teaches|assumes，tier: know|apply|teach），返回 lesson_id。
async fn insert_graph_node_with_ref(
    service: &LearningService,
    module_id: &str,
    title: &str,
    concept_id: &str,
    role: &str,
    tier: &str,
) -> String {
    let pool = service.pool_for_tests();
    let lesson_id = nomifun_common::LearningLessonId::new().into_string();
    let position: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM learning_lessons WHERE module_id = ?",
    )
    .bind(module_id)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lessons \
         (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
          content_generated) VALUES (?, ?, ?, '', '', ?, 10, 0)",
    )
    .bind(&lesson_id)
    .bind(module_id)
    .bind(title)
    .bind(position)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lesson_concepts \
         (lesson_id, concept_id, role, tier) VALUES (?, ?, ?, ?)",
    )
    .bind(&lesson_id)
    .bind(concept_id)
    .bind(role)
    .bind(tier)
    .execute(pool)
    .await
    .unwrap();
    lesson_id
}

/// 服务路径的就绪判定（ADR-0009）：就绪 = 未开始 ∧ 每条 assumes 被
/// 跨课程的已教账以不低于假定档位的档位覆盖。A 课程教 X@teach 时 B 的
/// assumes X@apply/@teach 节点都可学；教学降档到 apply 后，假定 X@teach
/// 的节点退出就绪集——跨课程 + 档位偏序同时生效。
#[tokio::test]
async fn ready_set_follows_cross_course_tier_coverage() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());

    let (prep_course, prep_module) = insert_graph_course_shell(&service, "准备课").await;
    let concept_x = mint_registry_concept(&service, "特征值").await;
    insert_graph_node_with_ref(
        &service,
        &prep_module,
        "教特征值",
        &concept_x,
        "teaches",
        "teach",
    )
    .await;

    let (main_course, main_module) = insert_graph_course_shell(&service, "主干课").await;
    let main_course_id = nomifun_common::LearningCourseId::parse(&main_course).unwrap();
    let node_apply = insert_graph_node_with_ref(
        &service,
        &main_module,
        "应用特征值",
        &concept_x,
        "assumes",
        "apply",
    )
    .await;
    let node_teach = insert_graph_node_with_ref(
        &service,
        &main_module,
        "讲解特征值",
        &concept_x,
        "assumes",
        "teach",
    )
    .await;

    // 跨课程覆盖：准备课在另一门课程里教到「能教」，主干课的两个假定
    // 节点全部就绪（None = 零进度视角，无 enrollment）。
    let ready = service.ready_set_for(&main_course_id, None).await.unwrap();
    assert_eq!(ready.len(), 2);
    assert!(ready.iter().any(|id| id.as_str() == node_apply));
    assert!(ready.iter().any(|id| id.as_str() == node_teach));

    // 图视图（course_detail）暴露同一份就绪集与水位契约。
    let detail = service
        .course_detail(&main_course_id, Some(&user_id))
        .await
        .unwrap();
    let graph = detail
        .graph
        .expect("graph course detail carries the graph view");
    assert_eq!(graph.ready_count, 2);
    assert_eq!(graph.recommended.len(), 2);
    assert_eq!(graph.ready_target, crate::learning_graph::READY_TARGET);
    assert_eq!(graph.ready_trigger, crate::learning_graph::READY_TRIGGER);

    // 教学降档到「会用」：假定「能教」的节点不再就绪。
    sqlx::query(
        "UPDATE learning_lesson_concepts SET tier = 'apply' \
         WHERE role = 'teaches' AND concept_id = ?",
    )
    .bind(&concept_x)
    .execute(database.pool())
    .await
    .unwrap();
    let ready = service.ready_set_for(&main_course_id, None).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].as_str(), node_apply);
    let _ = prep_course;
}

async fn wait_for_growth_batch(service: &LearningService, course_id: &str) {
    for _ in 0..500 {
        // 只等 applied 批：pending 行在生长开始即落库，等它会抢在落库前返回。
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_growth_batches              WHERE course_id = ? AND status = 'applied'",
        )
        .bind(course_id)
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
        if count > 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("growth batch did not land in time");
}

/// 结构读侧投影（t1）：未生成 = 200 形状全空（Missing）；有快照 = 预计算面
/// 就位；软删 = 404。投影不触碰概念登记表——空登记表照常成影。
#[tokio::test]
async fn structure_view_serves_missing_filled_and_soft_deleted_courses() {
    let (service, _knowledge, _owner) = job_test_service().await;
    let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();

    // 未生成：200 恒答、structure/terrain 均 None。
    let view = service.structure_view(&course).await.unwrap();
    assert!(view.structure.is_none());
    assert!(view.terrain.is_none());
    assert!(view.generated_at.is_none());

    // 快照就位：预计算面（主干/块/边/门口）可见。
    let envelope = serde_json::json!({
        "version": 1,
        "generated_at": 1000,
        "structure": {
            "goal": "能独立求解", "goal_type": "capability",
            "proficiency_statement": "p", "domain_type": "理论",
            "form_reference": "F", "source_tier": "名录",
            "generated_by": "m", "disclaimer": "d",
            "domain": { "core_definition": "c", "includes": ["i1", "i2", "i3"],
                        "excludes": ["e1", "e2", "e3"], "prerequisite_doors": ["G1"],
                        "audience": "a", "scenarios": ["s1", "s2"], "granularity": "g" },
            "spines": [
                { "name": "主干一", "definition": "d", "key_points": ["k"], "instance": "i" }
            ],
            "blocks": [
                { "name": "块A", "slug": "a", "spine": "主干一", "stage": "入门", "note": "n" }
            ],
            "edges": [],
            "gaps": [{ "item": "g", "direction": "d" }],
            "quality": [{ "check": "c", "status": "met" }]
        },
        "terrain": {
            "goal": "能独立求解", "learner_basis_assumed": "纯零基础（未声明）",
            "generated_by": "m", "disclaimer": "d",
            "doors": [{ "id": "G1", "criterion": "能解 x²=p", "kind": "hard" }]
        }
    });
    sqlx::query("UPDATE learning_courses SET structure_json = ? WHERE course_id = ?")
        .bind(serde_json::to_string(&envelope).unwrap())
        .bind(&course_id)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    let view = service.structure_view(&course).await.unwrap();
    let structure = view.structure.expect("filled snapshot");
    assert_eq!(structure.scalars.goal, "能独立求解");
    assert_eq!(structure.blocks.len(), 1);
    assert_eq!(structure.blocks[0].layer, 0);
    assert!(structure.blocks[0].no_prereq);
    assert_eq!(structure.matrix[0].entryway, vec!["块A"]);
    let terrain = view.terrain.expect("terrain present");
    assert_eq!(terrain.doors[0].id, "G1");
    assert_eq!(view.generated_at, Some(1000));

    // 软删：墓碑课程 404（对齐软删守卫语义）。
    sqlx::query("UPDATE learning_courses SET deleted_at = 1 WHERE course_id = ?")
        .bind(&course_id)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    let error = service.structure_view(&course).await.unwrap_err();
    assert!(matches!(error, AppError::NotFound(_)), "{error:?}");
}

/// 生长落库路径（服务级）：scripted completer 让教练起草一个合法批次
/// （铸名 + 两个互不依赖的入口节点），kick_growth 后批次经结构门与概念评审
/// 落库——登记表、节点、概念网边、批次档案各就各位。
#[tokio::test]
async fn growth_applies_a_gated_batch_to_registry_and_lessons() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    // 同批互不依赖：第二节点只假定既往批次已教的「向量基础」（种子节点 A
    // 已教到能教档，已教账覆盖）——结构门放行；评审 completer 返回同 JSON，
    // problems 缺省为空即通过。
    let batch_json = r#"{
        "nodes": [
            {"title": "用定义计算极限", "purpose": "首批基节点", "est": 15,
             "teaches": [{"name": "极限", "tier": "know"}], "assumes": []},
            {"title": "用极限法则求切线", "purpose": "衔接节点", "est": 20,
             "teaches": [{"name": "求导法则", "tier": "apply"}],
             "assumes": [{"name": "向量基础", "tier": "know"}]}
        ],
        "mints": [
            {"canonical": "极限", "aliases": ["Limits"], "definition": "无限逼近的值"},
            {"canonical": "求导法则", "aliases": [], "definition": ""}
        ],
        "note": "首批两节点"
    }"#;
    let completer = ScriptedCompleter::new(batch_json, false);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();

    wait_for_growth_batch(&service, &course_id).await;
    let pool = service.pool_for_tests();
    // 登记表：夹具概念 + 本批铸名 2 条。
    let registry: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(registry, 3);
    // 节点：夹具 2 节点 + 本批 2 节点。
    let lessons: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lessons l \
         JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(lessons, 4);
    // 概念网：夹具 2 条 + 本批 3 条（teaches ×2 + assumes ×1）。
    let refs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lesson_concepts lc \
         JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
         JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(refs, 5);
    // 批次档案：seq 从 1 起，批注与本批一致；新节点默认内容未生成。
    let (seq, note): (i64, String) = sqlx::query_as(
        "SELECT seq, note FROM learning_growth_batches WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(seq, 1);
    assert_eq!(note, "首批两节点");
    let generated: i64 = sqlx::query_scalar(
        "SELECT MIN(content_generated) FROM learning_lessons l \
         JOIN learning_modules m ON m.module_id = l.module_id \
         WHERE m.course_id = ? AND l.position >= 2",
    )
    .bind(&course_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(generated, 0, "grown nodes start without content");
}

// ── 生长/结构循环行为加固（二轮 t1 + 三轮 t2）────────────────────────────

/// 无法解析引用的违规批（每轮结构门同一份 ng.refs.resolvable 报告——同指
/// 纹早停的脚本）。
const GHOST_REF_BATCH: &str = r#"{
    "nodes": [
        {"title": "幽灵节点", "purpose": "引用未铸名概念", "est": 10,
         "teaches": [{"name": "幽灵", "tier": "know"}], "assumes": []}
    ],
    "mints": [],
    "note": "违约稿"
}"#;

/// 合法过门批（与 growth_applies 同款；末项重复供评审回包）。
const VALID_BATCH: &str = r#"{
    "nodes": [
        {"title": "用定义计算极限", "purpose": "首批基节点", "est": 15,
         "teaches": [{"name": "极限", "tier": "know"}], "assumes": []},
        {"title": "用极限法则求切线", "purpose": "衔接节点", "est": 20,
         "teaches": [{"name": "求导法则", "tier": "apply"}],
         "assumes": [{"name": "向量基础", "tier": "know"}]}
    ],
    "mints": [
        {"canonical": "极限", "aliases": ["Limits"], "definition": "无限逼近的值"},
        {"canonical": "求导法则", "aliases": [], "definition": ""}
    ],
    "note": "首批两节点"
}"#;

async fn wait_for_batch_status(
    service: &LearningService,
    course_id: &str,
    status: &str,
) -> bool {
    for _ in 0..500 {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_growth_batches \
             WHERE course_id = ? AND status = ?",
        )
        .bind(course_id)
        .bind(status)
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
        if count > 0 {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    false
}

/// 同指纹早停（learnhub 口径：连败两次即停，调用数 2 非 3）：同一份违约
/// 稿两轮结构门打回，第二轮与第一轮指纹相同 → 立即 exhausted，第 3 轮的
/// LLM 调用不发生；smoke 读数 stop_reason = early_stop。
#[tokio::test]
async fn growth_stops_early_when_the_gate_report_repeats() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let completer = ScriptedCompleter::new(GHOST_REF_BATCH, false);
    *service.course_completer.write().unwrap() = Some(completer.clone());
    service.kick_growth(&owner, &course, true, None).await.unwrap();
    assert!(
        wait_for_batch_status(&service, &course_id, "failed").await,
        "growth must fail on the repeating violation"
    );
    assert_eq!(
        completer.calls.load(AtomicOrdering::SeqCst),
        2,
        "early stop: two calls, not three"
    );
    let stats = service.growth_run_stats().expect("growth stats recorded");
    assert_eq!(stats.stop_reason, "early_stop");
    assert_eq!(stats.attempts_used, 2);
}

/// 异错烧满预算：每轮违约不同（指纹互异）→ 3 轮全烧，exhausted。
#[tokio::test]
async fn growth_burns_the_full_budget_when_violations_differ() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    // 三种互异违约：未解析引用 / purpose 缺失 / 中文档位。
    let empty_purpose = r#"{
        "nodes": [{"title": "缺用途节点", "purpose": "", "est": 10, "teaches": [], "assumes": []}],
        "mints": [], "note": "n"
    }"#;
    let bad_tier = r#"{
        "nodes": [{"title": "档位节点", "purpose": "p", "est": 10,
                   "teaches": [{"name": "极限", "tier": "精通"}], "assumes": []}],
        "mints": [{"canonical": "极限", "definition": "d"}], "note": "n"
    }"#;
    let completer = SequencedCompleter::new(vec![
        GHOST_REF_BATCH.to_owned(),
        empty_purpose.to_owned(),
        bad_tier.to_owned(),
    ]);
    *service.course_completer.write().unwrap() = Some(completer.clone());
    service.kick_growth(&owner, &course, true, None).await.unwrap();
    assert!(
        wait_for_batch_status(&service, &course_id, "failed").await,
        "growth must exhaust the budget"
    );
    assert_eq!(completer.call_count(), 3, "distinct violations burn the budget");
    let stats = service.growth_run_stats().expect("growth stats recorded");
    assert_eq!(stats.stop_reason, "exhausted");
}

/// 容量守卫（重裁出口）：超容稿的 ng.batch.size 整批违规改判定向重装指令
/// （ng.batch.redraft_capacity），下一轮按拆分形态装箱后过门落库。
#[tokio::test]
async fn over_capacity_redraft_gets_the_directed_instruction() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let oversized = {
        let mut nodes = Vec::new();
        for index in 0..8 {
            nodes.push(format!(
                r#"{{"title": "超容节点{index}", "purpose": "p", "est": 10,
                    "teaches": [{{"name": "概念{index}", "tier": "know"}}], "assumes": []}}"#
            ));
        }
        let mints: Vec<String> = (0..8)
            .map(|index| format!(r#"{{"canonical": "概念{index}", "definition": "d"}}"#))
            .collect();
        format!(
            r#"{{"nodes": [{}], "mints": [{}], "note": "超容稿"}}"#,
            nodes.join(","),
            mints.join(",")
        )
    };
    let completer = SequencedCompleter::new(vec![
        GHOST_REF_BATCH.to_owned(),
        oversized,
        VALID_BATCH.to_owned(),
    ]);
    *service.course_completer.write().unwrap() = Some(completer.clone());
    service.kick_growth(&owner, &course, true, None).await.unwrap();
    wait_for_growth_batch(&service, &course_id).await;
    let applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_growth_batches \
         WHERE course_id = ? AND status = 'applied'",
    )
    .bind(&course_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(applied, 1, "the directed redraft lands the trimmed batch");
    // 第三次调用（第二轮重裁）的重试装配携带专用条款码与拆分指令。
    let third_prompt = completer.prompt(2).expect("third call prompt recorded");
    assert!(
        third_prompt.contains("ng.batch.redraft_capacity"),
        "the capacity violation is redirected, not just rejected"
    );
    assert!(third_prompt.contains("留待后续批次"));
}

/// 「未点名被改」机械守卫：打回报告只点名甲，重裁改了乙 → guard 失败 →
/// 原地重试恰一次（不占轮预算）；二次越权计轮，第三轮点修落位。
#[tokio::test]
async fn guard_failure_retries_in_place_once_then_counts_the_round() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    // 违约稿：甲引用幽灵（门报告点名甲），乙丙干净（未点名）。
    let draft = r#"{
        "nodes": [
            {"title": "甲节点", "purpose": "引用未铸名概念", "est": 10,
             "teaches": [{"name": "幽灵", "tier": "know"}], "assumes": []},
            {"title": "乙节点", "purpose": "乙的原用途", "est": 10, "teaches": [], "assumes": []},
            {"title": "丙节点", "purpose": "丙的原用途", "est": 10, "teaches": [], "assumes": []}
        ],
        "mints": [], "note": "违约稿"
    }"#;
    // 重裁#1：甲修复、乙被偷偷改动（未点名）→ guard，原地重试。
    let redraft_editing_unnamed = r#"{
        "nodes": [
            {"title": "甲节点", "purpose": "修复后", "est": 10,
             "teaches": [{"name": "极限", "tier": "know"}], "assumes": []},
            {"title": "乙节点", "purpose": "被偷改的用途", "est": 10, "teaches": [], "assumes": []},
            {"title": "丙节点", "purpose": "丙的原用途", "est": 10, "teaches": [], "assumes": []}
        ],
        "mints": [{"canonical": "极限", "definition": "d"}], "note": "越权稿"
    }"#;
    // 重裁#2（原地重试）：乙复原、丙被偷改（守卫文案只点名过乙，丙仍属
    // 未点名）→ 二次越权，计轮。
    let redraft_editing_unnamed_again = r#"{
        "nodes": [
            {"title": "甲节点", "purpose": "修复后", "est": 10,
             "teaches": [{"name": "极限", "tier": "know"}], "assumes": []},
            {"title": "乙节点", "purpose": "乙的原用途", "est": 10, "teaches": [], "assumes": []},
            {"title": "丙节点", "purpose": "被偷改的用途（第二次）", "est": 10, "teaches": [], "assumes": []}
        ],
        "mints": [{"canonical": "极限", "definition": "d"}], "note": "越权稿二"
    }"#;
    // 重裁#3：合法批落位（重试反馈的守卫文案点名乙，guard 放行）。
    let completer = SequencedCompleter::new(vec![
        draft.to_owned(),
        redraft_editing_unnamed.to_owned(),
        redraft_editing_unnamed_again.to_owned(),
        VALID_BATCH.to_owned(),
    ]);
    *service.course_completer.write().unwrap() = Some(completer.clone());
    service.kick_growth(&owner, &course, true, None).await.unwrap();
    wait_for_growth_batch(&service, &course_id).await;
    let applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_growth_batches \
         WHERE course_id = ? AND status = 'applied'",
    )
    .bind(&course_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(applied, 1, "the third-round point-fix lands");
    // 调用数：起草1 + 重裁1 + 原地重试1（乙越权）+ 计轮重裁1（丙越权）+
    // 第 3 轮重裁1（合法稿 vs 守卫基准触发甲「被移除」的原地重试）+ 评审1
    // = 6。**3 轮预算内落库**本身即证明原地重试未占轮预算——纯计轮会在
    // 第 3 轮耗尽后失败（5 调用即断）。
    assert_eq!(completer.call_count(), 6);
    let stats = service.growth_run_stats().expect("growth stats recorded");
    assert_eq!(stats.stop_reason, "success");
    assert_eq!(stats.attempts_used, 3);
}

/// mid-call 取消（三轮 t2 真中断）：取消在 LLM 调用内触发 → wrapper 丢弃
/// 回包 → 本批零落库、批行转 failed、任务终态 cancelled、内层恰被调一次。
#[tokio::test]
async fn mid_call_cancellation_discards_the_result_and_persists_nothing() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let completer = Arc::new(CancelMidCallCompleter {
        service: service.clone(),
        calls: AtomicUsize::new(0),
        reply: VALID_BATCH.to_owned(),
    });
    *service.course_completer.write().unwrap() = Some(completer.clone());
    service.kick_growth(&owner, &course, true, None).await.unwrap();
    assert!(
        wait_for_batch_status(&service, &course_id, "failed").await,
        "the cancelled growth fails with its pending batch marked"
    );
    // 零落库：种子 2 节点，无新增。
    let lessons: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lessons l \
         JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(lessons, 2, "the discarded batch persisted nothing");
    assert_eq!(
        completer.calls.load(AtomicOrdering::SeqCst),
        1,
        "the inner call ran exactly once"
    );
    let stats = service.growth_run_stats().expect("growth stats recorded");
    assert_eq!(stats.stop_reason, "cancelled");
    // 任务终态 = cancelled（取消先到，中性终态）。
    let snapshot = service.generation_task_snapshot();
    let growth = snapshot
        .iter()
        .find(|task| task.kind == crate::service::KIND_GROWTH)
        .expect("growth task recorded");
    assert_eq!(growth.state, "cancelled");
}

/// 结构生成同款早停：同一份违约产物两轮逐条款报告相同 → 第 2 轮即停
///（第 3 轮调用不发生；结构稿违约后地形稿不调用，调用数 = 2 非 6）。
#[tokio::test]
async fn structure_generation_stops_early_on_repeated_violations() {
    // spines 空 = gd.spines.count 红，永不可修正——每轮同一份逐条款报告。
    let bad_structure = serde_json::json!({
        "goal": "目标", "goal_type": "capability",
        "proficiency_statement": "程度",
        "domain_type": "理论",
        "form_reference": "Flowy 学习结构模板 v1",
        "source_tier": "名录",
        "generated_by": "test",
        "disclaimer": "d",
        "domain": {
            "core_definition": "c", "includes": ["i1", "i2", "i3"],
            "excludes": ["e1", "e2", "e3"], "prerequisite_doors": [],
            "audience": "a", "scenarios": ["s1", "s2"], "granularity": "g"
        },
        "spines": [], "blocks": [], "edges": [], "gaps": [], "quality": []
    });
    let completer = ScriptedCompleter::new(bad_structure.to_string(), false);
    let input = crate::learning_graph::structure_gen::StructureInput::new("目标", None, None);
    let error = crate::learning_graph::structure_gen::generate_structure_artifacts(
        completer.as_ref(),
        None,
        &input,
        None,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("均未通过"));
    // 早停：结构稿每轮违约即打回；每轮耗 2 次调用（结构稿 JSON 合法、
    // 门口对账在地形稿之后才红）——2 轮 = 4 次调用，第 3 轮的 2 次不发生。
    assert_eq!(
        completer.calls.load(AtomicOrdering::SeqCst),
        4,
        "structure early stop: two attempts, not three"
    );
}

/// 停摆臂（ADR-0010：只留空图一臂）：零节点课程自动触发退出（Ok(false)），
/// 手动 force 豁免（Ok(true)）。生长无自动终态——有节点的课程自动触发
/// 只看就绪水位，不存在「全部完成即停」的第二臂。
#[tokio::test]
async fn growth_stall_suppresses_auto_but_not_force() {
    let (service2, _knowledge2, owner2) = job_test_service().await;
    // 臂：零节点图（无课时）。
    let empty_course = nomifun_common::LearningCourseId::new();
    let module_id = nomifun_common::LearningModuleId::new();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, created_at, updated_at) \
         VALUES (?, '空图课', '', 'general', 1, 'learning_graph', '目标', '', ?, ?)",
    )
    .bind(empty_course.as_str())
    .bind(now)
    .bind(now)
    .execute(service2.pool_for_tests())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, position) \
         VALUES (?, ?, '学习图', 0)",
    )
    .bind(module_id.as_str())
    .bind(empty_course.as_str())
    .execute(service2.pool_for_tests())
    .await
    .unwrap();
    let kicked = service2
        .kick_growth(&owner2, &empty_course, false, None)
        .await
        .unwrap();
    assert!(!kicked, "empty graph must suppress auto trigger");
    // 手动 force 豁免停摆（返回 true 即已进入生长管道；无 completer 的失败
    // 只留日志，不影响本断言）。
    let forced = service2
        .kick_growth(&owner2, &empty_course, true, None)
        .await
        .unwrap();
    assert!(forced, "manual force must bypass the stall arm");
}

/// 空手批（0 节点）：pending 行删除不落档案——就绪已满时生长干净退出，
/// 不留历史噪音。
#[tokio::test]
async fn growth_idle_batch_leaves_no_batch_row() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let idle_json = r#"{
        "nodes": [],
        "mints": [],
        "note": "无新增"
    }"#;
    let completer = ScriptedCompleter::new(idle_json, false);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    let pool = service.pool_for_tests();
    let mut rows: i64 = -1;
    for _ in 0..500 {
        rows = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_growth_batches WHERE course_id = ?",
        )
        .bind(course.as_str())
        .fetch_one(pool)
        .await
        .unwrap();
        if rows == 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(rows, 0, "idle growth must not leave a batch row behind");
}

/// 触发接线：完成就绪节点使就绪存量下降 → 自动触发在后台补一批
/// （pending → applied 全链路）。
#[tokio::test]
async fn growth_auto_triggers_on_lesson_completion() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, lesson_a, _lesson_b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let batch_json = r#"{
        "nodes": [
            {"title": "用定义计算极限", "purpose": "后续节点", "est": 15,
             "teaches": [{"name": "极限", "tier": "know"}], "assumes": []}
        ],
        "mints": [ { "canonical": "极限", "aliases": [], "definition": "" } ],
        "note": "完成触发的补货批"
    }"#;
    let completer = ScriptedCompleter::new(batch_json, false);
    *service.course_completer.write().unwrap() = Some(completer);
    // 完成节点 A（隐式报名）：就绪 2 → 1 < 触发线 3 → 自动生长。
    let lesson = nomifun_common::LearningLessonId::parse(&lesson_a).unwrap();
    service
        .update_lesson_progress(&lesson, &owner, LessonStatus::Completed)
        .await
        .unwrap();
    wait_for_growth_batch(&service, &course_id).await;
    let (status, note): (String, String) = sqlx::query_as(
        "SELECT status, note FROM learning_growth_batches WHERE course_id = ?",
    )
    .bind(course.as_str())
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(status, "applied");
    assert_eq!(note, "完成触发的补货批");
}

/// 课程大纲引擎的测试替身（t1）：只记录收到的简报，返回固定的最小蓝图。
struct RecordingOutlineEngine {
    briefs: Mutex<Vec<crate::course_outline::OutlineBrief>>,
}

#[async_trait::async_trait]
impl crate::course_outline::CourseOutlineAgentEngine for RecordingOutlineEngine {
    async fn generate(
        &self,
        brief: &crate::course_outline::OutlineBrief,
        _model_override: Option<(&str, &str)>,
    ) -> Result<crate::generation::Blueprint, nomifun_common::AppError> {
        self.briefs.lock().unwrap().push(brief.clone());
        Ok(crate::generation::Blueprint {
            title: "模型自定名".into(),
            description: "d".into(),
            domain: "general".into(),
            version: 1,
            modules: vec![crate::generation::BlueprintModule {
                title: "M".into(),
                description: String::new(),
                lessons: vec![crate::generation::BlueprintLesson {
                    title: "L".into(),
                    purpose: "P".into(),
                    source: None,
                    sources: Vec::new(),
                }],
            }],
        })
    }
}

/// 知识库流的目标输入（t1）+ 目标感知采样（t2）：带目标时──归一目标进简报
/// （进快照与下游上下文）、课程标题即目标文本、learning_goal 列落库、采样
/// 按目标相关度选入并留下披露快照；不带目标时（旧客户端）行为与现状完全
/// 一致：主题回退库名、标题是模型蓝图标题、目标列为空、无采样披露。
#[tokio::test]
async fn kb_goal_flows_into_brief_title_and_course_row() {
    let (service, knowledge_service, owner_id) = job_test_service().await;
    let engine = Arc::new(RecordingOutlineEngine {
        briefs: Mutex::new(Vec::new()),
    });
    service.set_course_outline_engine(engine.clone());
    let base = knowledge_service
        .quick_create_base(Some("Math"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let kb_id: nomifun_common::KnowledgeBaseId = base.base.knowledge_base_id.parse().unwrap();
    let root = knowledge_service
        .content_root_for_base(kb_id.as_str())
        .await
        .unwrap();
    std::fs::create_dir_all(root.join("notes")).unwrap();
    std::fs::create_dir_all(root.join("misc")).unwrap();
    // 与目标无关的文件放在另一个目录：既不入选相关集合、也不在连贯预算的
    // 同目录兄弟范围内——低分未采清单因此有素材。
    std::fs::write(root.join("misc/a.md"), "# A\n正文").unwrap();
    // 目标相关文件与同目录兄弟各一份：相关度选入 + 连贯补入两条规则都上榜。
    std::fs::write(
        root.join("notes/python-数据分析.md"),
        "# Python 数据分析\n用 pandas 做分组聚合。",
    )
    .unwrap();
    std::fs::write(root.join("notes/附录.md"), "# 附录\n常见问题与术语表。").unwrap();

    let request = |goal: Option<&str>| GenerateCourseRequest {
        course_kind: crate::models::CourseKind::Traditional,
        teaching_style: None,
        knowledge_base_id: Some(kb_id.clone()),
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

    let with_goal = service
        .generate_course(&owner_id, request(Some("  零基础学 Python 数据分析 ")))
        .await
        .unwrap();
    let brief = engine.briefs.lock().unwrap().first().cloned().unwrap();
    assert_eq!(brief.goal.as_deref(), Some("零基础学 Python 数据分析"));
    assert_eq!(with_goal.course.title, "零基础学 Python 数据分析");
    assert_eq!(
        with_goal.course.learning_goal.as_deref(),
        Some("零基础学 Python 数据分析")
    );
    let sampled: Vec<&str> = brief.samples.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        sampled,
        vec!["notes/python-数据分析.md", "notes/附录.md"],
        "相关文件入样 + 同目录兄弟靠连贯预算补入（清单仍按字节序，与旧采样同尺）"
    );
    let sampling = brief.sampling.clone().expect("goal run discloses sampling");
    assert!(!sampling.goal_fallback);
    assert_eq!(sampling.total_files, 3);
    assert_eq!(sampling.sampled_files, 2);
    assert_eq!(sampling.coherence_files, 1, "附录.md 因同目录兄弟补入");
    assert_eq!(sampling.low_score_files, vec!["misc/a.md".to_owned()]);
    // 详情 DTO 同步暴露披露与目标（TS 镜像同形状）。
    let view = with_goal.course.sampling.clone().expect("course summary carries the disclosure");
    assert_eq!(view.sampled_files, 2);
    assert_eq!(view.total_files, 3);
    assert_eq!(view.coherence_files, 1);
    assert_eq!(view.low_score_examples, vec!["misc/a.md".to_owned()]);
    assert!(!view.goal_fallback);

    let legacy = service
        .generate_course(&owner_id, request(None))
        .await
        .unwrap();
    assert_eq!(legacy.course.title, "模型自定名");
    assert!(legacy.course.learning_goal.is_none());
    assert!(legacy.course.sampling.is_none(), "旧路径不产生采样披露");
    let legacy_brief = engine.briefs.lock().unwrap()[1].clone();
    assert_eq!(
        legacy_brief.samples.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(),
        vec!["misc/a.md", "notes/python-数据分析.md", "notes/附录.md"],
        "无目标回退字母序全采（旧行为）"
    );
}

/// 目标与库无交集（t2）：生成照常成功（退回默认采样），披露如实标注
/// fallback——目标措辞与库内容不一致时不能连课都建不出来。
#[tokio::test]
async fn kb_goal_without_any_match_falls_back_and_still_generates() {
    let (service, knowledge_service, owner_id) = job_test_service().await;
    let engine = Arc::new(RecordingOutlineEngine {
        briefs: Mutex::new(Vec::new()),
    });
    service.set_course_outline_engine(engine.clone());
    let base = knowledge_service
        .quick_create_base(Some("Trading"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let kb_id: nomifun_common::KnowledgeBaseId = base.base.knowledge_base_id.parse().unwrap();
    let root = knowledge_service
        .content_root_for_base(kb_id.as_str())
        .await
        .unwrap();
    std::fs::write(root.join("期权入门.md"), "# 期权入门\n认购与认沽的基本概念。").unwrap();

    let detail = service
        .generate_course(
            &owner_id,
            GenerateCourseRequest {
                course_kind: crate::models::CourseKind::Traditional,
                teaching_style: None,
                knowledge_base_id: Some(kb_id.clone()),
                description: None,
                domain: None,
                provider_id: None,
                model: None,
                mode: crate::models::CourseGenerationMode::OnDemand,
                goal: Some("零基础学 Python 数据分析".into()),
                proficiency_statement: None,
                learner_basis: None,
                knowledge_base_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(detail.course.total_lessons, 1, "生成照常完成");
    let brief = engine.briefs.lock().unwrap().first().cloned().unwrap();
    assert_eq!(
        brief.samples.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(),
        vec!["期权入门.md"],
        "退回默认字母序采样"
    );
    let sampling = brief.sampling.expect("fallback still discloses");
    assert!(sampling.goal_fallback);
    assert!(sampling.brief_line().contains("已按默认顺序采样 1/1 个文件"));
}

/// 目标进课时生成上下文（t1）：传统课时的目标从课程行 learning_goal 读出，
/// 预渲染进 LessonGenerationContext（既有空位）；旧课程（无目标）恒为 None。
#[tokio::test]
async fn lesson_context_carries_the_course_goal() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool()).await.unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    let lesson = |title: &str| LessonPack {
        title: title.into(),
        summary: String::new(),
        purpose: "目标一".into(),
        est: 10,
        source: None,
        sources: Vec::new(),
        activities: Vec::new(),
        sections: Vec::new(),
    };
    let pack = |title: &str| CoursePack {
        title: title.into(),
        description: "d".into(),
        domain: "math".into(),
        source_kb_id: None,
        version: 1,
        modules: vec![ModulePack {
            title: "模块一".into(),
            description: String::new(),
            lessons: vec![lesson("第一课")],
        }],
        teaching_style: crate::models::TeachingStyle::Standard,
    };
    let blueprint = crate::generation::Blueprint {
        title: "目标课程".into(),
        description: "d".into(),
        domain: "math".into(),
        version: 1,
        modules: vec![crate::generation::BlueprintModule {
            title: "模块一".into(),
            description: String::new(),
            lessons: vec![crate::generation::BlueprintLesson {
                title: "第一课".into(),
                purpose: "目标一".into(),
                source: None,
                sources: Vec::new(),
            }],
        }],
    };
    let import = |service: &LearningService, learning_goal: &str| {
        let pack = pack("目标课程");
        let blueprint = blueprint.clone();
        let goal = learning_goal.to_owned();
        let service = service.clone();
        async move {
            service
                .import_course_outline(
                    pack,
                    OutlineSnapshot {
                        blueprint_json: serde_json::to_string(&blueprint).unwrap(),
                        samples_json: "[]".to_owned(),
                        learning_goal: goal,
                        samples_meta_json: None,
                        source_kb_ids: Vec::new(),
                    },
                )
                .await
                .unwrap()
        }
    };

    let engine = Arc::new(RecordingLessonEngine {
        contexts: Mutex::new(Vec::new()),
    });
    service.set_lesson_engine(engine.clone());

    let with_goal = import(&service, "零基础学 Python 数据分析").await;
    let lesson_id = with_goal.modules[0].lessons[0].id.clone();
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();
    assert_eq!(
        engine.contexts.lock().unwrap()[0].goal.as_deref(),
        Some("零基础学 Python 数据分析")
    );

    let legacy = import(&service, "").await;
    let lesson_id = legacy.modules[0].lessons[0].id.clone();
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();
    assert!(engine.contexts.lock().unwrap()[1].goal.is_none());
}

// ── t3 课时写作接回知识库（实时检索 + 全文读取，快照降级为兜底） ─────────────

/// t3 课时读库验收台：真实 `KnowledgeService`（managed 库落在 tempdir 上，
/// 库根要活到用例结束——`TempDir` 一并返回）+ 课时生成服务（引擎/事件由用例
/// 注入）。
async fn grounding_test_service() -> (
    LearningService,
    Arc<KnowledgeService>,
    tempfile::TempDir,
    UserId,
) {
    let data_dir = tempfile::tempdir().unwrap();
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool()).await.unwrap();
    let owner = UserId::parse(&owner_id).unwrap();
    let knowledge = Arc::new(KnowledgeService::new(
        Arc::new(nomifun_db::SqliteKnowledgeRepository::new(database.pool().clone())),
        data_dir.path(),
        nomifun_knowledge::KnowledgeEventEmitter::new(
            Arc::new(NoopBroadcaster),
            Arc::from(owner_id),
        ),
    ));
    let service = LearningService::new(database.pool().clone());
    service.set_generation_dependencies(knowledge.clone(), Arc::new(UnusedCompleter));
    (service, knowledge, data_dir, owner)
}

/// 截获课时生成事件（t3 读数帧：命中数/扩读数/降级态）。
#[derive(Default)]
struct CapturingBroadcaster {
    events: Mutex<Vec<(String, serde_json::Value)>>,
}

impl nomifun_realtime::UserEventSink for CapturingBroadcaster {
    fn send_to_user(&self, _user_id: &str, event: WebSocketMessage<serde_json::Value>) {
        self.events.lock().unwrap().push((event.name, event.data));
    }
}

/// 往 managed 库根写一个 markdown 文件（建目录、写内容）。
fn write_kb_file(root: &str, rel: &str, content: &str) {
    let path = std::path::PathBuf::from(root).join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

/// 按字节数截前缀（对齐字符边界）——模拟旧采样：建课时冻结的 8KB 文件头。
fn prefix_bytes(text: &str, budget: usize) -> String {
    let mut end = budget.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// 建一份「一模块一课时、source 指向库内文件」的 outline 课程：samples 是
/// 快照里冻结的摘录（模拟建课时的采样结果）。`goal` 为空 = 旧课程形状。
async fn import_kb_lesson_course(
    service: &LearningService,
    source_kb_id: &str,
    source_path: &str,
    samples: Vec<(String, String)>,
    goal: &str,
) -> CourseDetail {
    import_kb_lesson_course_with_sources(service, source_kb_id, &[source_path], samples, goal).await
}

/// 同上，但课时可引用多处来源（t4 评审补齐的多引用）：`source_paths` 位序即
/// 引用顺序（首个 = 主来源），蓝图列表与兼容位都按它生成。
async fn import_kb_lesson_course_with_sources(
    service: &LearningService,
    source_kb_id: &str,
    source_paths: &[&str],
    samples: Vec<(String, String)>,
    goal: &str,
) -> CourseDetail {
    let sources: Vec<SourceSpan> = source_paths
        .iter()
        .map(|path| SourceSpan {
            path: (*path).into(),
            start: None,
            end: None,
            kb_id: None,
        })
        .collect();
    let pack = CoursePack {
        title: "期权入门".into(),
        description: "d".into(),
        domain: "finance".into(),
        source_kb_id: Some(source_kb_id.parse().unwrap()),
        version: 1,
        modules: vec![ModulePack {
            title: "模块一".into(),
            description: String::new(),
            lessons: vec![LessonPack {
                title: "期权入门".into(),
                summary: String::new(),
                purpose: "理解期权的定义与权利义务".into(),
                est: 10,
                source: sources.first().cloned(),
                sources: sources.clone(),
                activities: Vec::new(),
                sections: Vec::new(),
            }],
        }],
        teaching_style: crate::models::TeachingStyle::Standard,
    };
    let blueprint = crate::generation::Blueprint {
        title: "期权入门".into(),
        description: "d".into(),
        domain: "finance".into(),
        version: 1,
        modules: vec![crate::generation::BlueprintModule {
            title: "模块一".into(),
            description: String::new(),
            lessons: vec![crate::generation::BlueprintLesson {
                title: "期权入门".into(),
                purpose: "理解期权的定义与权利义务".into(),
                source: sources.first().cloned(),
                sources,
            }],
        }],
    };
    service
        .import_course_outline(
            pack,
            OutlineSnapshot {
                blueprint_json: serde_json::to_string(&blueprint).unwrap(),
                samples_json: serde_json::to_string(&samples).unwrap(),
                learning_goal: goal.to_owned(),
                // 旧课程形状：无采样披露（目标词条集也无从持久化）。
                samples_meta_json: None,
                source_kb_ids: Vec::new(),
            },
        )
        .await
        .unwrap()
}

/// t4 评审钉（课时多引用）：一份课时引用两处来源时——课时视图按引用位序列出
/// 两条（`source` 兼容位 = 首个、逐条解析库归属、落库列表与三列同源），课时
/// grounding 把**每个被引用文件**都作为 primary 片段（位序保留、各带原文与
/// 库归属），总预算不超过 40KB。
#[tokio::test]
async fn lesson_grounding_carries_every_cited_source_as_a_primary_fragment() {
    let (service, knowledge, _data_dir, user_id) = grounding_test_service().await;
    let base = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let kb_id = base.base.knowledge_base_id.as_str().to_owned();
    write_kb_file(
        &base.base.root_path,
        "docs/认购.md",
        "# 认购\n第一处来源标记：买方持有权利。\n",
    );
    write_kb_file(
        &base.base.root_path,
        "docs/认沽.md",
        "# 认沽\n第二处来源标记：卖方承担义务。\n",
    );
    // 第二条引用是大文件：预算守卫必须把两条 primary 合起来压到 40KB 以内。
    let filler = "填充段落，用于把文件推过单条主引用的 24KB 上限。\n\n";
    let mut big = String::from("# 认沽\n第二处来源标记：卖方承担义务。\n\n");
    for _ in 0..1500 {
        big.push_str(filler);
    }
    write_kb_file(&base.base.root_path, "docs/认沽.md", &big);

    let detail = import_kb_lesson_course_with_sources(
        &service,
        &kb_id,
        &["docs/认购.md", "docs/认沽.md"],
        vec![
            ("docs/认购.md".to_owned(), "快照一".to_owned()),
            ("docs/认沽.md".to_owned(), "快照二".to_owned()),
        ],
        "",
    )
    .await;

    // 视图：两条来源按引用位序，各自解析到唯一来源库；兼容位 = 首个。
    let lesson = &detail.modules[0].lessons[0];
    assert_eq!(
        lesson
            .sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<Vec<_>>(),
        vec!["docs/认购.md", "docs/认沽.md"],
        "视图按引用位序给出全部来源"
    );
    assert!(
        lesson.sources.iter().all(|source| source.kb_id.is_some()),
        "单库课程的裸路径回退唯一来源库（逐条）"
    );
    assert_eq!(
        lesson.source.as_ref().map(|source| source.path.as_str()),
        Some("docs/认购.md"),
        "兼容位 = 首个引用"
    );

    let lesson_id = lesson.id.clone();
    let engine = Arc::new(RecordingLessonEngine {
        contexts: Mutex::new(Vec::new()),
    });
    service.set_lesson_engine(engine.clone());
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();
    let contexts = engine.contexts.lock().unwrap();
    let grounding = contexts[0].grounding();
    let primaries: Vec<&LessonExcerpt> = grounding
        .iter()
        .filter(|fragment| fragment.role == ExcerptRole::Primary)
        .copied()
        .collect();
    assert_eq!(primaries.len(), 2, "每个被引用文件各成一条 primary（位序保留）");
    assert_eq!(primaries[0].path, "docs/认购.md");
    assert_eq!(primaries[1].path, "docs/认沽.md");
    assert!(primaries[0].text.contains("第一处来源标记"), "{}", primaries[0].text);
    assert!(primaries[1].text.contains("第二处来源标记"), "{}", primaries[1].text);
    assert!(
        primaries.iter().all(|fragment| fragment.kb_id.as_deref() == Some(kb_id.as_str())),
        "逐条实时读库各带库归属"
    );
    // 预算内联（与 `GROUNDING_TOTAL_MAX_BYTES` 同值，测试不依赖生产常量）。
    let total: usize = grounding.iter().map(|fragment| fragment.text.len()).sum();
    assert!(
        total <= 40 * 1024,
        "多引用合并后仍在 40KB 预算内: {total}"
    );
    assert!(contexts[0].grounding().first().is_some_and(|f| f.role == ExcerptRole::Primary));
}


#[tokio::test]
async fn lesson_grounding_reads_the_full_source_past_the_snapshot_prefix() {
    let (service, knowledge, _data_dir, user_id) = grounding_test_service().await;
    let base = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let kb_id = base.base.knowledge_base_id.as_str().to_owned();
    let filler = "这是一段与关键事实无关的填充内容，用于把关键段落推到大文件中段。\n\n";
    let mut big = String::from("# 期权入门\n\n开头噪声标记：本节只讲名词解释。\n\n");
    for _ in 0..400 {
        big.push_str(filler);
    }
    big.push_str("## 权利义务不对称\n\n关键事实标记：买方持有权利、卖方承担义务。\n\n");
    for _ in 0..400 {
        big.push_str(filler);
    }
    assert!(big.len() > 8 * 1024, "fixture must exceed the old 8KB prefix");
    write_kb_file(&base.base.root_path, "docs/期权入门.md", &big);
    // 快照 = 建课时冻结的 8KB 前缀：中段事实确实不在其中（夹具自检）。
    let snapshot_prefix = prefix_bytes(&big, 8 * 1024);
    assert!(
        !snapshot_prefix.contains("关键事实标记"),
        "the fact must sit past 8KB"
    );

    let detail = import_kb_lesson_course(
        &service,
        &kb_id,
        "docs/期权入门.md",
        vec![("docs/期权入门.md".to_owned(), snapshot_prefix)],
        "",
    )
    .await;
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    let engine = Arc::new(RecordingLessonEngine {
        contexts: Mutex::new(Vec::new()),
    });
    service.set_lesson_engine(engine.clone());
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();

    let contexts = engine.contexts.lock().unwrap();
    let grounding = contexts[0].grounding();
    assert_eq!(grounding.len(), 1, "单一来源文件：只有 primary");
    let primary = grounding[0];
    assert_eq!(primary.role, ExcerptRole::Primary);
    assert_eq!(primary.path, "docs/期权入门.md");
    assert_eq!(primary.kb_id.as_deref(), Some(kb_id.as_str()));
    assert!(
        primary.text.contains("关键事实标记：买方持有权利、卖方承担义务"),
        "8KB 之后的中段事实必须进入 grounding（全文读取 + 超预算按相关度选摘）"
    );
    assert!(
        primary.text.contains("截断"),
        "文件超过 primary 预算：截断在上下文里明示"
    );
    assert!(
        primary.text.len() <= 24 * 1024,
        "primary 守住 24KB 预算：{}",
        primary.text.len()
    );
}

/// t3 验收钉子②（陈旧快照解除）：建课后修改库文件 → 新生成的课时取到新内容
/// （快照旧、库新，实时优先）；已生成课时不因后续库改动而变（幂等重入）。
#[tokio::test]
async fn lesson_grounding_prefers_the_live_file_over_the_stale_snapshot() {
    let (service, knowledge, _data_dir, user_id) = grounding_test_service().await;
    let base = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let kb_id = base.base.knowledge_base_id.as_str().to_owned();
    write_kb_file(
        &base.base.root_path,
        "docs/期权入门.md",
        "# 期权入门\n\n旧版事实标记：期权只有欧式一种。\n",
    );
    let detail = import_kb_lesson_course(
        &service,
        &kb_id,
        "docs/期权入门.md",
        vec![(
            "docs/期权入门.md".to_owned(),
            "旧版事实标记：期权只有欧式一种。".to_owned(),
        )],
        "",
    )
    .await;
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    // 建课后修改知识库文件：课程行的快照仍是旧内容。
    write_kb_file(
        &base.base.root_path,
        "docs/期权入门.md",
        "# 期权入门\n\n新版事实标记：场内既有欧式也有美式期权。\n",
    );

    let engine = Arc::new(RecordingLessonEngine {
        contexts: Mutex::new(Vec::new()),
    });
    service.set_lesson_engine(engine.clone());
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();
    {
        let contexts = engine.contexts.lock().unwrap();
        let grounding = contexts[0].grounding();
        assert!(grounding[0].text.contains("新版事实标记"), "实时读取优先于快照");
        assert!(
            !grounding[0].text.contains("旧版事实标记"),
            "旧快照内容不再进 grounding"
        );
    }
    // 已生成课时不变：第二次调用是幂等重入（不再驱动引擎、内容不刷新）。
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();
    assert_eq!(
        engine.contexts.lock().unwrap().len(),
        1,
        "内容已生成的课时不因库改动而重新生成"
    );
}

/// t3 验收钉子③（锚定专项）：检索命中散落多个文件时，课时引用的主 source
/// 不被替换；扩读片段带库+文件（+命中章节）归属，可溯源；片段交付顺序按
/// 结构（路径字典序）而非检索分——阅读线不因检索结果被打乱。
#[tokio::test]
async fn lesson_grounding_keeps_the_cited_source_and_traces_supplements() {
    let (service, knowledge, _data_dir, user_id) = grounding_test_service().await;
    let base = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let kb_id = base.base.knowledge_base_id.as_str().to_owned();
    write_kb_file(
        &base.base.root_path,
        "docs/期权入门.md",
        "# 期权入门\n\n主线事实标记：期权的价值随标的价格变化。\n",
    );
    // 两个命中散落的旁支文件：检索分与路径序刻意相反（z- 相关性更强），
    // 交付顺序必须回到路径字典序——相关度决定读什么，结构决定按什么顺序读。
    write_kb_file(
        &base.base.root_path,
        "notes/a-期权简记.md",
        "# 期权简记\n\n补充事实标记甲：期权可以组合成价差。\n",
    );
    write_kb_file(
        &base.base.root_path,
        "notes/z-期权基础.md",
        "# 期权基础\n\n补充事实标记乙：权利金是期权的价格，买方持有权利、卖方承担义务。\n",
    );
    let detail = import_kb_lesson_course(
        &service,
        &kb_id,
        "docs/期权入门.md",
        vec![(
            "docs/期权入门.md".to_owned(),
            "旧快照标记：建课时冻结的摘录。".to_owned(),
        )],
        "",
    )
    .await;
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    let engine = Arc::new(RecordingLessonEngine {
        contexts: Mutex::new(Vec::new()),
    });
    service.set_lesson_engine(engine.clone());
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();

    let contexts = engine.contexts.lock().unwrap();
    let grounding = contexts[0].grounding();
    let primary = grounding
        .iter()
        .find(|excerpt| excerpt.role == ExcerptRole::Primary)
        .expect("primary 恒在");
    assert_eq!(primary.path, "docs/期权入门.md", "主 source 不被检索结果替换");
    assert!(
        primary.text.contains("主线事实标记"),
        "primary 是主 source 的全文"
    );
    assert_eq!(
        grounding[0].role,
        ExcerptRole::Primary,
        "primary 恒在首位（锚定纪律）"
    );
    let supplements: Vec<_> = grounding
        .iter()
        .filter(|excerpt| excerpt.role == ExcerptRole::Supplement)
        .collect();
    let supplement_paths: Vec<&str> = supplements
        .iter()
        .map(|excerpt| excerpt.path.as_str())
        .collect();
    assert_eq!(
        supplement_paths,
        vec!["notes/a-期权简记.md", "notes/z-期权基础.md"],
        "命中散落多文件都作补充，且按结构序（路径字典序）交付"
    );
    for supplement in &supplements {
        // 可溯源：库 + 文件（+ 命中章节）。
        assert_eq!(supplement.kb_id.as_deref(), Some(kb_id.as_str()));
        let provenance = supplement.provenance_label();
        assert!(provenance.contains("金融笔记"), "{provenance}");
        assert!(provenance.contains(&supplement.path), "{provenance}");
        assert!(
            provenance.contains("章节"),
            "命中章节随片段给出：{provenance}"
        );
        assert!(
            supplement.text.contains("补充事实标记"),
            "补充片段带全文：{}",
            supplement.path
        );
    }
    // 主 source 自己命中时不做补充（不重复占用预算）。
    assert!(
        !supplement_paths.contains(&"docs/期权入门.md"),
        "primary 不重复出现为补充片段"
    );
}

/// t3 验收钉子④（快照兜底 + 披露降级）：源库不可达（库根被删）→ 课时仍可
/// 生成，primary 回到快照摘录，读数帧如实披露降级。
#[tokio::test]
async fn lesson_grounding_falls_back_to_the_snapshot_when_the_base_is_gone() {
    let (service, knowledge, _data_dir, user_id) = grounding_test_service().await;
    let base = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let kb_id = base.base.knowledge_base_id.as_str().to_owned();
    write_kb_file(
        &base.base.root_path,
        "docs/期权入门.md",
        "# 期权入门\n\n库内事实标记：这条消息随库一起消失。\n",
    );
    let detail = import_kb_lesson_course(
        &service,
        &kb_id,
        "docs/期权入门.md",
        vec![(
            "docs/期权入门.md".to_owned(),
            "快照摘录标记：建课时冻结的摘录。".to_owned(),
        )],
        "",
    )
    .await;
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    // 源库不可达：整个库根目录消失（本地盘被拔、目录被删一类）。
    std::fs::remove_dir_all(&base.base.root_path).unwrap();

    let events = Arc::new(CapturingBroadcaster::default());
    service.set_event_sink(LearningEventEmitter::new(
        events.clone(),
        Arc::from(user_id.as_str()),
    ));
    let engine = Arc::new(RecordingLessonEngine {
        contexts: Mutex::new(Vec::new()),
    });
    service.set_lesson_engine(engine.clone());
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();

    let contexts = engine.contexts.lock().unwrap();
    let grounding = contexts[0].grounding();
    assert_eq!(grounding.len(), 1, "库不可达：只有快照兜底片段");
    assert_eq!(grounding[0].role, ExcerptRole::Primary);
    assert!(
        grounding[0].text.contains("快照摘录标记"),
        "快照摘录成为 primary（现状能力保留）"
    );
    // 读数帧（票面「读数」项）：命中数/扩读数 + 降级披露。
    let events = events.events.lock().unwrap();
    let frame = events
        .iter()
        .find(|(name, payload)| {
            name == "learning.lesson-generation" && payload["phase"] == "grounding"
        })
        .expect("grounding 读数帧必须发出");
    assert_eq!(frame.1["primary_live"], serde_json::json!(false));
    assert_eq!(frame.1["degraded"], serde_json::json!(true));
    assert_eq!(frame.1["supplements"], serde_json::json!(0));
}

/// t3 验收钉子⑤（老课程零回归）：无目标、无采样披露、旧快照形状的老课程
/// 照常生成——查询词来自课时标题/purpose（目标词条集缺省为空），primary 取
/// 实时全文，相关旁支文件仍作补充；旧 samples_json 形状原样可读。
#[tokio::test]
async fn legacy_course_without_goal_still_generates_from_title_purpose_retrieval() {
    let (service, knowledge, _data_dir, user_id) = grounding_test_service().await;
    let base = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let kb_id = base.base.knowledge_base_id.as_str().to_owned();
    write_kb_file(
        &base.base.root_path,
        "docs/期权入门.md",
        "# 期权入门\n\n源文件全文标记：期权是标准化合约。\n",
    );
    write_kb_file(
        &base.base.root_path,
        "notes/期权杂记.md",
        "# 期权杂记\n\n旁支事实标记：期权可以组合成价差。\n",
    );
    let detail = import_kb_lesson_course(
        &service,
        &kb_id,
        "docs/期权入门.md",
        vec![(
            "docs/期权入门.md".to_owned(),
            "旧快照标记：建课时冻结的摘录。".to_owned(),
        )],
        "",
    )
    .await;
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    let engine = Arc::new(RecordingLessonEngine {
        contexts: Mutex::new(Vec::new()),
    });
    service.set_lesson_engine(engine.clone());
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();

    let contexts = engine.contexts.lock().unwrap();
    let context = &contexts[0];
    assert!(context.goal.is_none(), "旧课程没有目标");
    let grounding = context.grounding();
    assert_eq!(grounding[0].role, ExcerptRole::Primary);
    assert!(
        grounding[0].text.contains("源文件全文标记"),
        "实时全文仍是 primary"
    );
    assert!(
        grounding
            .iter()
            .any(|excerpt| excerpt.path == "notes/期权杂记.md"),
        "无目标也按标题/purpose 检索扩读"
    );
}

// ── t4 多知识库（数组落库 + 预算分摊 + 归属引用 + 读侧过滤） ────────────────

/// 引用采样路径的大纲引擎替身（t4）：返回一份 source 指向指定路径的最小
/// 蓝图——模拟模型照采样清单（命名空间路径）引用来源。
struct CitingOutlineEngine {
    briefs: Mutex<Vec<crate::course_outline::OutlineBrief>>,
    cite: Option<String>,
}

#[async_trait::async_trait]
impl crate::course_outline::CourseOutlineAgentEngine for CitingOutlineEngine {
    async fn generate(
        &self,
        brief: &crate::course_outline::OutlineBrief,
        _model_override: Option<(&str, &str)>,
    ) -> Result<crate::generation::Blueprint, nomifun_common::AppError> {
        self.briefs.lock().unwrap().push(brief.clone());
        Ok(crate::generation::Blueprint {
            title: "模型自定名".into(),
            description: "d".into(),
            domain: "general".into(),
            version: 1,
            modules: vec![crate::generation::BlueprintModule {
                title: "M".into(),
                description: String::new(),
                lessons: vec![crate::generation::BlueprintLesson {
                    title: "L".into(),
                    purpose: "P".into(),
                    source: self.cite.clone().map(|path| SourceSpan {
                        path,
                        start: None,
                        end: None,
                        kb_id: None,
                    }),
                    sources: Vec::new(),
                }],
            }],
        })
    }
}

/// t4 验收钉子①（两库同名文件 → 引用各自解析到正确库）：两个库都有
/// `docs/intro.md`（内容不同），多库建课时路径带 `{库名}/rel` 命名空间，
/// 蓝图 source 的库归属按命名空间标注；课时视图还原成 `(kb_id, 库内相对
/// 路径)` —— 同名文件不再互相串库。
#[tokio::test]
async fn multi_kb_generation_namespaces_paths_and_resolves_source_owner() {
    let (service, knowledge, owner_id) = job_test_service().await;
    let finance = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let course_notes = knowledge
        .quick_create_base(Some("课程笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let finance_id = finance.base.knowledge_base_id.clone();
    let course_id_kb = course_notes.base.knowledge_base_id.clone();
    // 同名文件（两库都含 docs/intro.md）：内容不同，用来验证归属解析。
    write_kb_file(&finance.base.root_path, "docs/intro.md", "# 金融\n认购与认沽。");
    write_kb_file(&course_notes.base.root_path, "docs/intro.md", "# 课程\n第 1 讲。");
    write_kb_file(&course_notes.base.root_path, "docs/lesson-02.md", "# 课程\n第 2 讲。");

    let engine = Arc::new(CitingOutlineEngine {
        briefs: Mutex::new(Vec::new()),
        // 引用非主来源（课程笔记）的文件：归属必须解析到它。
        cite: Some("课程笔记/docs/intro.md".to_owned()),
    });
    service.set_course_outline_engine(engine.clone());

    let request = GenerateCourseRequest {
        course_kind: crate::models::CourseKind::Traditional,
        teaching_style: None,
        // 用户选择顺序：金融在前 = 主来源。
        knowledge_base_id: None,
        knowledge_base_ids: vec![finance_id.parse().unwrap(), course_id_kb.parse().unwrap()],
        description: None,
        domain: None,
        provider_id: None,
        model: None,
        mode: crate::models::CourseGenerationMode::OnDemand,
        goal: Some("学会期权".into()),
    proficiency_statement: None,
    learner_basis: None,
    };
    let detail = service.generate_course(&owner_id, request).await.unwrap();

    // 简报：全量来源库按选择顺序 + 采样路径带命名空间。
    let brief = engine.briefs.lock().unwrap().first().cloned().unwrap();
    let names: Vec<&str> = brief
        .source_knowledge_bases()
        .iter()
        .map(|base| base.name.as_str())
        .collect();
    assert_eq!(names, vec!["金融笔记", "课程笔记"], "选择顺序 = 主来源在前");
    let paths: Vec<&str> = brief.samples.iter().map(|(path, _)| path.as_str()).collect();
    assert!(
        paths.iter().all(|path| path.starts_with("金融笔记/") || path.starts_with("课程笔记/")),
        "多库采样路径带来源命名空间: {paths:?}"
    );
    assert!(paths.contains(&"课程笔记/docs/intro.md"), "{paths:?}");

    // 落库：列表 = 选择顺序；主来源 = 首个；披露逐库。
    assert_eq!(
        detail.course.source_kb_ids.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
        vec![finance_id.as_str(), course_id_kb.as_str()]
    );
    assert_eq!(detail.course.source_kb_id.as_ref().map(KnowledgeBaseId::as_str), Some(finance_id.as_str()));
    let sampling = detail.course.sampling.clone().expect("多库建课必有披露");
    assert_eq!(sampling.bases.len(), 2);

    // 蓝图 source 标注：命名空间 → 课程笔记（非主来源）。
    let lesson = &detail.modules[0].lessons[0];
    let source = lesson.source.clone().expect("引用了采样路径");
    assert_eq!(source.kb_id.as_ref().map(KnowledgeBaseId::as_str), Some(course_id_kb.as_str()));
    assert_eq!(source.path, "docs/intro.md", "视图还原成库内相对路径");
}

/// t4 验收钉子②（删其中一个来源库 → 课程仍可用、不外泄悬空 id）：库被删后
/// 课程照常列出，来源列表剔除悬空 id、主来源顺延；被删库的课时 source 归属
/// 解析成「无归属」（不落到幸存库），其余库的来源照常。
#[tokio::test]
async fn multi_kb_course_survives_a_deleted_source_base() {
    let (service, knowledge, owner_id) = job_test_service().await;
    let finance = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let course_notes = knowledge
        .quick_create_base(Some("课程笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let finance_id = finance.base.knowledge_base_id.clone();
    let course_id_kb = course_notes.base.knowledge_base_id.clone();
    write_kb_file(&finance.base.root_path, "docs/intro.md", "# 金融\n认购与认沽。");
    write_kb_file(&course_notes.base.root_path, "docs/intro.md", "# 课程\n第 1 讲。");

    let engine = Arc::new(CitingOutlineEngine {
        briefs: Mutex::new(Vec::new()),
        cite: Some("金融笔记/docs/intro.md".to_owned()),
    });
    service.set_course_outline_engine(engine);
    let detail = service
        .generate_course(
            &owner_id,
            GenerateCourseRequest {
                course_kind: crate::models::CourseKind::Traditional,
                teaching_style: None,
                knowledge_base_id: None,
                knowledge_base_ids: vec![
                    finance_id.parse().unwrap(),
                    course_id_kb.parse().unwrap(),
                ],
                description: None,
                domain: None,
                provider_id: None,
                model: None,
                mode: crate::models::CourseGenerationMode::OnDemand,
                goal: Some("学会期权".into()),
            proficiency_statement: None,
            learner_basis: None,
            },
        )
        .await
        .unwrap();
    let course_id = detail.course.id.clone();

    // 删主来源库：课程仍可用，列表剔除悬空 id、主来源顺延到课程笔记。
    knowledge.delete_base(&finance_id, true).await.unwrap();

    let listed = service.list_courses(&owner_id).await.unwrap();
    let listed = listed.iter().find(|course| course.id == course_id).unwrap();
    assert_eq!(
        listed.source_kb_ids.iter().map(KnowledgeBaseId::as_str).collect::<Vec<_>>(),
        vec![course_id_kb.as_str()],
        "读侧过滤已删库 id：不外泄悬空 id"
    );
    assert_eq!(
        listed.source_kb_id.as_ref().map(KnowledgeBaseId::as_str),
        Some(course_id_kb.as_str()),
        "主来源顺延到幸存库"
    );

    let detail = service.course_detail(&course_id, Some(&owner_id)).await.unwrap();
    let source = detail.modules[0].lessons[0].source.clone().unwrap();
    assert!(
        source.kb_id.is_none(),
        "被删库的文件解析成无归属，绝不落到幸存库: {source:?}"
    );
    // 其余库的归属照常解析。
    let course_notes_ref = detail
        .modules
        .iter()
        .flat_map(|module| module.lessons.iter())
        .find(|lesson| lesson.source.as_ref().is_some_and(|s| s.kb_id.is_some()));
    assert!(course_notes_ref.is_none(), "本用例只引用被删库的文件");
}

/// t4 验收钉子⑥（缺库口径对齐；评审修复）：多库请求里含一个不存在的库时
/// 按库跳过——其余库照常建课（缺失库在披露里计数 0），不再一个坏库拖垮整门
/// 课；单库请求对同样的缺失库保持原有失败（零回归）。
#[tokio::test]
async fn multi_kb_generation_skips_a_missing_base_instead_of_failing() {
    let (service, knowledge, owner_id) = job_test_service().await;
    let finance = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let finance_id = finance.base.knowledge_base_id.clone();
    write_kb_file(&finance.base.root_path, "docs/intro.md", "# 金融\n认购与认沽。");
    // 幽灵库：合法 v7 id，但没有任何知识库行（被删/不存在的库）。
    let ghost = nomifun_common::generate_id();
    let engine = Arc::new(CitingOutlineEngine {
        briefs: Mutex::new(Vec::new()),
        cite: Some("金融笔记/docs/intro.md".to_owned()),
    });
    service.set_course_outline_engine(engine.clone());

    let request = |ids: Vec<KnowledgeBaseId>| GenerateCourseRequest {
        course_kind: crate::models::CourseKind::Traditional,
        teaching_style: None,
        knowledge_base_id: None,
        knowledge_base_ids: ids,
        description: None,
        domain: None,
        provider_id: None,
        model: None,
        mode: crate::models::CourseGenerationMode::OnDemand,
        goal: Some("学会期权".into()),
    proficiency_statement: None,
    learner_basis: None,
    };

    let detail = service
        .generate_course(
            &owner_id,
            request(vec![ghost.parse().unwrap(), finance_id.parse().unwrap()]),
        )
        .await
        .unwrap();
    let brief = engine.briefs.lock().unwrap().first().cloned().unwrap();
    let names: Vec<&str> = brief
        .source_knowledge_bases()
        .iter()
        .map(|base| base.name.as_str())
        .collect();
    assert_eq!(names, vec!["金融笔记"], "缺失的库被跳过，不冒充来源库");
    // 披露如实：缺失库计数 0（选择顺序原样保留在课程行里）。
    let sampling = detail.course.sampling.clone().expect("多库建课必有披露");
    assert_eq!(sampling.bases.len(), 2, "逐库披露包含被跳过的库");
    let ghost_id: KnowledgeBaseId = ghost.parse().unwrap();
    let skipped = sampling
        .bases
        .iter()
        .find(|base| base.kb_id == ghost_id)
        .expect("被跳过的库有一条披露条目");
    assert_eq!(skipped.total_files, 0);
    assert_eq!(skipped.sampled_files, 0);
    assert_eq!(
        sampling
            .bases
            .iter()
            .map(|base| base.kb_id.as_str())
            .collect::<Vec<_>>(),
        vec![ghost.as_str(), finance_id.as_str()],
        "披露逐库如实（含被跳过的库，按选择顺序）"
    );
    assert_eq!(
        detail
            .course
            .source_kb_ids
            .iter()
            .map(KnowledgeBaseId::as_str)
            .collect::<Vec<_>>(),
        vec![finance_id.as_str()],
        "课程行只写存活库（导入侧不写悬空 id）；缺席由披露承担"
    );
    assert_eq!(
        detail.course.source_kb_id.as_ref().map(KnowledgeBaseId::as_str),
        Some(finance_id.as_str()),
        "主来源 = 首个存活库"
    );

    // 单库请求：同一个缺失库仍然直接失败（原口径零回归）。
    let error = service
        .generate_course(&owner_id, request(vec![ghost.parse().unwrap()]))
        .await
        .expect_err("single missing knowledge base must still fail");
    assert!(
        matches!(error, nomifun_common::AppError::NotFound(_)),
        "单库缺库保持 NotFound: {error:?}"
    );
}

/// t4 验收钉子③（乱序提交同一组库 → 去重槽位拒绝）：来源键按库 id 排序拼接，
/// 提交顺序不影响槽位——不会重复建课。
#[tokio::test]
async fn multi_kb_shuffled_resubmission_hits_the_same_generation_slot() {
    let (service, knowledge, owner_id) = job_test_service().await;
    let first = knowledge
        .quick_create_base(Some("甲库"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let second = knowledge
        .quick_create_base(Some("乙库"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let a: KnowledgeBaseId = first.base.knowledge_base_id.parse().unwrap();
    let b: KnowledgeBaseId = second.base.knowledge_base_id.parse().unwrap();
    let request = |ids: Vec<KnowledgeBaseId>| GenerateCourseRequest {
        course_kind: crate::models::CourseKind::Traditional,
        teaching_style: None,
        knowledge_base_id: None,
        knowledge_base_ids: ids,
        description: None,
        domain: None,
        provider_id: None,
        model: None,
        mode: crate::models::CourseGenerationMode::OnDemand,
        goal: Some("学会聚类".into()),
    proficiency_statement: None,
    learner_basis: None,
    };

    let _slot = service
        .acquire_generation_slot(
            &owner_id,
            request(vec![a.clone(), b.clone()]).knowledge_base_key().unwrap(),
        )
        .unwrap();
    let rejected = service
        .acquire_generation_slot(&owner_id, request(vec![b, a]).knowledge_base_key().unwrap());
    let Err(rejected) = rejected else {
        panic!("乱序提交同一组库必须命中同一槽位（重复建课）");
    };
    assert!(
        matches!(rejected, AppError::Conflict(_)),
        "乱序提交同一组库必须命中同一槽位: {rejected:?}"
    );
}

/// t4 验收钉子④（同名文件各自解析到正确库的原文回看）：课时实时读库按
/// `(kb_id, 库内相对路径)` 去前缀——两库同名 `docs/intro.md` 时读到的是引用
/// 的那一份，不是主来源那份。
#[tokio::test]
async fn lesson_grounding_reads_the_cited_base_when_two_bases_share_a_path() {
    let (service, knowledge, _data_dir, user_id) = grounding_test_service().await;
    let primary = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let other = knowledge
        .quick_create_base(Some("课程笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let primary_id = primary.base.knowledge_base_id.clone();
    let other_id = other.base.knowledge_base_id.clone();
    write_kb_file(
        &primary.base.root_path,
        "docs/intro.md",
        "# 金融\n主库标记：认购与认沽的权利义务不对称。\n",
    );
    write_kb_file(
        &other.base.root_path,
        "docs/intro.md",
        "# 课程\n副库标记：期权是标准化合约。\n",
    );

    let cited = "课程笔记/docs/intro.md";
    let source = Some(SourceSpan {
        path: cited.to_owned(),
        start: None,
        end: None,
        kb_id: None,
    });
    let pack = CoursePack {
        title: "期权入门".into(),
        description: "d".into(),
        domain: "finance".into(),
        source_kb_id: Some(primary_id.parse().unwrap()),
        version: 1,
        modules: vec![ModulePack {
            title: "模块一".into(),
            description: String::new(),
            lessons: vec![LessonPack {
                title: "期权入门".into(),
                summary: String::new(),
                purpose: "理解期权".into(),
                est: 10,
                source: source.clone(),
                sources: Vec::new(),
                activities: Vec::new(),
                sections: Vec::new(),
            }],
        }],
        teaching_style: crate::models::TeachingStyle::Standard,
    };
    let blueprint = crate::generation::Blueprint {
        title: "期权入门".into(),
        description: "d".into(),
        domain: "finance".into(),
        version: 1,
        modules: vec![crate::generation::BlueprintModule {
            title: "模块一".into(),
            description: String::new(),
            lessons: vec![crate::generation::BlueprintLesson {
                title: "期权入门".into(),
                purpose: "理解期权".into(),
                source,
                sources: Vec::new(),
            }],
        }],
    };
    let disclosure = serde_json::json!({
        "goal": "学会期权",
        "total_files": 2,
        "sampled_files": 2,
        "coherence_files": 0,
        "goal_fallback": false,
        "goal_terms": ["期权"],
        "tree": ["金融笔记/docs/intro.md", cited],
        "low_score_files": [],
        "bases": [
            {"kb_id": primary_id, "name": "金融笔记", "namespace": "金融笔记",
             "total_files": 1, "sampled_files": 1, "coherence_files": 0,
             "low_score_files": 0, "goal_fallback": false},
            {"kb_id": other_id, "name": "课程笔记", "namespace": "课程笔记",
             "total_files": 1, "sampled_files": 1, "coherence_files": 0,
             "low_score_files": 0, "goal_fallback": false}
        ]
    });
    let detail = service
        .import_course_outline(
            pack,
            OutlineSnapshot {
                blueprint_json: serde_json::to_string(&blueprint).unwrap(),
                samples_json: serde_json::to_string(&vec![
                    ("金融笔记/docs/intro.md".to_owned(), "主来源快照".to_owned()),
                    (cited.to_owned(), "非主来源快照".to_owned()),
                ])
                .unwrap(),
                learning_goal: "学会期权".into(),
                samples_meta_json: Some(disclosure.to_string()),
                source_kb_ids: vec![primary_id.parse().unwrap(), other_id.parse().unwrap()],
            },
        )
        .await
        .unwrap();

    let view_source = detail.modules[0].lessons[0].source.clone().unwrap();
    assert_eq!(view_source.path, "docs/intro.md");
    assert_eq!(
        view_source.kb_id.as_ref().map(KnowledgeBaseId::as_str),
        Some(other_id.as_str()),
        "引用解析到课程笔记，不是主来源"
    );

    let lesson_id = detail.modules[0].lessons[0].id.clone();
    let engine = Arc::new(RecordingLessonEngine {
        contexts: Mutex::new(Vec::new()),
    });
    service.set_lesson_engine(engine.clone());
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();
    let contexts = engine.contexts.lock().unwrap();
    let grounding = contexts[0].grounding();
    let primary_excerpt = &grounding[0];
    assert_eq!(primary_excerpt.role, ExcerptRole::Primary);
    assert_eq!(primary_excerpt.path, "docs/intro.md", "库内相对路径");
    assert_eq!(
        primary_excerpt.kb_id.as_deref(),
        Some(other_id.as_str()),
        "实时读库按引用库读（非主来源）"
    );
    assert!(
        primary_excerpt.text.contains("副库标记"),
        "读到的是引用库的那一份同名文件: {}",
        primary_excerpt.text
    );
    assert!(
        !primary_excerpt.text.contains("主库标记"),
        "读到的是引用库的那一份，不是主来源的: {}",
        primary_excerpt.text
    );
}

/// t4 验收钉子⑤（已删库的引用绝不读成幸存库的文档；评审修复）：两库课程的
/// 引用库被删后，该课时的 primary 退回快照摘录、片段不携带任何库归属——
/// 修复前 `.or_else(primary)` 会把「课程笔记/…」这条路径归到幸存的主来源上
/// 去读；夹具让这次错读**读得到**（幸存库里恰好同路径有一份别的文档），
/// 错读的后果因此不是"碰巧读不到"而是把课时 ground 到别的库的文档上。
#[tokio::test]
async fn lesson_grounding_never_reads_a_deleted_base_path_from_the_surviving_base() {
    let (service, knowledge, _data_dir, user_id) = grounding_test_service().await;
    let primary = knowledge
        .quick_create_base(Some("金融笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let other = knowledge
        .quick_create_base(Some("课程笔记"), None, "blank", None, None, None, None)
        .await
        .unwrap();
    let primary_id = primary.base.knowledge_base_id.clone();
    let other_id = other.base.knowledge_base_id.clone();
    // 幸存库里的同名路径（阅读顺序错读的教科书场景）：删库前它不属于本课程。
    write_kb_file(
        &primary.base.root_path,
        "课程笔记/docs/intro.md",
        "# 别的文档\n错读标记：这份文档属于主来源库，不是被引用库。\n",
    );

    let cited = "课程笔记/docs/intro.md";
    let source = Some(SourceSpan {
        path: cited.to_owned(),
        start: None,
        end: None,
        kb_id: None,
    });
    let pack = CoursePack {
        title: "期权入门".into(),
        description: "d".into(),
        domain: "finance".into(),
        source_kb_id: Some(primary_id.parse().unwrap()),
        version: 1,
        modules: vec![ModulePack {
            title: "模块一".into(),
            description: String::new(),
            lessons: vec![LessonPack {
                title: "期权入门".into(),
                summary: String::new(),
                purpose: "理解期权".into(),
                est: 10,
                source: source.clone(),
                sources: Vec::new(),
                activities: Vec::new(),
                sections: Vec::new(),
            }],
        }],
        teaching_style: crate::models::TeachingStyle::Standard,
    };
    let blueprint = crate::generation::Blueprint {
        title: "期权入门".into(),
        description: "d".into(),
        domain: "finance".into(),
        version: 1,
        modules: vec![crate::generation::BlueprintModule {
            title: "模块一".into(),
            description: String::new(),
            lessons: vec![crate::generation::BlueprintLesson {
                title: "期权入门".into(),
                purpose: "理解期权".into(),
                source,
                sources: Vec::new(),
            }],
        }],
    };
    let disclosure = serde_json::json!({
        "goal": "学会期权",
        "total_files": 1,
        "sampled_files": 1,
        "coherence_files": 0,
        "goal_fallback": false,
        "goal_terms": ["期权"],
        "tree": [cited],
        "low_score_files": [],
        "bases": [
            {"kb_id": primary_id, "name": "金融笔记", "namespace": "金融笔记",
             "total_files": 0, "sampled_files": 0, "coherence_files": 0,
             "low_score_files": 0, "goal_fallback": false},
            {"kb_id": other_id, "name": "课程笔记", "namespace": "课程笔记",
             "total_files": 1, "sampled_files": 1, "coherence_files": 0,
             "low_score_files": 0, "goal_fallback": false}
        ]
    });
    let detail = service
        .import_course_outline(
            pack,
            OutlineSnapshot {
                blueprint_json: serde_json::to_string(&blueprint).unwrap(),
                samples_json: serde_json::to_string(&vec![(
                    cited.to_owned(),
                    "快照摘录标记：建课时冻结的摘录。".to_owned(),
                )])
                .unwrap(),
                learning_goal: "学会期权".into(),
                samples_meta_json: Some(disclosure.to_string()),
                source_kb_ids: vec![primary_id.parse().unwrap(), other_id.parse().unwrap()],
            },
        )
        .await
        .unwrap();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    // 引用库被删：只剩主来源库存活。
    knowledge.delete_base(&other_id, true).await.unwrap();

    let engine = Arc::new(RecordingLessonEngine {
        contexts: Mutex::new(Vec::new()),
    });
    service.set_lesson_engine(engine.clone());
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();

    let contexts = engine.contexts.lock().unwrap();
    let grounding = contexts[0].grounding();
    let primary_excerpt = &grounding[0];
    assert_eq!(primary_excerpt.role, ExcerptRole::Primary);
    assert!(
        primary_excerpt.text.contains("快照摘录标记"),
        "已删库的引用走快照兜底: {}",
        primary_excerpt.text
    );
    assert!(
        !primary_excerpt.text.contains("错读标记"),
        "绝不把被删库的路径读成幸存库的文档: {}",
        primary_excerpt.text
    );
    assert!(
        primary_excerpt.kb_id.is_none() && primary_excerpt.kb_name.is_none(),
        "快照兜底的片段不携带库归属（更不归属到幸存库）: {:?}",
        primary_excerpt
    );
}

// ── 别名闭环全量（j04 = 二轮 g3 + 三轮 g2）────────────────────────────────

/// 复用即落池：评审把 mints 提名裁为复用（concept_id 指向种子概念）→ 不铸
/// 新行，提名名落为该概念的活跃别名记录；引用该名的 teaches 解析到宿主。
#[tokio::test]
async fn reuse_ruling_pools_the_name_instead_of_minting() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let host: String = sqlx::query_scalar(
        "SELECT concept_id FROM learning_concept_registry WHERE canonical = '向量基础'",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    let batch_json = format!(
        r#"{{"nodes": [{{"title": "用向量视角重看排布", "purpose": "从座位调换切入", "est": 15, "teaches": [{{"name": "向量代数", "tier": "know"}}], "assumes": []}}], "mints": [{{"canonical": "向量代数", "aliases": [], "definition": "用向量语言改写既有对象关系"}}], "new_aliases": [], "note": "复用稿"}}"#
    );
    let review_json =
        format!(r#"{{"verdict":"pass","reuse":[{{"name":"向量代数","concept_id":"{host}"}}]}}"#);
    let completer = SequencedCompleter::new(vec![batch_json, review_json]);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    wait_for_growth_batch(&service, &course_id).await;

    // 不铸新行：登记表仍只有种子 1 条概念。
    let registry: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(registry, 1, "reuse must not mint a new row");
    // 提名名落池：宿主概念多了活跃别名「向量代数」，带提名定义。
    let aliases_json: String = sqlx::query_scalar(
        "SELECT aliases_json FROM learning_concept_registry WHERE concept_id = ?",
    )
    .bind(&host)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    let records = crate::learning_graph::alias_records::parse_alias_records(&aliases_json);
    assert!(records.iter().any(|record| record.name == "向量代数"
        && record.status == "active"
        && record.definition.as_deref() == Some("用向量语言改写既有对象关系")));
    // 引用落位：节点 teaches 向量代数 → 宿主概念的网边（ref_name 记投影名）。
    let refs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lesson_concepts lc \
         JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
         JOIN learning_modules m ON m.module_id = l.module_id \
         WHERE m.course_id = ? AND lc.concept_id = ? AND lc.role = 'teaches' AND lc.ref_name = '向量代数'",
    )
    .bind(&course_id)
    .bind(&host)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(
        refs, 1,
        "the reference hangs off the host concept via the pooled name"
    );
}

/// 裁决宿主只需在登记表中存在：评审从候选面之外（如相关提名的候选）挑中
/// 的登记表概念不再被误杀——候选集归属降级为日志注记，落库仍走宿主活跃
/// 别名（真机 run-1792395614887 的「霍纳法案」型误杀）。
#[tokio::test]
async fn reuse_ruling_accepts_registry_host_outside_the_candidate_set() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let now = now_ms();
    // 宿主「线性组合」与提名名「向量代数」无词面共享；登记表另有三条共享
    // 「向量」二元组的条目占满词面 top-k 补位——宿主必在候选集之外。
    let host = nomifun_common::LearningConceptId::new().into_string();
    for (concept_id, canonical) in [
        (nomifun_common::LearningConceptId::new().into_string(), "平面向量"),
        (nomifun_common::LearningConceptId::new().into_string(), "空间向量"),
        (nomifun_common::LearningConceptId::new().into_string(), "矩阵"),
        (host.clone(), "线性组合"),
    ] {
        sqlx::query(
            "INSERT INTO learning_concept_registry \
             (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
             VALUES (?, ?, '[]', '', ?, ?)",
        )
        .bind(&concept_id)
        .bind(canonical)
        .bind(now)
        .bind(now)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }
    let batch_json = r#"{
        "nodes": [
            {"title": "用向量语言重看组合", "purpose": "从座位调换切入", "est": 15,
             "teaches": [{"name": "向量代数", "tier": "know"}], "assumes": []}
        ],
        "mints": [
            {"canonical": "向量代数", "aliases": [], "definition": "用向量语言改写既有对象关系"}
        ],
        "new_aliases": [],
        "note": "候选集外复用稿"
    }"#;
    let review_json =
        format!(r#"{{"verdict":"pass","reuse":[{{"name":"向量代数","concept_id":"{host}"}}]}}"#);
    let completer = SequencedCompleter::new(vec![batch_json.to_owned(), review_json]);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    wait_for_growth_batch(&service, &course_id).await;

    // 不铸新行：提名名落为登记表宿主的活跃别名，引用挂到宿主概念。
    let minted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_concept_registry WHERE canonical = '向量代数'",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(
        minted, 0,
        "an out-of-candidates registry host must still be reused, not minted"
    );
    let aliases_json: String = sqlx::query_scalar(
        "SELECT aliases_json FROM learning_concept_registry WHERE concept_id = ?",
    )
    .bind(&host)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    let records = crate::learning_graph::alias_records::parse_alias_records(&aliases_json);
    assert!(
        records.iter().any(|record| record.name == "向量代数"
            && record.status == "active"
            && record.definition.as_deref() == Some("用向量语言改写既有对象关系")),
        "the ruling host pools the nominated name as an active alias: {aliases_json}"
    );
    let refs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lesson_concepts lc \
         JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
         JOIN learning_modules m ON m.module_id = l.module_id \
         WHERE m.course_id = ? AND lc.concept_id = ? AND lc.role = 'teaches' \
           AND lc.ref_name = '向量代数'",
    )
    .bind(&course_id)
    .bind(&host)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(refs, 1, "the node teaches edge hangs off the ruled host");
}

/// 评审建议机械转化（t4）：reject 里 advice 指名「裁为复用」且宿主可登记表
/// 解析的问题 → 代码合成 reuse 裁决执行（真机 run-1792403482606 call 3 形
/// 态），问题剔除不折返模型；落库不铸新行、提名落宿主活跃别名、note 留痕。
#[tokio::test]
async fn review_advice_mechanically_converts_to_reuse() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let host: String = sqlx::query_scalar(
        "SELECT concept_id FROM learning_concept_registry WHERE canonical = '向量基础'",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    let batch_json = r#"{
        "nodes": [
            {"title": "用向量视角重看排布", "purpose": "从座位调换切入", "est": 15,
             "teaches": [{"name": "向量代数", "tier": "know"}], "assumes": []}
        ],
        "mints": [
            {"canonical": "向量代数", "aliases": [], "definition": "用向量语言改写既有对象关系"}
        ],
        "new_aliases": [],
        "note": "建议转化稿"
    }"#;
    let review_json = format!(
        r#"{{"verdict":"reject","problems":[{{"node":"用向量视角重看排布","field":"mints","issue":"铸名「向量代数」与登记表既有概念「向量基础」内涵重叠——应复用既有名，不新铸。","advice":"把「向量代数」裁为复用「向量基础」的别名，不另铸新行。"}}]}}"#
    );
    let completer = SequencedCompleter::new(vec![batch_json.to_owned(), review_json]);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    wait_for_growth_batch(&service, &course_id).await;

    let registry: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(registry, 1, "advice conversion must not mint a new row");
    let aliases_json: String = sqlx::query_scalar(
        "SELECT aliases_json FROM learning_concept_registry WHERE concept_id = ?",
    )
    .bind(&host)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert!(
        aliases_json.contains("向量代数") && aliases_json.contains("用向量语言改写既有对象关系"),
        "the advised host pools the nominated name: {aliases_json}"
    );
    let note: String = sqlx::query_scalar(
        "SELECT note FROM learning_growth_batches WHERE course_id = ? AND status = 'applied'",
    )
    .bind(&course_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert!(
        note.contains("评审建议机械转化「向量代数」→复用「向量基础」"),
        "conversion must be recorded in the batch note: {note}"
    );
}

/// 解析失败（宿主不可登记表解析）的 advice 照常打回：同指纹两轮早停，
/// 不做机械转化。
#[tokio::test]
async fn unparseable_advice_stays_a_rejection() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let batch_json = r#"{
        "nodes": [
            {"title": "用向量视角重看排布", "purpose": "从座位调换切入", "est": 15,
             "teaches": [{"name": "向量代数", "tier": "know"}], "assumes": []}
        ],
        "mints": [
            {"canonical": "向量代数", "aliases": [], "definition": "用向量语言改写既有对象关系"}
        ],
        "new_aliases": [],
        "note": "不可解析稿"
    }"#;
    let review_json = r#"{"verdict":"reject","problems":[{"node":"用向量视角重看排布","field":"mints","issue":"铸名「向量代数」与某个不在登记表里的概念「向量算术语言」内涵重叠——应复用。","advice":"把「向量代数」裁为复用「向量算术语言」的别名。"}]}"#;
    let completer = SequencedCompleter::new(vec![
        batch_json.to_owned(),
        review_json.to_owned(),
        batch_json.to_owned(),
        review_json.to_owned(),
    ]);
    *service.course_completer.write().unwrap() = Some(completer);
    let kicked = service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    assert!(kicked, "manual growth kicks");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let stats = service.growth_run_stats().expect("stats recorded");
    assert_eq!(
        stats.stop_reason, "early_stop",
        "unparseable advice must stay a rejection: {stats:?}"
    );
    let applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_growth_batches WHERE course_id = ? AND status = 'applied'",
    )
    .bind(&course_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(applied, 0, "the batch must not land");
}

/// 铸名撞登记表现有名 → 门面机械复用（run-1792402566347 实录的红条死
/// 路）：ng.mints.collision 被吸收为 auto-ruling，不铸新行、引用落宿主。
#[tokio::test]
async fn mint_colliding_with_registry_name_is_mechanically_reused() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let host: String = sqlx::query_scalar(
        "SELECT concept_id FROM learning_concept_registry WHERE canonical = '向量基础'",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    let batch_json = r#"{
        "nodes": [
            {"title": "重新走一遍向量基础", "purpose": "从零起步切入", "est": 15,
             "teaches": [{"name": "向量基础", "tier": "know"}], "assumes": []}
        ],
        "mints": [
            {"canonical": "向量基础", "aliases": [], "definition": "有方向的量"}
        ],
        "new_aliases": [],
        "note": "撞池稿"
    }"#;
    let review_json = r#"{"verdict":"pass"}"#;
    let completer = SequencedCompleter::new(vec![batch_json.to_owned(), review_json.to_owned()]);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    wait_for_growth_batch(&service, &course_id).await;

    let registry: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(registry, 1, "a colliding canonical must not mint a new row");
    let refs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lesson_concepts lc \
         JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
         JOIN learning_modules m ON m.module_id = l.module_id \
         WHERE m.course_id = ? AND lc.concept_id = ? AND lc.role = 'teaches' \
           AND lc.ref_name = '向量基础'",
    )
    .bind(&course_id)
    .bind(&host)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(refs, 1, "the teaches edge lands on the owning host");
    let note: String = sqlx::query_scalar(
        "SELECT note FROM learning_growth_batches WHERE course_id = ? AND status = 'applied'",
    )
    .bind(&course_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert!(
        note.contains("撞池机械复用「向量基础」"),
        "the absorbed collision must be recorded in the batch note: {note}"
    );
}

/// new_aliases 归一化直定宿主：提名定义与既有概念逐字相同 → 门面机械绑定
///（评审零裁决也放行），别名带提名定义与出处落池。
#[tokio::test]
async fn new_alias_with_exact_definition_binds_directly() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let host: String = sqlx::query_scalar(
        "SELECT concept_id FROM learning_concept_registry WHERE canonical = '向量基础'",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    sqlx::query("UPDATE learning_concept_registry SET definition = ? WHERE concept_id = ?")
        .bind("用有方向的量描述位移与力")
        .bind(&host)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    let batch_json = r#"{
        "nodes": [
            {"title": "换名重述向量", "purpose": "从不同叫法切入", "est": 15,
             "teaches": [{"name": "向量算术", "tier": "know"}], "assumes": []}
        ],
        "mints": [],
        "new_aliases": [
            {"name": "向量算术", "definition": "用有方向的量描述位移与力", "source": "教练提名：低龄学习者的日常叫法"}
        ],
        "note": "直定稿"
    }"#;
    let review_json = r#"{"verdict":"pass"}"#;
    let completer = SequencedCompleter::new(vec![batch_json.to_owned(), review_json.to_owned()]);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    wait_for_growth_batch(&service, &course_id).await;
    let aliases_json: String = sqlx::query_scalar(
        "SELECT aliases_json FROM learning_concept_registry WHERE concept_id = ?",
    )
    .bind(&host)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert!(
        aliases_json.contains("向量算术") && aliases_json.contains("低龄学习者的日常叫法"),
        "direct-host alias lands with definition and attestation: {aliases_json}"
    );
}

/// 裁决新铸打回指点：new_alias 未被裁复用（评审零裁决、无直定宿主）→
/// 整批打回，反馈携带改走 mints 的指点；同错两轮 → 早停（不再烧第 3 轮）。
#[tokio::test]
async fn new_alias_without_reuse_ruling_returns_the_batch_with_pointer() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let batch_json = r#"{
        "nodes": [
            {"title": "无宿主提名", "purpose": "p", "est": 15,
             "teaches": [{"name": "新名", "tier": "know"}], "assumes": []}
        ],
        "mints": [],
        "new_aliases": [
            {"name": "新名", "definition": "登记表里没有的独立定义", "source": "s"}
        ],
        "note": "n"
    }"#;
    let review_json = r#"{"verdict":"pass"}"#;
    let completer = SequencedCompleter::new(vec![batch_json.to_owned(), review_json.to_owned()]);
    *service.course_completer.write().unwrap() = Some(completer.clone());
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    assert!(
        wait_for_batch_status(&service, &course_id, "failed").await,
        "the batch must be returned with the mints pointer"
    );
    assert_eq!(
        completer.call_count(),
        4,
        "same fingerprint stops at two attempts (draft/redraft + review per attempt)"
    );
    let stats = service.growth_run_stats().expect("stats recorded");
    assert_eq!(stats.stop_reason, "early_stop");
}

/// 别名池可见：教练起草装配含【概念别名池】行（正名=别名，带投影定义）。
#[tokio::test]
async fn coach_context_carries_the_alias_pool() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    sqlx::query(
        "UPDATE learning_concept_registry SET aliases_json = ? WHERE canonical = '向量基础'",
    )
    .bind(r#"[{"name":"向量语言","definition":"把对象关系改写成有向量的说法","status":"active"}]"#)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    let completer = ScriptedCompleter::new(VALID_BATCH, false);
    *service.course_completer.write().unwrap() = Some(completer.clone());
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    wait_for_growth_batch(&service, &course_id).await;
    let prompt = completer
        .last_user
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_default();
    assert!(prompt.contains("【概念别名池"), "{prompt}");
    assert!(prompt.contains("向量基础=向量语言"), "{prompt}");
    assert!(prompt.contains("把对象关系改写成有向量的说法"), "{prompt}");
}

/// 名池豁免（退役名不占池）：撞退役概念的 canonical 仍可新铸——退役行
/// 不阻塞，新活跃行接手该名字，引用落在活跃行。
#[tokio::test]
async fn retired_names_do_not_block_minting() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _a, _b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, deprecated, created_at, updated_at) \
         VALUES ('0190f5fe-7c00-7a00-8abc-333333333333', '向量代数', '[]', '旧定义', 1, 1, 1)",
    )
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    let batch_json = r#"{
        "nodes": [
            {"title": "用向量代数重排", "purpose": "p", "est": 15,
             "teaches": [{"name": "向量代数", "tier": "know"}], "assumes": []}
        ],
        "mints": [
            {"canonical": "向量代数", "aliases": [], "definition": "新定义：向量运算的代数规则"}
        ],
        "new_aliases": [],
        "note": "n"
    }"#;
    let review_json = r#"{"verdict":"pass"}"#;
    let completer = SequencedCompleter::new(vec![batch_json.to_owned(), review_json.to_owned()]);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    wait_for_growth_batch(&service, &course_id).await;
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_concept_registry \
         WHERE canonical = '向量代数' AND deprecated = 0",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(active, 1, "the retired row does not block the fresh mint");
    let reference_hangs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lesson_concepts lc \
         JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
         WHERE reg.canonical = '向量代数' AND reg.deprecated = 0",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(reference_hangs, 1, "the reference lands on the active row");
}


// ---------------------------------------------------------------------------
// j05：复习事件加固 + XP 结算后端（三轮 t3 + 三轮 t1）
// ---------------------------------------------------------------------------

/// 教新线结算：完成课时按 est 计价入账，幂等锚吃掉重复完成。
#[tokio::test]
async fn xp_node_settlement_is_priced_by_est_and_idempotent() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    let course = service.import_course(valid_pack()).await.unwrap();
    service.enroll(&course.course.id, &user_id).await.unwrap();
    let detail = service
        .course_detail(&course.course.id, Some(&user_id))
        .await
        .unwrap();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
    // 练习全对（1/1 通过 → full）：est 10 + bonus 2。
    let result = service
        .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
        .await
        .unwrap();
    assert!(result.passed);
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();
    let (count, earned, bonus, basis): (i64, i64, i64, String) = sqlx::query_as(
        "SELECT COUNT(*), earned_xp, xp_bonus, basis FROM learning_xp_events \
         WHERE kind = 'node-completed'",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!((earned, bonus, basis.as_str()), (12, 2, "full"));

    // 重复完成：锚吸收，不重复结算（一次性事实）。
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_xp_events WHERE kind = 'node-completed'",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(count, 1, "duplicate completion must not settle twice");
}

/// 复习线：一次真实推进结算一笔（同卡同学习日一笔）；同日折叠 = 只记作答
/// 流水，不推进、不落日志、不重复结算。
#[tokio::test]
async fn xp_review_settlement_folds_same_day_repeat() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    let course = service.import_course(valid_pack()).await.unwrap();
    service.enroll(&course.course.id, &user_id).await.unwrap();
    let detail = service
        .course_detail(&course.course.id, Some(&user_id))
        .await
        .unwrap();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();
    make_all_due(&service, &user_id).await;
    let due = service
        .due_reviews(&user_id, 10, &[], true, false, &[])
        .await
        .unwrap();
    assert_eq!(due.len(), 1);
    let card = due[0].id.clone();

    // 承认遗忘：推进 + auto-again 落日志，结算一笔 0 分 partial。
    let result = service
        .answer_review(&card, &user_id, Value::Null, true, Some(4_200))
        .await
        .unwrap();
    assert!(!result.correct);
    assert!(result.advanced);
    let (log_rows, xp_rows, earned): (i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT COUNT(*) FROM learning_review_log WHERE rating_source != 'synthetic'), \
           (SELECT COUNT(*) FROM learning_xp_events WHERE kind = 'review-completed'), \
           (SELECT COALESCE(MAX(earned_xp), -1) FROM learning_xp_events WHERE kind = 'review-completed')",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!((log_rows, xp_rows, earned), (1, 1, 0));
    // 作答流水行带完整作答锚（j05 补列）。
    let (elapsed_ms, forgot, response): (Option<i64>, i64, String) = sqlx::query_as(
        "SELECT elapsed_ms, forgot, response FROM learning_review_events",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!((elapsed_ms, forgot), (Some(4_200), 1));
    assert_eq!(response, "null");

    // 同学习日重复作答（卡未再到期）：折叠 —— 作答流水照记，调度、日志、
    // 结算全部不动。
    let (due_at, stability): (i64, f64) = sqlx::query_as(
        "SELECT due_at, stability_days FROM learning_review_items",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    let folded = service
        .answer_review(&card, &user_id, Value::Null, true, Some(1_000))
        .await
        .unwrap();
    assert!(!folded.advanced, "same-day repeat must fold");
    let (due_at_after, stability_after): (i64, f64) = sqlx::query_as(
        "SELECT due_at, stability_days FROM learning_review_items",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!((due_at_after, stability_after), (due_at, stability), "fold must not move the schedule");
    let (log_rows, xp_rows, flow_rows): (i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT COUNT(*) FROM learning_review_log WHERE rating_source != 'synthetic'), \
           (SELECT COUNT(*) FROM learning_xp_events WHERE kind = 'review-completed'), \
           (SELECT COUNT(*) FROM learning_review_events)",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!((log_rows, xp_rows, flow_rows), (1, 1, 1), "fold adds an answer-flow row only");
}

/// 复习日志推进前快照四字段：首推为 NULL，真有记忆状态的推进逐字段铸值。
#[tokio::test]
async fn review_log_casts_pre_push_snapshot_fields() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    let course = service.import_course(valid_pack()).await.unwrap();
    service.enroll(&course.course.id, &user_id).await.unwrap();
    let detail = service
        .course_detail(&course.course.id, Some(&user_id))
        .await
        .unwrap();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();
    make_all_due(&service, &user_id).await;
    let card = service
        .due_reviews(&user_id, 10, &[], true, false, &[])
        .await
        .unwrap()[0]
        .id
        .clone();
    // 首推（无记忆状态）：四字段全 NULL、elapsed_days 0 ——「无则落 null
    // 不省略」。
    service
        .answer_review(&card, &user_id, Value::Null, true, None)
        .await
        .unwrap();
    let (elapsed, stability, difficulty, r_pred): (i64, Option<f64>, Option<f64>, Option<f64>) =
        sqlx::query_as(
            "SELECT elapsed_days, stability_before, difficulty_before, r_pred \
             FROM learning_review_log WHERE rating_source != 'synthetic'",
        )
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
    assert_eq!((elapsed, stability, difficulty, r_pred), (0, None, None, None));

    // 第二次真实推进（重列到期）：四字段逐个从卡片推进前状态铸值。
    make_all_due(&service, &user_id).await;
    service
        .answer_review(&card, &user_id, Value::Null, true, None)
        .await
        .unwrap();
    let (stability, difficulty, r_pred): (Option<f64>, Option<f64>, Option<f64>) = sqlx::query_as(
        "SELECT stability_before, difficulty_before, r_pred \
         FROM learning_review_log WHERE rating_source != 'synthetic' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert!(stability.is_some_and(|value| value > 0.0), "stability_before casts the pre-push state");
    assert!(difficulty.is_some_and(|value| (1.0..=10.0).contains(&value)));
    assert!(r_pred.is_some_and(|value| value > 0.0 && value <= 1.0));
}

/// 复习线自建题：无课程归属结算（锚 custom 段），错答 0 分不误伤。
#[tokio::test]
async fn xp_custom_review_settles_without_course_scope() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    let question_id = service
        .create_custom_question(
            &user_id,
            CreateCustomQuestionRequest {
                kind: ActivityKind::SingleChoice,
                prompt: "2+2=?".into(),
                options: vec!["3".into(), "4".into()],
                answer: json!("4"),
                explanation: String::new(),
                distractors: Vec::new(),
            },
        )
        .await
        .unwrap();
    sqlx::query("UPDATE learning_custom_questions SET due_at = ? WHERE custom_question_id = ?")
        .bind(now_ms() - 1_000)
        .bind(&question_id)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    // 错答：auto-again 推进 → 结算 0 分 partial（错答不罚负，乱猜判据
    // n≥4 才生效）。
    let result = service
        .answer_custom_review(&question_id, &user_id, json!("3"), false)
        .await
        .unwrap();
    assert!(!result.correct && result.advanced);
    let (kind, course_id, node_id, anchor, earned): (
        String,
        Option<String>,
        Option<String>,
        String,
        i64,
    ) = sqlx::query_as(
        "SELECT kind, course_id, node_id, anchor, earned_xp FROM learning_xp_events",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(kind, "review-completed");
    assert_eq!((course_id.as_deref(), node_id.as_deref()), (None, None));
    assert!(anchor.starts_with("review:custom:"), "custom anchor: {anchor}");
    assert_eq!(earned, 0);
}

/// 种卡幂等：重复完成课时 = 已存在跳过，调度原样（不静默重置）。
#[tokio::test]
async fn seeding_review_items_is_idempotent() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    let course = service.import_course(valid_pack()).await.unwrap();
    service.enroll(&course.course.id, &user_id).await.unwrap();
    let detail = service
        .course_detail(&course.course.id, Some(&user_id))
        .await
        .unwrap();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();
    make_all_due(&service, &user_id).await;
    service
        .answer_review(
            &service
                .due_reviews(&user_id, 10, &[], true, false, &[])
                .await
                .unwrap()[0]
                .id
                .clone(),
            &user_id,
            Value::Null,
            true,
            None,
        )
        .await
        .unwrap();
    let (due_at, review_count): (i64, i64) = sqlx::query_as(
        "SELECT due_at, review_count FROM learning_review_items",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    // 再次完成：卡片存在 = 跳过，推进状态原样。
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();
    let (due_at_after, review_count_after, count): (i64, i64, i64) = sqlx::query_as(
        "SELECT due_at, review_count, COUNT(*) FROM learning_review_items",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(count, 1, "no duplicate card");
    assert_eq!(
        (due_at_after, review_count_after),
        (due_at, review_count),
        "existing card keeps its schedule (no silent reset)"
    );
}

/// 读数尺：进度尺分子分母同筛（现存且启用）、两把尺同源互不换算、
/// 尺永不倒挂（settled ≤ total、remaining ≥ 0、ETA 纯投影）。
#[tokio::test]
async fn xp_reading_gauges_never_invert() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    let reading = service.xp_reading(&user_id).await.unwrap();
    assert_eq!(
        (
            reading.today_xp,
            reading.settled_nodes,
            reading.total_nodes,
            reading.remaining_est
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(reading.eta_days, None, "no pace signal → null ETA");

    let course = service.import_course(valid_pack()).await.unwrap();
    service.enroll(&course.course.id, &user_id).await.unwrap();
    let detail = service
        .course_detail(&course.course.id, Some(&user_id))
        .await
        .unwrap();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
    service
        .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
        .await
        .unwrap();
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();

    let reading = service.xp_reading(&user_id).await.unwrap();
    assert_eq!(reading.total_nodes, 1);
    assert_eq!(reading.settled_nodes, 1);
    assert!(reading.settled_nodes <= reading.total_nodes, "尺永不倒挂");
    assert_eq!(reading.remaining_est, 0);
    assert_eq!(reading.eta_days, Some(0), "nothing remains → ETA 0");
    assert_eq!(reading.today_xp, 12, "est 10 + bonus 2");
    assert!(!reading.goal_achieved, "default goal 40 > 12");

    // 日历：恒 84 桶含零桶、末桶 = 今日、不过课程筛。
    let calendar = service.xp_calendar(&user_id, 12).await.unwrap();
    assert_eq!(calendar.days.len(), 84);
    assert!(calendar.days.iter().all(|day| day.xp >= 0));
    assert_eq!(calendar.days.last().unwrap().xp, 12);
    assert_eq!(calendar.days.first().unwrap().xp, 0);
}

/// 打卡口径退役：日达标按 XP（dailyXpGoal），锁定快照记 XP 口径。
#[tokio::test]
async fn checkin_today_locks_on_xp_gauge() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    let course = service.import_course(valid_pack()).await.unwrap();
    service.enroll(&course.course.id, &user_id).await.unwrap();
    let detail = service
        .course_detail(&course.course.id, Some(&user_id))
        .await
        .unwrap();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
    service
        .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
        .await
        .unwrap();
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
        .await
        .unwrap();

    // 缺省目标 40：今日 XP 12 未达标。
    let status = service.checkin_today(&user_id).await.unwrap();
    assert_eq!((status.goal, status.reviewed_count), (40, 12));
    assert!(!status.completed);

    // 目标降到 10：达标并锁定（快照 = XP 口径）。
    sqlx::query(
        "INSERT INTO client_preferences (key, value, updated_at) \
         VALUES ('learning.dailyXpGoal', '10', ?) \
         ON CONFLICT(key) DO UPDATE SET value = '10', updated_at = excluded.updated_at",
    )
    .bind(now_ms())
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    let status = service.checkin_today(&user_id).await.unwrap();
    assert_eq!((status.goal, status.reviewed_count), (10, 12));
    assert!(status.completed);
    assert!(status.locked_at.is_some());
}

/// 结构挂载回合（j08）：真实管线一次挂载（三节点全评估 + hash 锚定落盘），
/// 重跑幂等（待挂集空 = skipped 零写）。
#[tokio::test]
async fn structure_mount_round_mounts_then_skips() {
    use crate::learning_graph::mounting;
    struct ScriptedMount(std::sync::Mutex<Vec<String>>);
    #[async_trait::async_trait]
    impl LearningCompleter for ScriptedMount {
        async fn complete(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<String, nomifun_common::AppError> {
            Err(nomifun_common::AppError::Internal("mount only uses json".into()))
        }
        async fn complete_json_with_thinking(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            _: &str,
            _: u32,
            _: Option<f32>,
        ) -> Result<String, nomifun_common::AppError> {
            let mut replies = self.0.lock().unwrap();
            if replies.is_empty() {
                return Err(nomifun_common::AppError::Internal("script exhausted".into()));
            }
            Ok(replies.remove(0))
        }
    }

    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id.clone()).unwrap();
    let service = LearningService::new(database.pool().clone());
    let artifact = serde_json::json!({
        "mounts": [
            { "node": "节点一", "blocks": ["block-01"], "reason": "同主题" },
            { "node": "节点二", "blocks": ["block-02"], "reason": "进阶同主题" },
            { "node": "节点三", "blocks": [], "reason": "纯练习节点" }
        ],
        "note": "n"
    });
    service.set_generation_dependencies(
        Arc::new(KnowledgeService::new(
            Arc::new(nomifun_db::SqliteKnowledgeRepository::new(
                database.pool().clone(),
            )),
            std::env::temp_dir().join("mount-test-kb").as_path(),
            nomifun_knowledge::KnowledgeEventEmitter::new(
                Arc::new(NoopBroadcaster),
                Arc::from(owner_id.as_str()),
            ),
        )),
        Arc::new(ScriptedMount(std::sync::Mutex::new(vec![serde_json::to_string(
            &artifact,
        )
        .unwrap()]))),
    );

    // 种子：图课程（结构两块）+ 三节点。
    let now = now_ms();
    let course_id = LearningCourseId::new().into_string();
    let module_id = LearningModuleId::new().into_string();
    let envelope = serde_json::json!({
        "version": 1,
        "generated_at": now,
        "structure": {
            "goal": "g",
            "spines": [{ "name": "主干一", "definition": "d", "key_points": [], "instance": "i" }],
            "blocks": [
                { "name": "块一", "slug": "block-01", "spine": "主干一", "stage": "入门", "note": "n" },
                { "name": "块二", "slug": "block-02", "spine": "主干一", "stage": "进阶", "note": "n" }
            ],
            "edges": []
        },
        "terrain": { "doors": [] },
    });
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, structure_json, created_at, updated_at) \
         VALUES (?, '挂载课程', '', 'general', 1, 'learning_graph', 'g', '', ?, ?, ?)",
    )
    .bind(&course_id)
    .bind(serde_json::to_string(&envelope).unwrap())
    .bind(now)
    .bind(now)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(&module_id)
    .bind(&course_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    for (position, title) in ["节点一", "节点二", "节点三"].iter().enumerate() {
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes) \
             VALUES (?, ?, ?, '', 'p', ?, 10)",
        )
        .bind(LearningLessonId::new().into_string())
        .bind(&module_id)
        .bind(title)
        .bind(position as i64)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }

    let course = LearningCourseId::parse(&course_id).unwrap();
    let first = service
        .run_structure_mount(&user_id, &course, None)
        .await
        .unwrap();
    assert_eq!(first.status, "mounted");
    assert_eq!(first.pending_nodes, 3);
    assert_eq!((first.mounted_nodes, first.outside_nodes), (2, 1));
    // hash 锚定：挂载行 hash = 结构字节 FNV-1a 64。
    let structure_raw: String = sqlx::query_scalar(
        "SELECT structure_json FROM learning_courses WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    let (row_hash, mounts_json): (i64, String) = sqlx::query_as(
        "SELECT structure_hash, mounts_json FROM learning_structure_mounts WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(
        row_hash as u64,
        mounting::fnv1a_64(structure_raw.as_bytes())
    );
    let entries = mounting::parse_mount_entries(
        &serde_json::from_str::<serde_json::Value>(&mounts_json).unwrap(),
    );
    assert_eq!(entries.len(), 3);

    // 读侧三态：挂载行 + 结构 + 节点 → 2 mounted / 1 outside / 0 未评估。
    let view = service.structure_view(&course).await.unwrap();
    let mounts = view.mounts.expect("mount row exists");
    assert!(!mounts.stale);
    assert_eq!(mounts.mounted_count, 2);
    assert_eq!(mounts.outside_count, 1);
    assert_eq!(mounts.not_evaluated_count, 0);
    assert_eq!(mounts.index.len(), 2);

    // 重跑幂等：待挂集空 = skipped，零 LLM 零写（脚本已耗尽，任何调用都会
    // 报错——skipped 分支根本不触 completer）。
    let second = service
        .run_structure_mount(&user_id, &course, None)
        .await
        .unwrap();
    assert_eq!(second.status, "skipped");
    assert_eq!(second.total_entries, first.total_entries);

    // 结构变了（改一个字节）→ stale 显影 + 全量重评待挂。
    sqlx::query("UPDATE learning_courses SET structure_json = ? WHERE course_id = ?")
        .bind(&format!("{structure_raw} "))
        .bind(&course_id)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    let view = service.structure_view(&course).await.unwrap();
    let mounts = view.mounts.expect("mount row exists");
    assert!(mounts.stale, "hash mismatch must surface as stale");
}

/// 结构修补回合（j09）：触发门槛（皆空 = skipped 零 LLM 零写）+ findings
/// 留痕驱动（落一条 findings 后触发面非空）+ 审计行落表。
#[tokio::test]
async fn structure_repair_skips_when_no_triggers_and_audits() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id.clone()).unwrap();
    let service = LearningService::new(database.pool().clone());
    // 无 completer：skipped 分支根本不触生成通道，任何意外 LLM 调用都会
    // 报「completer 未配置」——测试因此钉住零 LLM。
    let now = now_ms();
    let course_id = LearningCourseId::new().into_string();
    let envelope = serde_json::json!({
        "version": 1,
        "generated_at": now,
        "structure": {
            "goal": "g",
            "spines": [{ "name": "主干一", "definition": "d", "key_points": [], "instance": "i" }],
            "blocks": [
                { "name": "块一", "slug": "block-01", "spine": "主干一", "stage": "入门", "note": "n" }
            ],
            "edges": []
        },
        "terrain": { "doors": [] },
    });
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, structure_json, created_at, updated_at) \
         VALUES (?, '修补课程', '', 'general', 1, 'learning_graph', 'g', '', ?, ?, ?)",
    )
    .bind(&course_id)
    .bind(serde_json::to_string(&envelope).unwrap())
    .bind(now)
    .bind(now)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    let course = LearningCourseId::parse(&course_id).unwrap();

    // 皆空：skipped（零 LLM 零写，审计也无行）。
    let summary = service.run_structure_repair(&user_id, &course, None).await.unwrap();
    assert_eq!(summary.status, "skipped");
    assert!(summary.skipped);
    let audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_structure_repairs WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(audits, 0, "skipped round leaves no audit row");

    // 落一条 findings 留痕：触发面非空。无 completer 时回合如实报生成通
    // 道不可用（Conflict）——触发门槛本身已验证。
    sqlx::query(
        "INSERT INTO learning_structure_findings \
         (ts, course_id, source, node, field, issue, advice) \
         VALUES (?, ?, 'concept-review', '', '', '测试留痕', '建议')",
    )
    .bind(now_ms())
    .bind(&course_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    let error = service
        .run_structure_repair(&user_id, &course, None)
        .await
        .unwrap_err();
    // 无 completer 的诚实失败：组合根未接生成通道（course_completer 缺席）
    // 或回合面如实报通道不可用——两者都不是条款违约。
    let message = error.to_string();
    assert!(
        message.contains("生成通道不可用") || message.contains("not configured"),
        "findings present but no completer → honest conflict: {message}"
    );
}

    // —— quiz 触发进度（q1） ——

    /// Seeds one course with `count` lessons (est 25 each, each carrying one
    /// single_choice activity answered by "B"), enrolled by `user_id`;
    /// returns (course_id, lesson_ids, activity_ids).
    async fn seed_quiz_course(
        service: &LearningService,
        user_id: &UserId,
        count: usize,
    ) -> (String, Vec<String>, Vec<String>) {
        let now = now_ms();
        let course_id = LearningCourseId::new().into_string();
        let module_id = LearningModuleId::new().into_string();
        let enrollment_id = LearningEnrollmentId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, created_at, updated_at) \
             VALUES (?, 'quiz trigger course', '', 'general', 1, ?, ?)",
        )
        .bind(&course_id)
        .bind(now)
        .bind(now)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
             VALUES (?, ?, 'Module', '', 0)",
        )
        .bind(&module_id)
        .bind(&course_id)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        let mut lesson_ids = Vec::new();
        let mut activity_ids = Vec::new();
        for index in 0..count {
            let lesson_id = LearningLessonId::new().into_string();
            sqlx::query(
                "INSERT INTO learning_lessons \
                 (lesson_id, module_id, title, summary, position, estimated_minutes) \
                 VALUES (?, ?, ?, '', ?, 25)",
            )
            .bind(&lesson_id)
            .bind(&module_id)
            .bind(format!("Lesson {index}"))
            .bind(index as i64)
            .execute(service.pool_for_tests())
            .await
            .unwrap();
            let activity_id = LearningActivityId::new().into_string();
            sqlx::query(
                "INSERT INTO learning_activities \
                 (activity_id, lesson_id, kind, prompt, config_json, position) \
                 VALUES (?, ?, 'single_choice', ?, ?, ?)",
            )
            .bind(&activity_id)
            .bind(&lesson_id)
            .bind(format!("What is {index} + {index}?"))
            .bind(
                r#"{"answer":"B","options":["A","B"],"explanation":"2×n.","distractors":[],"matches":[]}"#,
            )
            .bind(index as i64)
            .execute(service.pool_for_tests())
            .await
            .unwrap();
            lesson_ids.push(lesson_id);
            activity_ids.push(activity_id);
        }
        sqlx::query(
            "INSERT INTO learning_enrollments \
             (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&enrollment_id)
        .bind(user_id.as_str())
        .bind(&course_id)
        .bind(now)
        .bind(now)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        (course_id, lesson_ids, activity_ids)
    }

    async fn complete_quiz_lesson(service: &LearningService, user_id: &UserId, lesson_id: &str) {
        service
            .update_lesson_progress(
                &LearningLessonId::parse(lesson_id).unwrap(),
                user_id,
                LessonStatus::Completed,
            )
            .await
            .unwrap();
    }

    /// 触发计数只认学新结算（learnhub `settlements_since_last_quiz` 同语义）：
    /// 复习账不推进、恰 K 触发、quiz 结算清零重新积累；未知课程 = NotFound。
    #[tokio::test]
    async fn quiz_trigger_counts_learn_settlements_only() {
        let (service, user_id) = checkin_test_service().await;
        let (course_id, lessons, _activities) = seed_quiz_course(&service, &user_id, 7).await;
        let course = LearningCourseId::parse(&course_id).unwrap();

        // 5 个学新结算：N=5 < K=6 未到期；复习账混入不推进触发进度。
        for lesson in &lessons[..5] {
            complete_quiz_lesson(&service, &user_id, lesson).await;
        }
        sqlx::query(
            "INSERT INTO learning_xp_events \
             (event_id, user_id, ts, course_id, node_id, kind, earned_xp, xp_bonus, basis, ratio, origin, anchor) \
             VALUES (?, ?, ?, ?, NULL, 'review-completed', 5, 2, 'full', NULL, 'review-course', 'review:quiz-smoke:1')",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(now_ms())
        .bind(&course_id)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        let view = service.quiz_progress(&user_id, &course).await.unwrap();
        assert_eq!(view.settled_count, 5, "复习账不推进触发进度（只认 node-completed）");
        assert_eq!(view.trigger_count, 6);
        assert!(!view.due, "5 < 6 未到期");

        // 第 6 个学新结算 → 恰 K 触发。
        complete_quiz_lesson(&service, &user_id, &lessons[5]).await;
        let view = service.quiz_progress(&user_id, &course).await.unwrap();
        assert_eq!(view.settled_count, 6);
        assert!(view.due, "6 ≥ 6 到期");

        // quiz 结算后计数清零；其后新学新重新积累。
        sqlx::query(
            "INSERT INTO learning_xp_events \
             (event_id, user_id, ts, course_id, node_id, kind, earned_xp, xp_bonus, basis, ratio, origin, anchor) \
             VALUES (?, ?, ?, ?, ?, 'quiz-completed', 4, 0, 'full', NULL, 'quiz', 'quiz:smoke:1')",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(now_ms())
        .bind(&course_id)
        .bind(&lessons[0])
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        let view = service.quiz_progress(&user_id, &course).await.unwrap();
        assert_eq!(view.settled_count, 0, "quiz 结算清零");
        assert!(!view.due);
        complete_quiz_lesson(&service, &user_id, &lessons[6]).await;
        let view = service.quiz_progress(&user_id, &course).await.unwrap();
        assert_eq!(view.settled_count, 1, "quiz 后重新积累");

        // 未知课程 = NotFound。
        let missing = service.quiz_progress(&user_id, &LearningCourseId::new()).await;
        assert!(matches!(missing, Err(AppError::NotFound(_))), "{missing:?}");
    }

    /// q2 开测全链路前置：due 门（Conflict 附 N/K）→ 到期后出脱敏回合
    /// （判底零字段）。
    #[tokio::test]
    async fn quiz_start_conflicts_before_due_then_carries_sanitized_round() {
        let (service, user_id) = checkin_test_service().await;
        let (course_id, lessons, _activities) = seed_quiz_course(&service, &user_id, 6).await;
        let course = LearningCourseId::parse(&course_id).unwrap();

        let error = service.quiz_start(&user_id, &course).await.unwrap_err();
        assert!(
            matches!(error, AppError::Conflict(ref message) if message.contains("0/6")),
            "未到期 = Conflict 附 N/K: {error:?}"
        );

        for lesson in &lessons {
            complete_quiz_lesson(&service, &user_id, lesson).await;
        }
        let round = service.quiz_start(&user_id, &course).await.unwrap();
        assert_eq!(round.questions.len(), 6, "每覆盖节点 1 题");
        assert_eq!(round.settled_count, 6);
        assert_eq!(round.trigger_count, 6);
        assert_eq!(round.budget, 15);
        assert_eq!(round.status, "active");
        // 脱敏红线：投影零判底（answer/explanation 不出场）。
        let view_json = serde_json::to_string(&round).unwrap();
        assert!(
            !view_json.contains("explanation"),
            "start 投影不得携带解析: {view_json}"
        );
        assert!(
            !view_json.contains("\"answer\""),
            "start 投影不得携带判底: {view_json}"
        );
    }

    /// 选题 LRU：同节点多题时取「最久未出」（从未作答最先；已作答的后置）。
    #[tokio::test]
    async fn quiz_round_selection_is_lru_per_node() {
        let (service, user_id) = checkin_test_service().await;
        let (course_id, lessons, mut activities) = seed_quiz_course(&service, &user_id, 6).await;
        let course = LearningCourseId::parse(&course_id).unwrap();
        // 首节点加第二道题，并先把首题作出答（LRU 应取从未作答的第二题）。
        let second = LearningActivityId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_activities \
             (activity_id, lesson_id, kind, prompt, config_json, position) \
             VALUES (?, ?, 'single_choice', 'backfill?', \
                     '{\"answer\":\"B\",\"options\":[\"A\",\"B\"],\"explanation\":\"\",\"distractors\":[],\"matches\":[]}', 1)",
        )
        .bind(&second)
        .bind(&lessons[0])
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        service
            .submit_attempt(
                &LearningActivityId::parse(&activities[0]).unwrap(),
                &user_id,
                json!("B"),
                None,
                None,
            )
            .await
            .unwrap();
        activities.push(second);

        for lesson in &lessons {
            complete_quiz_lesson(&service, &user_id, lesson).await;
        }
        let round = service.quiz_start(&user_id, &course).await.unwrap();
        let first_picked = round
            .questions
            .iter()
            .find(|q| q.node_id.as_str() == lessons[0])
            .expect("首节点应在覆盖面");
        assert_ne!(
            first_picked.activity_id.as_str(),
            activities[0],
            "已作答的首题让位（LRU:从未作答最先）"
        );
        assert_eq!(round.questions.len(), 6, "每节点仍 1 题");
    }

    /// 全卷结算：分级落账（full/partial 各至少一笔、恒非负）→ 触发清零 →
    /// 首写为终（重复 complete Conflict）→ 同日折叠（第二轮全折叠零计薪）。
    #[tokio::test]
    async fn quiz_round_complete_settles_tiers_folds_and_is_first_write_wins() {
        let (service, user_id) = checkin_test_service().await;
        let (course_id, lessons, activities) = seed_quiz_course(&service, &user_id, 6).await;
        let course = LearningCourseId::parse(&course_id).unwrap();
        for lesson in &lessons {
            complete_quiz_lesson(&service, &user_id, lesson).await;
        }
        let round = service.quiz_start(&user_id, &course).await.unwrap();

        // 判分面：对 2 题、错 1 题（判底随反馈下发）。
        let feedback = service
            .quiz_grade(&user_id, &course, &round.round_id, &activities[0], &json!("B"))
            .await
            .unwrap();
        assert!(feedback.correct, "正确作答判对");
        assert_eq!(feedback.answer, json!("B"), "判底揭示");
        let feedback = service
            .quiz_grade(&user_id, &course, &round.round_id, &activities[1], &json!("A"))
            .await
            .unwrap();
        assert!(!feedback.correct, "错误作答判错");
        // 计划外题判分拒绝。
        let stray = service
            .quiz_grade(&user_id, &course, &round.round_id, "not-in-plan", &json!("B"))
            .await;
        assert!(matches!(stray, Err(AppError::BadRequest(_))), "{stray:?}");

        // 全卷作答（首尾两题错：单题节点 accuracy ∈ {1, 0} → full 3 / partial 0；
        // 断言按 activity 定位——计划序是弱先序，不假设种子序）。
        let wrong: std::collections::HashSet<String> = [activities[0].clone(), activities[5].clone()]
            .into_iter()
            .collect();
        let mut answers = HashMap::new();
        for activity in &activities {
            let response = if wrong.contains(activity) { "A" } else { "B" };
            answers.insert(activity.clone(), json!(response));
        }
        let report = service
            .quiz_complete(&user_id, &course, &round.round_id, &answers)
            .await
            .unwrap();
        assert_eq!(report.earned_xp, 12, "4 × full 3 + 2 × partial 0（恒非负）");
        let activity_of_node = |node: &str| {
            round
                .questions
                .iter()
                .find(|q| q.node_id.as_str() == node)
                .map(|q| q.activity_id.as_str())
                .unwrap()
        };
        for entry in &report.covered {
            let expected = if wrong.contains(
                round
                    .questions
                    .iter()
                    .find(|q| q.node_id.as_str() == entry.node_id.as_str())
                    .map(|q| q.activity_id.as_str())
                    .unwrap(),
            ) {
                Some(0)
            } else {
                Some(3)
            };
            assert_eq!(
                entry.earned_xp, expected,
                "节点 {} 的授予档",
                entry.node_id
            );
        }
        assert_eq!(report.covered.len(), 6);
        assert_eq!(
            report.covered.iter().map(|entry| entry.correct).sum::<i64>(),
            4,
            "4 节点全对、2 节点全错"
        );

        // 落账盘面：6 笔 quiz-completed、总额 12；回合行 completed。
        let (count, total): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(earned_xp), 0) FROM learning_xp_events \
             WHERE kind = 'quiz-completed'",
        )
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
        assert_eq!((count, total), (6, 12));
        let (status, earned): (String, i64) = sqlx::query_as(
            "SELECT status, earned_xp FROM learning_quiz_rounds WHERE round_id = ?",
        )
        .bind(&round.round_id)
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
        assert_eq!((status.as_str(), earned), ("completed", 12));

        // 触发清零；结算后的判分/结算都拒绝（首写为终）。
        let progress = service.quiz_progress(&user_id, &course).await.unwrap();
        assert_eq!(progress.settled_count, 0, "quiz 结算清零触发进度");
        let again = service
            .quiz_complete(&user_id, &course, &round.round_id, &answers)
            .await
            .unwrap_err();
        assert!(matches!(again, AppError::Conflict(_)), "{again:?}");
        let graded = service
            .quiz_grade(&user_id, &course, &round.round_id, &activities[0], &json!("B"))
            .await
            .unwrap_err();
        assert!(matches!(graded, AppError::Conflict(_)), "{graded:?}");

        // 同日折叠：直插第二轮（同计划 active），全对作答 → 逐节点全折叠
        // （coverage 保持、earned 空、账本零新增）。
        let plan_json: String = sqlx::query_scalar(
            "SELECT plan_json FROM learning_quiz_rounds WHERE round_id = ?",
        )
        .bind(&round.round_id)
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
        let round_2 = generate_id();
        sqlx::query(
            "INSERT INTO learning_quiz_rounds \
             (round_id, user_id, course_id, started_at, status, plan_json) \
             VALUES (?, ?, ?, ?, 'active', ?)",
        )
        .bind(&round_2)
        .bind(user_id.as_str())
        .bind(&course_id)
        .bind(now_ms())
        .bind(&plan_json)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        let mut all_correct = HashMap::new();
        for activity in &activities {
            all_correct.insert(activity.clone(), json!("B"));
        }
        let folded = service
            .quiz_complete(&user_id, &course, &round_2, &all_correct)
            .await
            .unwrap();
        assert_eq!(folded.earned_xp, 0, "同日折叠零计薪");
        assert!(
            folded.covered.iter().all(|node| node.earned_xp.is_none()),
            "折叠节点 earned 空（coverage 保持）"
        );
        let (count, total): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(earned_xp), 0) FROM learning_xp_events \
             WHERE kind = 'quiz-completed'",
        )
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
        assert_eq!((count, total), (6, 12), "账本零新增（不重复计薪）");
    }

    /// 作答必满（quiz 必作答，测量语义）：缺题的 complete 拒绝。
    #[tokio::test]
    async fn quiz_complete_requires_every_planned_answer() {
        let (service, user_id) = checkin_test_service().await;
        let (course_id, lessons, activities) = seed_quiz_course(&service, &user_id, 6).await;
        let course = LearningCourseId::parse(&course_id).unwrap();
        for lesson in &lessons {
            complete_quiz_lesson(&service, &user_id, lesson).await;
        }
        let round = service.quiz_start(&user_id, &course).await.unwrap();
        let mut answers = HashMap::new();
        answers.insert(activities[0].clone(), json!("B"));
        let error = service
            .quiz_complete(&user_id, &course, &round.round_id, &answers)
            .await
            .unwrap_err();
        // 计划序是弱先序：点名的是任一缺席题即可（缺题拒绝语义）。
        let message = error.to_string();
        assert!(
            matches!(error, AppError::BadRequest(_))
                && activities[1..]
                    .iter()
                    .any(|activity| message.contains(activity.as_str())),
            "缺题拒绝并点名: {message}"
        );
    }

    /// 同课程至多一个 active 回合：重复 start = supersede（旧回合标
    /// abandoned 且不可再结算），新回合可用（部分唯一索引兜并发）。
    #[tokio::test]
    async fn quiz_start_supersedes_active_round() {
        let (service, user_id) = checkin_test_service().await;
        let (course_id, lessons, activities) = seed_quiz_course(&service, &user_id, 6).await;
        let course = LearningCourseId::parse(&course_id).unwrap();
        for lesson in &lessons {
            complete_quiz_lesson(&service, &user_id, lesson).await;
        }
        let first = service.quiz_start(&user_id, &course).await.unwrap();
        let second = service.quiz_start(&user_id, &course).await.unwrap();
        assert_ne!(first.round_id, second.round_id, "重开 = 新回合");
        let status: String = sqlx::query_scalar(
            "SELECT status FROM learning_quiz_rounds WHERE round_id = ?",
        )
        .bind(&first.round_id)
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
        assert_eq!(status, "abandoned", "被取代回合标 abandoned");
        let mut answers = HashMap::new();
        for activity in &activities {
            answers.insert(activity.clone(), json!("B"));
        }
        let error = service
            .quiz_complete(&user_id, &course, &first.round_id, &answers)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::Conflict(_)), "abandoned 不可结算: {error:?}");
        let report = service
            .quiz_complete(&user_id, &course, &second.round_id, &answers)
            .await
            .unwrap();
        assert_eq!(report.earned_xp, 18, "新回合照常结算（6 × full 3）");
    }

    // —— q3：弱先排序 / LLM 变体 / 校准 ——

    /// 将某节点的首张卡推成「最近一次真实推进 = 错答」（auto Again =
    /// escalation 档样本，同时给卡落 stability）。
    async fn fail_first_card(service: &LearningService, user_id: &UserId, lesson_id: &str) {
        let item_id: String = sqlx::query_scalar(
            "SELECT r.review_item_id FROM learning_review_items r \
             JOIN learning_activities a ON a.activity_id = r.activity_id \
             WHERE a.lesson_id = ? ORDER BY r.id LIMIT 1",
        )
        .bind(lesson_id)
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
        sqlx::query("UPDATE learning_review_items SET due_at = ? WHERE review_item_id = ?")
            .bind(now_ms() - 1_000)
            .bind(&item_id)
            .execute(service.pool_for_tests())
            .await
            .unwrap();
        let answer = service
            .answer_review(
                &LearningReviewItemId::parse(&item_id).unwrap(),
                user_id,
                json!("A"),
                false,
                Some(1_000),
            )
            .await
            .unwrap();
        assert!(!answer.correct, "错答 → auto Again（escalation 样本）");
    }

    /// 弱先七档：escalation 节点先出（最近一次真实推进打脸 = 上卷错题优先
    /// 重出；交错序第 1 槽首位）。
    #[tokio::test]
    async fn quiz_weak_first_orders_escalated_node_first() {
        let (service, user_id) = checkin_test_service().await;
        let (course_id, lessons, _activities) = seed_quiz_course(&service, &user_id, 6).await;
        let course = LearningCourseId::parse(&course_id).unwrap();
        for lesson in &lessons {
            complete_quiz_lesson(&service, &user_id, lesson).await;
        }
        fail_first_card(&service, &user_id, &lessons[3]).await;
        let round = service.quiz_start(&user_id, &course).await.unwrap();
        assert_eq!(
            round.questions[0].node_id.as_str(),
            lessons[3],
            "escalation 节点先出"
        );
        // 未打脸节点信号全同 → ULID 兜底（确定性）：其余槽按 node ULID 升序。
        let rest: Vec<&str> = round.questions[1..]
            .iter()
            .map(|q| q.node_id.as_str())
            .collect();
        let mut sorted = rest.clone();
        sorted.sort_unstable();
        assert_eq!(rest, sorted, "同信号节点按 ULID 兜底");
    }

    /// 变体（q3）：completer 缺席 = 每节点只出旧题（如实跳过）；脚本
    /// completer 在场 = 每节点 1 变式（同型换皮 + 防复用池），变式判分走
    /// plan 内判分底、投影同样零判底，complete 连变式一起结算。
    #[tokio::test]
    async fn quiz_variants_generate_with_completer_and_skip_honestly_without() {
        let (service, user_id) = checkin_test_service().await;
        let (course_id, lessons, _activities) = seed_quiz_course(&service, &user_id, 6).await;
        let course = LearningCourseId::parse(&course_id).unwrap();
        for lesson in &lessons {
            complete_quiz_lesson(&service, &user_id, lesson).await;
        }
        // 无 completer：bank-only。
        let round = service.quiz_start(&user_id, &course).await.unwrap();
        assert_eq!(round.questions.len(), 6, "无 completer = 每节点只出旧题");
        let plan_json: String = sqlx::query_scalar(
            "SELECT plan_json FROM learning_quiz_rounds WHERE round_id = ?",
        )
        .bind(&round.round_id)
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
        assert!(!plan_json.contains(r#""origin":"variant""#), "无 completer 无变式: {plan_json}");

        // 有 completer：每节点 (1 旧题 + 1 变式)。
        let reply = r#"{"kind":"single_choice","prompt":"Variant probe: which property is unique?","options":["A","B"],"answer":"A","explanation":"variant bottom","distractors":[]}"#;
        let kb_dir = std::env::temp_dir().join("quiz-variant-test-kb");
        let knowledge_service = Arc::new(KnowledgeService::new(
            Arc::new(nomifun_db::SqliteKnowledgeRepository::new(
                service.pool_for_tests().clone(),
            )),
            &kb_dir,
            nomifun_knowledge::KnowledgeEventEmitter::new(
                Arc::new(NoopBroadcaster),
                Arc::from(user_id.as_str()),
            ),
        ));
        service.set_generation_dependencies(
            knowledge_service,
            ScriptedCompleter::new(reply, false),
        );
        let round = service.quiz_start(&user_id, &course).await.unwrap();
        assert_eq!(round.questions.len(), 12, "6 节点 × (1 旧题 + 1 变式)");
        // 交错：同节点两题不相邻。
        for pair in round.questions.windows(2) {
            assert_ne!(
                pair[0].node_id, pair[1].node_id,
                "跨节点交错（同节点题不相邻）"
            );
        }
        // 变式投影同样零判底。
        let view_json = serde_json::to_string(&round).unwrap();
        assert!(
            !view_json.contains("explanation") && !view_json.contains("\"answer\""),
            "变式不泄漏判底: {view_json}"
        );
        // 变式判分走 plan 内判分底。
        let variant_question = round
            .questions
            .iter()
            .find(|q| q.prompt.contains("Variant probe"))
            .expect("变式在场");
        let feedback = service
            .quiz_grade(
                &user_id,
                &course,
                &round.round_id,
                variant_question.activity_id.as_str(),
                &json!("A"),
            )
            .await
            .unwrap();
        assert!(feedback.correct, "变式判对（判分底 = plan 内 config）");
        // 全卷结算（变式一起计价）：每节点 2 题全对 → full (2+2)/节点 = 12。
        let mut answers = HashMap::new();
        for question in &round.questions {
            let response = if question.prompt.contains("Variant probe") {
                "A"
            } else {
                "B"
            };
            answers.insert(question.activity_id.to_string(), json!(response));
        }
        let report = service
            .quiz_complete(&user_id, &course, &round.round_id, &answers)
            .await
            .unwrap();
        assert_eq!(report.earned_xp, 24, "6 节点 × full(2+2)");
        assert!(
            report
                .covered
                .iter()
                .all(|node| node.answered == 2 && node.correct == 2),
            "每节点 2 题全对"
        );
    }

    /// 校准（q3）：被真实推进过的卡出题时点带预测 R（Some），未推卡/变式
    /// 为 None 不入均值；quiz_calibration 分桶聚合（预测均值 vs 实际正确率）。
    #[tokio::test]
    async fn quiz_calibration_points_bucket_predictions() {
        let (service, user_id) = checkin_test_service().await;
        let (course_id, lessons, activities) = seed_quiz_course(&service, &user_id, 6).await;
        let course = LearningCourseId::parse(&course_id).unwrap();
        for lesson in &lessons {
            complete_quiz_lesson(&service, &user_id, lesson).await;
        }
        // 无样本读面：空表（不造假数据）。
        let bins_before = service.quiz_calibration(&user_id).await.unwrap();
        assert!(bins_before.is_empty(), "无 completed 回合 = 空校准面");
        fail_first_card(&service, &user_id, &lessons[0]).await;
        let round = service.quiz_start(&user_id, &course).await.unwrap();
        assert_eq!(round.questions[0].node_id.as_str(), lessons[0], "escalation 先出");
        let mut answers = HashMap::new();
        for question in &round.questions {
            answers.insert(question.activity_id.to_string(), json!("B"));
        }
        let report = service
            .quiz_complete(&user_id, &course, &round.round_id, &answers)
            .await
            .unwrap();
        let pushed: Vec<_> = report
            .covered
            .iter()
            .flat_map(|node| &node.questions)
            .filter(|point| point.predicted_r.is_some())
            .collect();
        assert_eq!(pushed.len(), 1, "只有被真实推进过的卡带预测面");
        assert_eq!(pushed[0].activity_id.as_str(), activities[0]);
        let bins = service.quiz_calibration(&user_id).await.unwrap();
        assert_eq!(bins.len(), 1, "单点单桶");
        assert_eq!(bins[0].count, 1);
        assert_eq!(bins[0].actual, Some(1.0), "该题答对 → 桶内实际正确率 1.0");
        assert!(bins[0].predicted > 0.0 && bins[0].predicted <= 1.0);
    }
