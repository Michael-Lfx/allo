//! One-shot seeding of the tutorial knowledge base and the example learning
//! course, gated on a `.version` file under `{data_dir}/tutorial-learning/`.
//!
//! The knowledge base is registered through the injected `KnowledgeService`
//! and the example course is imported from a frozen asset produced ONCE by
//! the real multi-stage generation pipeline (blueprint → per-lesson deep
//! documents) against the tutorial knowledge base. The seed itself needs no
//! LLM call and works on a fresh install with no provider configured. Once
//! the version file is written the seed never runs again — even if the user
//! deletes the tutorial content — until a factory reset clears `data_dir`.

use std::path::Path;

use nomifun_common::{AppError, KnowledgeBaseId};

use crate::models::CoursePack;
use crate::service::LearningService;

pub const TUTORIAL_KB_NAME: &str = "Flowy 使用指南";
pub const TUTORIAL_KB_DESCRIPTION: &str =
    "Flowy 自带教程知识库：学习模块使用指南与文档规范（描述/例子/验证三要素必备、可选小节按需取舍），也是示例课程的生成语料。";
pub const TUTORIAL_COURSE_TITLE: &str = "学习模块上手指南";

const VERSION_DIR_NAME: &str = "tutorial-learning";
const VERSION_FILE_NAME: &str = ".version";

impl LearningService {
    /// Seeds the tutorial knowledge base and example course once per binary
    /// version. Returns `true` when content was written, `false` when the
    /// version gate said "skip". Failures propagate to the caller, which
    /// treats them as non-fatal at startup.
    ///
    /// Idempotency does NOT rely on the version file alone: the seed also
    /// reuses an existing knowledge base by name and compares the preset
    /// course version against the asset — absent course is imported, a
    /// same-version course is skipped, a stale course is replaced. A lost
    /// version gate (crash mid-seed, version bump, fresh data dir, concurrent
    /// duplicate boot) can therefore never create a duplicate tutorial course.
    pub async fn seed_tutorial_content(&self, data_dir: &Path) -> Result<bool, AppError> {
        let version = env!("CARGO_PKG_VERSION");
        // The gate covers both the app version and the preset course version:
        // bumping either re-runs the seed (and the course version comparison
        // below then upgrades or keeps the existing course).
        let asset_version = tutorial_course_version();
        let gate_content = format!("{version}\n{asset_version}");
        let gate_dir = data_dir.join(VERSION_DIR_NAME);
        let version_file = gate_dir.join(VERSION_FILE_NAME);
        if std::fs::read_to_string(&version_file).ok().as_deref() == Some(gate_content.as_str()) {
            return Ok(false);
        }

        let knowledge_service = self.injected_knowledge_service()?;

        // Reuse an existing tutorial base instead of registering a duplicate.
        let base_id = match knowledge_service
            .find_base_id_by_name(TUTORIAL_KB_NAME)
            .await?
        {
            Some(base_id) => base_id,
            None => knowledge_service
                .create_base(TUTORIAL_KB_NAME, TUTORIAL_KB_DESCRIPTION, None, None)
                .await?
                .knowledge_base_id,
        };
        // The guide files are cheap to refresh (atomic overwrite) and may have
        // been deleted by the user since the last seed, so they are written on
        // every re-seed regardless of whether the base was just created.
        knowledge_service
            .write_file(
                base_id.as_str(),
                "README.md",
                include_str!("../assets/tutorial/README.md"),
            )
            .await?;
        knowledge_service
            .write_file(
                base_id.as_str(),
                "LEARNING_GUIDE.md",
                include_str!("../assets/tutorial/LEARNING_GUIDE.md"),
            )
            .await?;

        // Course version ladder: absent → import, same version → skip,
        // stale version → replace (delete + import) so content updates land.
        match self.course_version_by_title(TUTORIAL_COURSE_TITLE).await? {
            None => {
                self.import_course(tutorial_course_pack(base_id)).await?;
            }
            Some(existing) if existing >= asset_version => {}
            Some(_) => {
                self.delete_courses_by_title(TUTORIAL_COURSE_TITLE).await?;
                self.import_course(tutorial_course_pack(base_id)).await?;
            }
        }

        std::fs::create_dir_all(&gate_dir).map_err(|error| {
            AppError::Internal(format!(
                "create tutorial seed gate dir {}: {error}",
                gate_dir.display()
            ))
        })?;
        // Atomic write: stage next to the target, then rename into place so a
        // crash mid-write never leaves a truncated version file that would
        // trigger a duplicate seed on the next boot.
        let staging = version_file.with_file_name(format!("{VERSION_FILE_NAME}.tmp"));
        std::fs::write(&staging, gate_content).map_err(|error| {
            AppError::Internal(format!(
                "write tutorial seed version file {}: {error}",
                staging.display()
            ))
        })?;
        std::fs::rename(&staging, &version_file).map_err(|error| {
            AppError::Internal(format!(
                "finalize tutorial seed version file {}: {error}",
                version_file.display()
            ))
        })?;
        Ok(true)
    }
}

/// The version stamp of the frozen tutorial course asset. The seed gate
/// embeds it so bumping the asset re-runs the seed and upgrades the course.
fn tutorial_course_version() -> i64 {
    let pack: CoursePack =
        serde_json::from_str(include_str!("../assets/tutorial/course.json"))
            .expect("tutorial course asset must parse as CoursePack");
    pack.version
}

/// The example course is a frozen asset produced by one real run of the
/// multi-stage generation pipeline (blueprint → per-lesson deep documents)
/// against the tutorial knowledge base, so the preset course is
/// representative of actual generation output while the seed itself never
/// calls the LLM.
fn tutorial_course_pack(kb_id: KnowledgeBaseId) -> CoursePack {
    let mut pack: CoursePack =
        serde_json::from_str(include_str!("../assets/tutorial/course.json"))
            .expect("tutorial course asset must parse as CoursePack");
    pack.source_kb_id = Some(kb_id);
    pack
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ActivityKind;
    use nomifun_api_types::WebSocketMessage;
    use nomifun_common::UserId;
    use nomifun_knowledge::KnowledgeService;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[derive(Default)]
    struct NoopBroadcaster;

    impl nomifun_realtime::UserEventSink for NoopBroadcaster {
        fn send_to_user(
            &self,
            _user_id: &str,
            _event: WebSocketMessage<serde_json::Value>,
        ) {
        }
    }

    async fn seeded_service(
        data_dir: &Path,
    ) -> (LearningService, Arc<KnowledgeService>) {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let knowledge_service = Arc::new(KnowledgeService::new(
            Arc::new(nomifun_db::SqliteKnowledgeRepository::new(
                database.pool().clone(),
            )),
            data_dir,
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
        (learning_service, knowledge_service)
    }

    /// Placeholder completer: the seed path never calls the LLM, but
    /// `set_generation_dependencies` requires a completer value.
    struct UnusedCompleter;

    #[async_trait::async_trait]
    impl nomifun_knowledge::KnowledgeCompleter for UnusedCompleter {
        async fn complete(
            &self,
            _system: &str,
            _user: &str,
        ) -> Result<String, nomifun_common::AppError> {
            Err(nomifun_common::AppError::Internal(
                "tutorial seed does not invoke the completer".into(),
            ))
        }
    }

    #[test]
    fn tutorial_lessons_follow_the_new_document_rule() {
        let pack = tutorial_course_pack(KnowledgeBaseId::new());
        assert_eq!(pack.modules.len(), 3);
        let total_lessons: usize = pack.modules.iter().map(|m| m.lessons.len()).sum();
        assert_eq!(total_lessons, 6);

        let concept_keys: HashSet<&str> = pack
            .concepts
            .iter()
            .map(|concept| concept.key.as_str())
            .collect();
        assert_eq!(concept_keys.len(), 6);

        for module in &pack.modules {
            for lesson in &module.lessons {
                // 描述/例子/验证 three required sections appear in order.
                let mut position = 0;
                for section in ["## 描述", "## 例子", "## 验证"] {
                    let relative = lesson.summary[position..]
                        .find(section)
                        .unwrap_or_else(|| {
                            panic!(
                                "lesson {} is missing section {section}",
                                lesson.title
                            )
                        });
                    position += relative + section.len();
                }
                // The pipeline targets 1000-1500 chars; keep a tolerant floor.
                let chars = lesson.summary.trim().chars().count();
                assert!(
                    chars >= 800,
                    "lesson {} summary too short ({chars} chars)",
                    lesson.title
                );
                // 3-5 activities with at least 2 objective ones, so lessons
                // can feed diagnostics and the review queue.
                let activity_count = lesson.activities.len();
                assert!(
                    (3..=5).contains(&activity_count),
                    "lesson {} has {activity_count} activities",
                    lesson.title
                );
                let objective = lesson
                    .activities
                    .iter()
                    .filter(|a| a.kind != ActivityKind::Reflection)
                    .count();
                assert!(
                    objective >= 2,
                    "lesson {} lacks objective activities",
                    lesson.title
                );
                for activity in &lesson.activities {
                    // Every activity binds a concept key from the blueprint.
                    assert!(
                        activity
                            .concepts
                            .iter()
                            .all(|key| concept_keys.contains(key.as_str())),
                        "lesson {} activity {:?} binds an unknown concept",
                        lesson.title,
                        activity.prompt
                    );
                    // Answer formats: single choice picks an option, true/false
                    // is a boolean, reflection carries no answer.
                    match activity.kind {
                        ActivityKind::SingleChoice => {
                            let answer = activity
                                .answer
                                .as_str()
                                .expect("single choice answer is a string");
                            assert!(
                                activity.options.iter().any(|option| option == answer),
                                "lesson {} single choice answer not in options: {answer}",
                                lesson.title
                            );
                        }
                        ActivityKind::TrueFalse => {
                            assert!(
                                activity.answer.is_boolean(),
                                "lesson {} true_false answer is not a boolean",
                                lesson.title
                            );
                        }
                        ActivityKind::Reflection => {
                            assert!(
                                activity.answer.is_null(),
                                "lesson {} reflection answer is not null",
                                lesson.title
                            );
                        }
                        ActivityKind::FillInBlank => {
                            assert!(
                                activity.prompt.contains("___"),
                                "lesson {} fill_in_blank prompt lacks a ___ blank",
                                lesson.title
                            );
                            assert!(
                                activity.answer.as_array().is_some_and(|answers| {
                                    !answers.is_empty() && answers.len() <= 3
                                }),
                                "lesson {} fill_in_blank answer must be 1-3 accepted answers",
                                lesson.title
                            );
                            assert!(
                                !activity.distractors.is_empty(),
                                "lesson {} fill_in_blank lacks distractor traps",
                                lesson.title
                            );
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn seed_is_idempotent_and_links_course_to_knowledge_base() {
        let data_dir = tempfile::tempdir().unwrap();
        let (learning_service, knowledge_service) = seeded_service(data_dir.path()).await;

        assert!(learning_service
            .seed_tutorial_content(data_dir.path())
            .await
            .unwrap());
        // Second call is gated by the version file.
        assert!(!learning_service
            .seed_tutorial_content(data_dir.path())
            .await
            .unwrap());

        // Exactly one knowledge base and one course.
        let bases: Vec<_> = knowledge_service.list_bases().await.unwrap();
        assert_eq!(bases.len(), 1);
        assert_eq!(bases[0].name, TUTORIAL_KB_NAME);
        let files = knowledge_service
            .list_files(bases[0].knowledge_base_id.as_str())
            .await
            .unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.rel_path == "LEARNING_GUIDE.md"));

        // The course exists and points at the tutorial knowledge base.
        let courses = learning_service
            .list_courses(&UserId::new())
            .await
            .unwrap();
        assert_eq!(courses.len(), 1);
        assert_eq!(courses[0].title, TUTORIAL_COURSE_TITLE);
        assert_eq!(
            courses[0].source_kb_id.as_ref(),
            Some(&bases[0].knowledge_base_id)
        );
    }

    /// A lost version gate (crash before the gate was written, version bump,
    /// fresh data dir) must re-seed idempotently: reuse the existing base and
    /// skip the course import instead of duplicating either.
    #[tokio::test]
    async fn seed_with_lost_version_gate_does_not_duplicate_content() {
        let data_dir = tempfile::tempdir().unwrap();
        let (learning_service, knowledge_service) = seeded_service(data_dir.path()).await;

        assert!(learning_service
            .seed_tutorial_content(data_dir.path())
            .await
            .unwrap());
        // Simulate a gate that never made it to disk (crash mid-seed).
        std::fs::remove_file(data_dir.path().join(VERSION_DIR_NAME).join(VERSION_FILE_NAME))
            .unwrap();
        assert!(learning_service
            .seed_tutorial_content(data_dir.path())
            .await
            .unwrap());

        // Still exactly one base and one course; the guide files survive.
        let bases = knowledge_service.list_bases().await.unwrap();
        assert_eq!(bases.len(), 1);
        assert_eq!(bases[0].name, TUTORIAL_KB_NAME);
        let files = knowledge_service
            .list_files(bases[0].knowledge_base_id.as_str())
            .await
            .unwrap();
        assert_eq!(files.len(), 2);
        let courses = learning_service
            .list_courses(&UserId::new())
            .await
            .unwrap();
        assert_eq!(courses.len(), 1);
        assert_eq!(courses[0].title, TUTORIAL_COURSE_TITLE);
        assert_eq!(
            courses[0].source_kb_id.as_ref(),
            Some(&bases[0].knowledge_base_id)
        );
    }

    /// A stale preset course version (content upgraded in the asset) is
    /// replaced by the fresh import instead of piling up duplicates.
    #[tokio::test]
    async fn seed_replaces_stale_course_version() {
        let data_dir = tempfile::tempdir().unwrap();
        let (learning_service, knowledge_service) = seeded_service(data_dir.path()).await;

        assert!(learning_service
            .seed_tutorial_content(data_dir.path())
            .await
            .unwrap());
        // Downgrade the seeded course and drop the gate: simulates a user
        // data dir seeded from an older asset version.
        let pool = learning_service.pool_for_tests();
        sqlx::query(
            "UPDATE learning_courses SET version = 1 WHERE title = ?",
        )
        .bind(TUTORIAL_COURSE_TITLE)
        .execute(pool)
        .await
        .unwrap();
        std::fs::remove_file(data_dir.path().join(VERSION_DIR_NAME).join(VERSION_FILE_NAME))
            .unwrap();

        assert!(learning_service
            .seed_tutorial_content(data_dir.path())
            .await
            .unwrap());

        // Exactly one course, upgraded to the asset version.
        let courses = learning_service
            .list_courses(&UserId::new())
            .await
            .unwrap();
        assert_eq!(courses.len(), 1);
        assert_eq!(courses[0].version, tutorial_course_version());
        // The knowledge base is still the reused single one.
        let bases = knowledge_service.list_bases().await.unwrap();
        assert_eq!(bases.len(), 1);
    }
}
