    use super::checkin::local_wall_clock_utc_ms;
    use super::course::{recommend_next_lesson, validate_pack};
    use super::progress::evaluate;
    use super::*;
    use crate::models::{ActivityPack, ConceptPack, LessonPack, ModulePack};
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
            knowledge_base_id: base.base.knowledge_base_id.parse().unwrap(),
            domain: None,
            provider_id: None,
            model: None,
            module_count: 3,
            lessons_per_module: 3,
            mode: crate::models::CourseGenerationMode::OnDemand,
        }
    }

    #[tokio::test]
    async fn start_course_job_rejects_active_duplicate_for_same_base() {
        let (service, knowledge_service, owner_id) = job_test_service().await;
        let request = generation_request(&knowledge_service).await;
        // Simulate an already-running job (the background runner may not have
        // claimed it yet): the second submission must be refused with a
        // conflict pointing at the active job.
        let active_job_id = "0190f5fe-7c00-7a00-8000-0000000000ff";
        let request_json = serde_json::to_string(&request).unwrap();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO learning_course_jobs \
             (job_id, user_id, session_id, source, kb_id, request_json, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'queued', ?, ?)",
        )
        .bind(active_job_id)
        .bind(owner_id.as_str())
        .bind(Option::<&str>::None)
        .bind(CourseJobSource::Agent.as_str())
        .bind(request.knowledge_base_id.as_str())
        .bind(&request_json)
        .bind(now)
        .bind(now)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        let error = service
            .start_course_job(request.clone(), &owner_id, CourseJobSource::Http, None)
            .await
            .unwrap_err();
        assert!(
            matches!(&error, AppError::Conflict(message) if message.contains(active_job_id)),
            "expected a conflict naming the active job, got {error}"
        );
        // The slot is per-user: another user's submission on the same base
        // is not blocked by the active job.
        let other_user = nomifun_common::UserId::new();
        service
            .start_course_job(request.clone(), &other_user, CourseJobSource::Http, None)
            .await
            .unwrap();
        // A terminal job releases the slot: the same base can be generated
        // again. Status is not asserted here because the background runner
        // may have already advanced (or failed) the fresh job by the time the
        // response view is built.
        sqlx::query(
            "UPDATE learning_course_jobs SET status = 'completed', updated_at = ? \
             WHERE job_id = ?",
        )
        .bind(now_ms())
        .bind(active_job_id)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        service
            .start_course_job(request, &owner_id, CourseJobSource::Http, None)
            .await
            .unwrap();
    }

    fn valid_pack() -> CoursePack {
        CoursePack {
            title: "Linear Algebra".into(),
            description: "A small generic course".into(),
            domain: "mathematics".into(),
            source_kb_id: None,
            version: 1,
            concepts: vec![ConceptPack {
                key: "vector".into(),
                title: "Vector".into(),
                description: String::new(),
                prerequisites: Vec::new(),
            }],
            modules: vec![ModulePack {
                title: "Foundations".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    estimated_minutes: 10,
                    source: None,
                    concepts: vec!["vector".into()],
                    activities: vec![ActivityPack {
                        kind: ActivityKind::TrueFalse,
                        prompt: "A vector has magnitude and direction.".into(),
                        options: Vec::new(),
                        answer: Value::Bool(true),
                        explanation: "That is the geometric definition.".into(),
                        concepts: vec!["vector".into()],
                    distractors: Vec::new(),
                    }],
                }],
            }],
        }
    }

    #[test]
    fn pack_validation_rejects_unknown_concepts() {
        let mut pack = valid_pack();
        pack.modules[0].lessons[0].concepts = vec!["missing".into()];
        let error = validate_pack(&pack).unwrap_err();
        assert!(error.to_string().contains("unknown concept key"));
    }

    #[test]
    fn pack_validation_rejects_prerequisite_cycles() {
        let mut pack = valid_pack();
        pack.concepts.push(ConceptPack {
            key: "matrix".into(),
            title: "Matrix".into(),
            description: String::new(),
            prerequisites: vec!["vector".into()],
        });
        pack.concepts[0].prerequisites = vec!["matrix".into()];
        let error = validate_pack(&pack).unwrap_err();
        assert!(error.to_string().contains("prerequisite cycle"));
    }

    #[test]
    fn builtin_evaluator_does_not_trust_client_scores() {
        let config = StoredActivityConfig {
            options: Vec::new(),
            answer: Value::Bool(true),
            explanation: "source-backed explanation".into(),
            distractors: Vec::new(),
        };
        let (score, _) = evaluate(ActivityKind::TrueFalse, &config, &Value::Bool(false)).unwrap();
        assert_eq!(score, 0.0);
    }

    #[test]
    fn recommendation_repairs_out_of_order_prerequisites() {
        let prerequisite_id = LearningConceptId::new();
        let advanced_id = LearningConceptId::new();
        let prerequisite_lesson_id = LearningLessonId::new();
        let advanced_lesson_id = LearningLessonId::new();
        let lesson = |id: LearningLessonId, title: &str, concept: LearningConceptId| LessonView {
            id,
            title: title.into(),
            summary: String::new(),
            purpose: String::new(),
            position: 0,
            estimated_minutes: 10,
            generated: true,
            source: None,
            status: LessonStatus::NotStarted,
            concepts: vec![concept],
            activities: Vec::new(),
        };
        let modules = vec![ModuleView {
            id: LearningModuleId::new(),
            title: "Module".into(),
            description: String::new(),
            position: 0,
            lessons: vec![
                lesson(advanced_lesson_id, "Advanced", advanced_id.clone()),
                lesson(
                    prerequisite_lesson_id.clone(),
                    "Prerequisite",
                    prerequisite_id.clone(),
                ),
            ],
        }];
        let concepts = vec![
            ConceptView {
                id: prerequisite_id.clone(),
                key: "prerequisite".into(),
                title: "Prerequisite".into(),
                description: String::new(),
                prerequisites: Vec::new(),
                mastery: None,
            },
            ConceptView {
                id: advanced_id,
                key: "advanced".into(),
                title: "Advanced".into(),
                description: String::new(),
                prerequisites: vec![prerequisite_id],
                mastery: None,
            },
        ];
        assert_eq!(
            recommend_next_lesson(&modules, &concepts),
            Some(prerequisite_lesson_id)
        );
    }

    #[tokio::test]
    async fn imports_enrolls_and_updates_mastery() {
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
        assert_eq!(diagnostic.total_concepts, 1);
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
        assert_eq!(detail.concepts[0].mastery, Some(1.0));
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

        // One concept shared across two lessons: completing lesson A seeds a
        // review item for its own question A1, lesson B is never touched.
        let shared = ActivityPack {
            kind: ActivityKind::TrueFalse,
            prompt: String::new(),
            options: Vec::new(),
            answer: Value::Bool(true),
            explanation: String::new(),
            concepts: vec!["shared".into()],
            distractors: Vec::new(),
        };
        let pack = CoursePack {
            title: "Shared Concepts".into(),
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            concepts: vec![ConceptPack {
                key: "shared".into(),
                title: "Shared".into(),
                description: String::new(),
                prerequisites: Vec::new(),
            }],
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
                        concepts: vec!["shared".into()],
                        activities: vec![ActivityPack {
                            prompt: "A1".into(),
                            ..shared.clone()
                        }],
                    },
                    LessonPack {
                        title: "Lesson B".into(),
                        summary: String::new(),
                        purpose: String::new(),
                        estimated_minutes: 10,
                        source: None,
                        concepts: vec!["shared".into()],
                        activities: vec![ActivityPack {
                            prompt: "A2".into(),
                            ..shared
                        }],
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

    /// A reflection-only course with two concepts: the activity targets
    /// "vector" while the full course list also carries "matrix" — the
    /// coverage checklist AI grading must evaluate against.
    fn reflection_pack() -> CoursePack {
        CoursePack {
            title: "Reflective Learning".into(),
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            concepts: vec![
                ConceptPack {
                    key: "vector".into(),
                    title: "Vector".into(),
                    description: "magnitude and direction".into(),
                    prerequisites: Vec::new(),
                },
                ConceptPack {
                    key: "matrix".into(),
                    title: "Matrix".into(),
                    description: "rectangular number grid".into(),
                    prerequisites: Vec::new(),
                },
            ],
            modules: vec![ModulePack {
                title: "Foundations".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    estimated_minutes: 10,
                    source: None,
                    concepts: vec!["vector".into(), "matrix".into()],
                    activities: vec![ActivityPack {
                        kind: ActivityKind::Reflection,
                        prompt: "Explain what a vector is.".into(),
                        options: Vec::new(),
                        answer: Value::Null,
                        explanation: "A vector has magnitude and direction.".into(),
                        concepts: vec!["vector".into()],
                    distractors: Vec::new(),
                    }],
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
        // The grading prompt carries the answer AND the exercise's linked
        // concepts (its own lesson's concepts — never other lessons').
        let user = completer.last_user.lock().unwrap().clone().unwrap();
        assert!(user.contains("Explain what a vector is."));
        assert!(user.contains("A vector is a quantity"));
        assert!(user.contains("Vector"));
        // The AI score feeds the mastery state and the persisted attempt.
        let (mastery,): (f64,) = sqlx::query_as(
            "SELECT mastery FROM learning_mastery_states \
             WHERE enrollment_id = (SELECT enrollment_id FROM learning_enrollments WHERE user_id = ?)",
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(mastery, 0.75);
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
        assert!(error.to_string().contains("unparseable reflection grading reply"));
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
            concepts: vec![ConceptPack {
                key: "vector".into(),
                title: "Vector".into(),
                description: String::new(),
                prerequisites: Vec::new(),
            }],
            modules: vec![ModulePack {
                title: "Foundations".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    estimated_minutes: 10,
                    source: None,
                    concepts: vec!["vector".into()],
                    activities: vec![ActivityPack {
                        kind: ActivityKind::FillInBlank,
                        prompt: "A vector has ___ and direction.".into(),
                        options: Vec::new(),
                        answer: json!(["magnitude"]),
                        explanation: "That is the geometric definition.".into(),
                        concepts: vec!["vector".into()],
                        distractors: vec!["length".into()],
                    }],
                }],
            }],
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
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            concepts: vec![ConceptPack {
                key: "vector".into(),
                title: "Vector".into(),
                description: String::new(),
                prerequisites: Vec::new(),
            }],
            modules: vec![ModulePack {
                title: "Module".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    purpose: String::new(),
                    estimated_minutes: 10,
                    source: None,
                    concepts: vec!["vector".into()],
                    activities: vec![
                        ActivityPack {
                            kind: ActivityKind::SingleChoice,
                            prompt: "Which term names the size of a vector?".into(),
                            options: vec![
                                "magnitude".into(),
                                "speed".into(),
                                "velocity".into(),
                            ],
                            answer: json!("magnitude"),
                            explanation: String::new(),
                            concepts: vec!["vector".into()],
                            distractors: Vec::new(),
                        },
                        ActivityPack {
                            kind: ActivityKind::FillInBlank,
                            prompt: "A vector has ___ and direction.".into(),
                            options: Vec::new(),
                            answer: json!(["magnitude"]),
                            explanation: String::new(),
                            concepts: vec!["vector".into()],
                            distractors: vec!["length".into()],
                        },
                    ],
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
    async fn custom_questions_accept_fill_in_blank() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let request = CreateCustomQuestionRequest {
            kind: ActivityKind::FillInBlank,
            prompt: "The derivative of position is ___, and it measures the rate of change.".into(),
            options: Vec::new(),
            answer: json!(["velocity", "velocity vector"]),
            explanation: "Velocity is the rate of change of position.".into(),
            concept_id: None,
            distractors: vec!["speed".into()],
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
        assert_eq!(kind, "fill_in_blank");
        let config: StoredActivityConfig = serde_json::from_str(&config_json).unwrap();
        assert_eq!(config.answer, json!(["velocity", "velocity vector"]));
        assert_eq!(config.distractors, vec!["speed"]);
        // The custom blank joins the orphan queue due on the next review day
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
            .answer_custom_review(
                &question_id,
                &user_id,
                Value::String("Velocity Vector".into()),
                false,
            )
            .await
            .unwrap();
        assert!(result.correct);
        // Payload validation rejects missing blanks, non-array answers and
        // the reflection kind, mirroring the generated side.
        let invalid = |prompt: &str, answer: Value| CreateCustomQuestionRequest {
            kind: ActivityKind::FillInBlank,
            prompt: prompt.into(),
            options: Vec::new(),
            answer,
            explanation: String::new(),
            concept_id: None,
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
                    concept_id: None,
                    distractors: Vec::new(),
                },
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("only support"));
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
             (custom_question_id, user_id, kind, prompt, config_json, concept_id, \
              due_at, stability_days, difficulty, review_count, lapse_count, \
              last_reviewed_at, created_at, updated_at) \
             VALUES (?, ?, 'true_false', 'p', '{}', NULL, ?, 0, 5.0, 0, 0, NULL, ?, ?)",
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
             (custom_question_id, user_id, kind, prompt, config_json, concept_id, \
              due_at, stability_days, difficulty, review_count, lapse_count, \
              last_reviewed_at, created_at, updated_at) \
             VALUES (?, ?, 'true_false', 'p', '{}', NULL, ?, 0, 5.0, 0, 0, NULL, ?, ?)",
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
