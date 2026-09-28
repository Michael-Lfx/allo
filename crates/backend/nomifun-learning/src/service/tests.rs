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
            teaching_style: None,
            knowledge_base_id: Some(base.base.knowledge_base_id.parse().unwrap()),
            description: None,
            domain: None,
            provider_id: None,
            model: None,
            mode: crate::models::CourseGenerationMode::OnDemand,
            endpoints: Vec::new(),
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
                    estimated_minutes: 10,
                    source: None,
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
            estimated_minutes: 10,
            source,
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
                serde_json::to_string(&blueprint).unwrap(),
                serde_json::to_string(&samples).unwrap(),
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
            estimated_minutes: 10,
            generated: true,
            source: None,
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
                        estimated_minutes: 10,
                        source: None,
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
                        estimated_minutes: 10,
                        source: None,
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
                    estimated_minutes: 10,
                    source: None,
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
                    estimated_minutes: 10,
                    source: None,
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
                    estimated_minutes: 10,
                    source: None,
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

    async fn set_checkin_goal(service: &LearningService, goal: i64) {
        sqlx::query(
            "INSERT INTO client_preferences (key, value, updated_at) VALUES \
             ('learning.dailyCheckinGoal', ?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(goal.to_string())
        .bind(now_ms())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
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
    async fn checkin_locks_when_goal_reached() {
        let (service, user_id) = checkin_test_service().await;
        set_checkin_goal(&service, 5).await;
        let now = now_ms();
        for _ in 0..5 {
            insert_review_event(&service, &user_id, now).await;
        }
        let status = service.checkin_today(&user_id).await.unwrap();
        assert_eq!(status.reviewed_count, 5);
        assert_eq!(status.goal, 5);
        assert!(status.completed, "goal reached must complete the day");
        assert!(status.locked_at.is_some());
        assert_eq!(checkin_rows(&service, &user_id).await, 1);
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

    #[tokio::test]
    async fn checkin_completes_after_reviewing_then_clearing_queue() {
        let (service, user_id) = checkin_test_service().await;
        // goal = 0: no count target, clearing the queue after at least one
        // review completes the day.
        set_checkin_goal(&service, 0).await;
        let before = service.checkin_today(&user_id).await.unwrap();
        assert_eq!(before.goal, 0);
        assert_eq!(before.reviewed_count, 0);
        assert_eq!(before.due_count, 0);
        assert!(!before.completed, "an empty queue without review is not a check-in");
        assert_eq!(checkin_rows(&service, &user_id).await, 0);
        insert_review_event(&service, &user_id, now_ms()).await;
        let status = service.checkin_today(&user_id).await.unwrap();
        assert_eq!(status.reviewed_count, 1);
        assert_eq!(status.due_count, 0);
        assert!(status.completed, "queue cleared after reviewing must complete");
        assert_eq!(checkin_rows(&service, &user_id).await, 1);
    }

    #[tokio::test]
    async fn checkin_stays_locked_after_new_due_cards() {
        let (service, user_id) = checkin_test_service().await;
        // 先刷一张卡（队列本就为空）→ 清空条件锁定当天。
        insert_review_event(&service, &user_id, now_ms()).await;
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
    /// assumes 同概念 @know——依赖由概念网折叠，没有先修边）+ 一条终点锚
    /// （标题行 + 零正文标记课时行）+ 概念登记表行。
    async fn seed_graph_course(
        service: &LearningService,
    ) -> (String, String, String) {
        let pool = service.pool_for_tests();
        let course_id = nomifun_common::LearningCourseId::new();
        let module_id = nomifun_common::LearningModuleId::new();
        let lesson_a = nomifun_common::LearningLessonId::new();
        let lesson_b = nomifun_common::LearningLessonId::new();
        let endpoint_id = nomifun_common::LearningEndpointId::new();
        let endpoint_lesson_id = nomifun_common::LearningLessonId::new();
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
        // 一条终点锚：标记课时行 + 终点行成对落库（零正文，不入可学列表）。
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
              content_generated) VALUES (?, ?, '期末终点', '', '', 2, 1, 0)",
        )
        .bind(endpoint_lesson_id.as_str())
        .bind(module_id.as_str())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_course_endpoints \
             (endpoint_id, course_id, lesson_id, title, goal_note, declared_at) \
             VALUES (?, ?, ?, '期末终点', '能独立完成综合题', ?)",
        )
        .bind(endpoint_id.as_str())
        .bind(course_id.as_str())
        .bind(endpoint_lesson_id.as_str())
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        (
            course_id.into_string(),
            lesson_a.into_string(),
            lesson_b.into_string(),
        )
    }

    /// 图课程的目录级删除（不勾“同时删除复习数据”）：概念网引用随课时行
    /// 一并保留（复习体系继续引用），课程行消失即可——ADR-0009 之后不存在
    /// 会泄漏的前置边表。
    #[tokio::test]
    async fn graph_course_catalog_delete_keeps_lessons_and_concept_web() {
        let (service, _knowledge, owner_id) = job_test_service().await;
        let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
        service
            .delete_course(
                &nomifun_common::LearningCourseId::parse(&course_id).unwrap(),
                &owner_id,
                false,
            )
            .await
            .unwrap();
        let pool = service.pool_for_tests();
        let courses: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_courses WHERE course_id = ?",
        )
        .bind(&course_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(courses, 0, "the catalog row must disappear");
        let lessons: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_lessons")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(
            lessons, 3,
            "reviewable content (nodes + endpoint marker) must survive the catalog delete"
        );
        let refs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_lesson_concepts")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(
            refs, 2,
            "concept web refs stay attached to their surviving lessons"
        );
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
                estimated_minutes: 10,
                source: None,
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
    assert!(graph.compass.is_none());

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

/// 终点锚 CRUD（服务路径）：add_endpoint 成对落终点行 + 零正文标记课时
/// 行；图视图列出终点；delete_endpoint 一并清理两行；未知终点 404。
#[tokio::test]
async fn endpoint_add_and_delete_roundtrip_cleans_marker_lesson() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let owner = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();

    let view = service
        .add_endpoint(
            &owner,
            &course,
            &crate::models::EndpointInput {
                title: "  能独立建模  ".into(),
                goal_note: "拿到真实数据也能建模".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(view.title, "能独立建模", "endpoint title is trimmed");
    assert!(!view.completed, "fresh endpoints are never completed");
    let endpoints: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_course_endpoints WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert_eq!(endpoints, 2, "fixture endpoint + the new one");
    let marker: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_lessons WHERE lesson_id = ?")
            .bind(view.lesson_id.as_str())
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(marker, 1, "the endpoint marker lesson row exists");

    // 图视图列出两条终点。
    let detail = service
        .course_detail(&course, Some(&owner))
        .await
        .unwrap();
    let graph = detail.graph.expect("graph view on a graph course");
    assert_eq!(graph.endpoints.len(), 2);
    assert!(
        graph
            .endpoints
            .iter()
            .any(|endpoint| endpoint.endpoint_id == view.endpoint_id)
    );

    // 删除终点连带清理标记课时行。
    let endpoint = nomifun_common::LearningEndpointId::parse(&view.endpoint_id).unwrap();
    service
        .delete_endpoint(&owner, &course, &endpoint)
        .await
        .unwrap();
    let endpoints: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_course_endpoints WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert_eq!(endpoints, 1);
    let marker: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_lessons WHERE lesson_id = ?")
            .bind(view.lesson_id.as_str())
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(marker, 0, "the marker lesson row is cleaned up");

    // 未知终点删除 404。
    let error = service
        .delete_endpoint(&owner, &course, &nomifun_common::LearningEndpointId::new())
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::NotFound(_)));
}

/// 轮询等待后台生长落库（后台任务与测试共享 current_thread 运行时，
/// 每次 await 让出即推进一步；超时则失败）。
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

/// 生长落库路径（服务级）：scripted completer 让教练起草一个合法批次
/// （铸名 + 两个批内自足节点），kick_growth 后批次经结构门与概念评审
/// 落库——登记表、节点、概念网边、批次档案各就各位。
#[tokio::test]
async fn growth_applies_a_gated_batch_to_registry_and_lessons() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    // 批内自足：极限 随批铸名并由首节点教到「知道」，第二节点假定同档
    // ——结构门放行；评审 completer 返回同 JSON，problems 缺省为空即通过。
    let batch_json = r#"{
        "nodes": [
            {"title": "用定义计算极限", "purpose": "首批基节点", "minutes": 15,
             "teaches": [{"name": "极限", "tier": "know"}], "assumes": []},
            {"title": "用极限法则求切线", "purpose": "衔接节点", "minutes": 20,
             "teaches": [{"name": "求导法则", "tier": "apply"}],
             "assumes": [{"name": "极限", "tier": "know"}]}
        ],
        "mints": [
            {"canonical": "极限", "aliases": ["Limits"], "definition": "无限逼近的值"},
            {"canonical": "求导法则", "aliases": [], "definition": ""}
        ],
        "completed_endpoints": [],
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
    // 节点：夹具 2 节点 + 1 终点标记 + 本批 2 节点。
    let lessons: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lessons l \
         JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(lessons, 5);
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
         WHERE m.course_id = ? AND l.position >= 3",
    )
    .bind(&course_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(generated, 0, "grown nodes start without content");
}

/// 停摆两臂（ADR-0009 Amendment 1）：全部终点被教练标记完成 → 自动触发
/// 退出（Ok(false)），手动 force 豁免（Ok(true)）；空图课程同理走臂 1。
#[tokio::test]
async fn growth_stall_arms_suppress_auto_but_not_force() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    // 就绪 2 < 触发线 3：若不停摆，自动触发本应启动。先把终点标完成 → 臂 2。
    sqlx::query("UPDATE learning_course_endpoints SET completed = 1 WHERE course_id = ?")
        .bind(course.as_str())
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    let kicked = service
        .kick_growth(&owner, &course, false, None)
        .await
        .unwrap();
    assert!(!kicked, "all endpoints completed must suppress auto trigger");
    // 手动 force 豁免停摆（返回 true 即已进入生长管道；无 completer 的失败
    // 只留日志，不影晌本断言）。
    let forced = service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    assert!(forced, "manual force must bypass the stall arms");

    // 臂 1：零节点图（无课时无终点）。
    let (service2, _knowledge2, owner2) = job_test_service().await;
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
}

/// 空手批（0 节点 + 终点裁决）：pending 行删除不落档案，终点完成位仍生效
/// ——「裁决终点完成」不需要以落一批节点为代价。
#[tokio::test]
async fn growth_idle_batch_applies_verdicts_without_batch_row() {
    let (service, _knowledge, owner) = job_test_service().await;
    let (course_id, _lesson_a, _lesson_b) = seed_graph_course(&service).await;
    let course = nomifun_common::LearningCourseId::parse(&course_id).unwrap();
    let idle_json = r#"{
        "nodes": [],
        "mints": [],
        "completed_endpoints": [ { "title": "期末终点", "completed": true } ],
        "note": "无新增，裁决终点完成"
    }"#;
    let completer = ScriptedCompleter::new(idle_json, false);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&owner, &course, true, None)
        .await
        .unwrap();
    // 空手批：等终态观察点（裁决生效）出现，再断言不残留批次行。
    let pool = service.pool_for_tests();
    let mut completed: i64 = 0;
    for _ in 0..500 {
        completed = sqlx::query_scalar(
            "SELECT completed FROM learning_course_endpoints WHERE course_id = ?",
        )
        .bind(course.as_str())
        .fetch_one(pool)
        .await
        .unwrap();
        if completed == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(completed, 1, "verdict applies even without a batch");
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_growth_batches WHERE course_id = ?",
    )
    .bind(course.as_str())
    .fetch_one(pool)
    .await
    .unwrap();
    if rows != 0 {
        let (status, seq, node_ids, note): (String, i64, String, String) = sqlx::query_as(
            "SELECT status, seq, node_ids_json, note FROM learning_growth_batches WHERE course_id = ?",
        )
        .bind(course.as_str())
        .fetch_one(pool)
        .await
        .unwrap();
        panic!("debug leftover: status={status} seq={seq} nodes={node_ids} note={note}");
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
            {"title": "用定义计算极限", "purpose": "后续节点", "minutes": 15,
             "teaches": [{"name": "极限", "tier": "know"}], "assumes": []}
        ],
        "mints": [ { "canonical": "极限", "aliases": [], "definition": "" } ],
        "completed_endpoints": [],
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
