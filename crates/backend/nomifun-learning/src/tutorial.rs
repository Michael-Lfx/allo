//! One-shot seeding of the tutorial knowledge base and the example learning
//! course, gated on a `.version` file under `{data_dir}/tutorial-learning/`.
//!
//! The knowledge base is registered through the injected `KnowledgeService`
//! and the example course is imported as a pre-written `CoursePack`, so the
//! seed needs no LLM call and works on a fresh install with no provider
//! configured. Once the version file is written the seed never runs again —
//! even if the user deletes the tutorial content — until a factory reset
//! clears `data_dir`.

use std::path::Path;

use nomifun_common::{AppError, KnowledgeBaseId};

use crate::models::{
    ActivityKind, ActivityPack, ConceptPack, CoursePack, LessonPack, ModulePack,
};
use crate::service::LearningService;

pub const TUTORIAL_KB_NAME: &str = "Flowy 使用指南";
pub const TUTORIAL_KB_DESCRIPTION: &str =
    "Flowy 自带教程知识库：学习模块使用指南与文档规范（七要素），也是示例课程的生成语料。";
pub const TUTORIAL_COURSE_TITLE: &str = "学习模块上手指南";

const VERSION_DIR_NAME: &str = "tutorial-learning";
const VERSION_FILE_NAME: &str = ".version";

impl LearningService {
    /// Seeds the tutorial knowledge base and example course once per binary
    /// version. Returns `true` when content was written, `false` when the
    /// version gate said "skip". Failures propagate to the caller, which
    /// treats them as non-fatal at startup.
    pub async fn seed_tutorial_content(&self, data_dir: &Path) -> Result<bool, AppError> {
        let version = env!("CARGO_PKG_VERSION");
        let gate_dir = data_dir.join(VERSION_DIR_NAME);
        let version_file = gate_dir.join(VERSION_FILE_NAME);
        if std::fs::read_to_string(&version_file).ok().as_deref() == Some(version) {
            return Ok(false);
        }

        let knowledge_service = self.injected_knowledge_service()?;

        let base = knowledge_service
            .create_base(TUTORIAL_KB_NAME, TUTORIAL_KB_DESCRIPTION, None, None)
            .await?;
        knowledge_service
            .write_file(
                base.knowledge_base_id.as_str(),
                "README.md",
                include_str!("../assets/tutorial/README.md"),
            )
            .await?;
        knowledge_service
            .write_file(
                base.knowledge_base_id.as_str(),
                "LEARNING_GUIDE.md",
                include_str!("../assets/tutorial/LEARNING_GUIDE.md"),
            )
            .await?;
        self.import_course(tutorial_course_pack(base.knowledge_base_id.clone()))
            .await?;

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
        std::fs::write(&staging, version).map_err(|error| {
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

/// The pre-written example course. Every lesson summary follows the atomic
/// seven-section document rule (描述/例子/迁移/其他/关键词/验证/推广) so the
/// course reads like a real study document and its objective activities can
/// feed both diagnostics and the review queue.
fn tutorial_course_pack(kb_id: KnowledgeBaseId) -> CoursePack {
    CoursePack {
        title: TUTORIAL_COURSE_TITLE.into(),
        description: "跟着这门示例课程走一遍学习模块：课程、课时、诊断、复习、问题管理。\
                      课程内容与本教程知识库文档一一对应，完成全部课时即可体验完整学习闭环。"
            .into(),
        domain: "product-guide".into(),
        source_kb_id: Some(kb_id),
        version: 1,
        concepts: vec![
            ConceptPack {
                key: "learning".into(),
                title: "学习模块".into(),
                description: "把知识库变成课程并驱动复习队列的学习闭环。".into(),
                prerequisites: Vec::new(),
            },
            ConceptPack {
                key: "course".into(),
                title: "课程".into(),
                description: "由模块、课时、概念组成的课程单元。".into(),
                prerequisites: vec!["learning".into()],
            },
            ConceptPack {
                key: "lesson".into(),
                title: "课时与活动".into(),
                description: "最小学习单元：原子文档与客观/反思活动。".into(),
                prerequisites: vec!["course".into()],
            },
            ConceptPack {
                key: "diagnostic".into(),
                title: "知识诊断".into(),
                description: "摸底测试并按掌握度推荐个性化路径。".into(),
                prerequisites: vec!["lesson".into()],
            },
            ConceptPack {
                key: "review".into(),
                title: "复习队列".into(),
                description: "按概念播种复习卡片，FSRS 排期到期复习。".into(),
                prerequisites: vec!["lesson".into()],
            },
            ConceptPack {
                key: "question".into(),
                title: "问题管理".into(),
                description: "追踪客观题与复习队列的关系，支持自定义问题。".into(),
                prerequisites: vec!["review".into()],
            },
        ],
        modules: vec![
            module_pack("认识学习模块", "学习模块的整体定位与课程结构。", vec![
                lesson_pack(
                    "学习模块是什么",
                    "learning",
                    r#"## 描述

学习模块把「知识库」变成「学习课程」，再驱动「复习队列」帮助记忆。一条完整链路是：准备知识库 → 生成或导入课程 → 加入课程 → 学习课时（做活动）→ 完成课时 → 复习队列到期复习 → 问题管理追踪掌握。

## 例子

打开学习页面，选中左侧知识库列表里的「Flowy 使用指南」，点击「生成课程」并等待生成；生成的课程出现在课程列表，点击「加入课程」即可开始学习。

## 迁移

任何格式良好的 markdown 知识库都可以生成课程：工作文档、语言学习笔记、考试复习资料都可以。变的是语料，不变的是「生成 → 学习 → 复习 → 追踪」这套闭环。

## 其他

生成课程需要已配置可用的模型；没有模型时仍可以直接导入课程包。课程生成质量取决于知识库文档的结构化程度。

## 关键词

学习模块, 课程, 知识库, 生成课程, 学习闭环

## 验证

判断题：学习模块可以从知识库自动生成课程。（正确）

## 推广

继续学习下一课「课程与概念」，了解课程由什么组成。"#,
                    vec![
                        tf_activity(
                            "学习模块可以从知识库自动生成课程。",
                            true,
                            "课程生成以知识库 markdown 文档为语料，这是学习模块的核心入口。",
                            vec!["learning"],
                        ),
                        single_choice_activity(
                            "学习链路的正确顺序是？",
                            vec![
                                "生成课程 → 加入课程 → 学习课时 → 复习",
                                "复习 → 生成课程 → 学习课时",
                                "加入课程 → 复习 → 生成课程",
                                "学习课时 → 生成课程 → 加入课程",
                            ],
                            0,
                            "先生成或导入课程，加入后学习课时，完成课时后进入复习队列。",
                            vec!["learning"],
                        ),
                        reflection_activity(
                            "你打算用学习模块学习什么内容？写下一个目标。",
                            vec!["learning"],
                        ),
                    ],
                ),
                lesson_pack(
                    "课程与概念",
                    "course",
                    r#"## 描述

课程由模块与课时组成，课时绑定概念；概念是课程的粒度单位，概念之间存在先修关系，用于推荐学习路径。

## 例子

本课程有三个模块：认识学习模块、学习与诊断、复习与问题管理；概念链为「学习模块 → 课程 → 课时与活动 → 知识诊断 → 复习队列 → 问题管理」，前一个概念是后一个的先修。

## 迁移

设计自己的课程时，先列出核心概念与先修顺序，再按概念组织课时，学习路径会更清晰；推荐算法会按先修关系引导学习顺序。

## 其他

概念可以跨课时共享：完成任一包含该概念的课时，就会为概念播种复习卡片；但某道题所在课时未完成时，这道题不会进入复习队列，问题管理中显示「未入队」。

## 关键词

课程, 模块, 课时, 概念, 先修关系, 学习路径

## 验证

单选题：概念之间通过什么关系组织学习路径？A. 先修关系 B. 包含关系 C. 随机关系 D. 无关系（答案：A）

## 推广

下一课「课时与活动」介绍课时的原子文档与活动类型。"#,
                    vec![
                        single_choice_activity(
                            "概念之间通过什么关系组织学习路径？",
                            vec!["先修关系", "包含关系", "随机关系", "无关系"],
                            0,
                            "概念的先修关系决定推荐顺序。",
                            vec!["course"],
                        ),
                        tf_activity(
                            "概念之间存在先修关系，用于推荐学习路径。",
                            true,
                            "推荐算法按先修链引导学习顺序。",
                            vec!["course"],
                        ),
                        reflection_activity(
                            "为你想学的主题列出 3 个概念和它们的先修顺序。",
                            vec!["course"],
                        ),
                    ],
                ),
            ]),
            module_pack("学习与诊断", "课时学习与掌握度摸底。", vec![
                lesson_pack(
                    "课时与活动",
                    "lesson",
                    r#"## 描述

课时是学习的最小单元：一篇原子学习文档（summary，按七要素结构书写）加上若干活动。活动有三种类型：单选题、判断题、反思题。

## 例子

在课程详情点击「开始课时」，先阅读课时的原子文档；然后逐题作答：单选题和判断题会得到对错反馈，反思题记录你的思考；最后点击「完成课时」。

## 迁移

把课时理解为一个自包含的知识单元：一篇文档 + 若干自测，这个结构适合任何主题的拆解与组织。

## 其他

活动绑定概念，决定复习归属；反思题不进入复习队列——队列只服务客观题（单选题与判断题），诊断也只抽取客观题。

## 关键词

课时, 活动, 单选题, 判断题, 反思题, 原子文档

## 验证

单选题：下面哪种不是课时活动的类型？A. 单选题 B. 判断题 C. 反思题 D. 拖拽题（答案：D）

## 推广

完成课时后概念进入复习队列，同时可以先用「知识诊断」摸底（下一课）。"#,
                    vec![
                        single_choice_activity(
                            "下面哪种不是课时活动的类型？",
                            vec!["单选题", "判断题", "反思题", "拖拽题"],
                            3,
                            "课时活动只有单选、判断、反思三种。",
                            vec!["lesson"],
                        ),
                        tf_activity(
                            "课时的原子文档包含描述、例子、迁移等七个要素。",
                            true,
                            "七要素即：描述、例子、迁移、其他、关键词、验证、推广。",
                            vec!["lesson"],
                        ),
                        reflection_activity(
                            "回顾上一课的原子文档，七要素中哪一节对你最有帮助？",
                            vec!["lesson"],
                        ),
                    ],
                ),
                lesson_pack(
                    "知识诊断",
                    "diagnostic",
                    r#"## 描述

知识诊断是加入课程后可以做的摸底测试：抽取课程中的客观题作答，按掌握度评估结果推荐个性化学习路径。

## 例子

加入课程后点击「先做知识诊断」，逐题作答；完成后页面按掌握度与先修顺序给出「建议下一步」，例如「前往课时」或「直接进入复习」。

## 迁移

适合新加入课程时快速定位薄弱概念：把诊断当作学习的起点而非考试，之后的学习路径会更有针对性。

## 其他

诊断抽取课程内的客观题；诊断与课时答题都只更新掌握度，不会创建复习卡片——复习卡片只在课时完成时创建。

## 关键词

知识诊断, 掌握度, 个性化路径, 推荐, 摸底

## 验证

判断题：知识诊断会创建复习卡片。（错误——复习卡片只在课时完成时创建。）

## 推广

诊断之后进入复习环节：下一课「复习队列与评分」。"#,
                    vec![
                        tf_activity(
                            "知识诊断会创建复习卡片。",
                            false,
                            "复习卡片只在课时完成时创建，诊断只更新掌握度。",
                            vec!["diagnostic"],
                        ),
                        single_choice_activity(
                            "知识诊断的作用是？",
                            vec!["摸底并推荐个性化路径", "直接删除课程", "生成新知识库", "修改课程内容"],
                            0,
                            "诊断按掌握度评估并推荐下一步学习内容。",
                            vec!["diagnostic"],
                        ),
                        reflection_activity(
                            "完成诊断后，你希望推荐算法优先安排什么内容？",
                            vec!["diagnostic"],
                        ),
                    ],
                ),
            ]),
            module_pack("复习与问题管理", "巩固记忆与追踪全部题目。", vec![
                lesson_pack(
                    "复习队列与评分",
                    "review",
                    r#"## 描述

课时完成后，该课时涉及的概念自动创建复习卡片（按概念粒度），使用 FSRS 算法排期；到期卡片进入「今日复习」，复习时评分会重排下次到期时间。

## 例子

完成一个课时后回到学习页首页，「今日复习」出现到期卡片；点击「开始复习」逐张作答并评分（忘记 / 困难 / 良好 / 轻松），评分后下次复习时间按 FSRS 重排。

## 迁移

把复习队列当作日常记忆维护：每天花几分钟清掉到期卡片，长期记忆由算法自动安排，适合持续积累型学习。

## 其他

「今日复习」显示的卡片数量是本次会话加载上限（默认 30，可在学习设置中调整），不是到期总数；复习项按概念创建，概念绑定多道题时，队列按复习次数轮换出题。

## 关键词

复习队列, FSRS, 评分, 到期, 今日复习, 记忆

## 验证

判断题：复习卡片在课时完成时创建。（正确）
单选题：复习评分后会发生什么？A. 下次复习时间按 FSRS 重排 B. 卡片被删除 C. 课时被重置 D. 课程被删除（答案：A）

## 推广

用问题管理追踪每道题与队列的关系（下一课）。"#,
                    vec![
                        tf_activity(
                            "复习卡片在课时完成时创建。",
                            true,
                            "完成课时后概念自动播种复习卡片。",
                            vec!["review"],
                        ),
                        single_choice_activity(
                            "复习评分后会发生什么？",
                            vec!["下次复习时间按 FSRS 重排", "卡片被删除", "课时被重置", "课程被删除"],
                            0,
                            "评分会更新 FSRS 状态并重排下次到期时间。",
                            vec!["review"],
                        ),
                        reflection_activity(
                            "你更倾向于每天固定时间复习，还是攒到一批再复习？",
                            vec!["review"],
                        ),
                    ],
                ),
                lesson_pack(
                    "问题管理与自定义问题",
                    "question",
                    r#"## 描述

问题管理集中查看所有客观题与复习队列的关系：未入队、待首复习、已到期、已排期，并支持添加自定义问题。点击任意一行可查看队列状态与复习指标。

## 例子

在「问题管理」页签筛选「已到期」，这些题对应今日复习队列；点击一行查看详情，复习队列区块显示「在队列中 / 未入队 / 自定义排期」与下次到期时间；用「添加问题」创建的自定义问题立即进入复习队列。

## 迁移

把问题管理当作学习的仪表盘：考前用它筛选薄弱概念（遗忘次数高的题），复习时优先处理「已到期」列表。

## 其他

未入队 = 该题所在课时尚未完成；概念跨课时共享时，未学课时的题不会被标成已到期（与复习队列口径一致）；自定义问题使用自己的排期，不受课时完成状态影响。

## 关键词

问题管理, 未入队, 待首复习, 已到期, 已排期, 自定义问题

## 验证

判断题：某题所在课时未完成时，它在问题管理中显示「已到期」。（错误——显示「未入队」）
单选题：自定义问题创建后？A. 立即进入复习队列 B. 需要先完成课时 C. 永远不会复习 D. 需要审核（答案：A）

## 推广

把七要素规范用于自己的知识库写作，生成质量更高的课程；后续模块教程将介绍技能、浏览器与自动化能力的用法。"#,
                    vec![
                        tf_activity(
                            "某题所在课时未完成时，它在问题管理中显示「已到期」。",
                            false,
                            "未完成课时的题显示「未入队」，与复习队列口径一致。",
                            vec!["question"],
                        ),
                        single_choice_activity(
                            "自定义问题创建后会怎样？",
                            vec!["立即进入复习队列", "需要先完成课时", "永远不会复习", "需要审核"],
                            0,
                            "自定义问题自带排期，创建即可复习。",
                            vec!["question"],
                        ),
                        reflection_activity(
                            "你希望用问题管理追踪哪些学习指标？",
                            vec!["question"],
                        ),
                    ],
                ),
            ]),
        ],
    }
}

fn module_pack(title: &str, description: &str, lessons: Vec<LessonPack>) -> ModulePack {
    ModulePack {
        title: title.into(),
        description: description.into(),
        lessons,
    }
}

fn lesson_pack(title: &str, concept: &str, summary: &str, activities: Vec<ActivityPack>) -> LessonPack {
    LessonPack {
        title: title.into(),
        summary: summary.into(),
        estimated_minutes: 10,
        source: None,
        concepts: vec![concept.into()],
        activities,
    }
}

fn tf_activity(prompt: &str, answer: bool, explanation: &str, concepts: Vec<&str>) -> ActivityPack {
    ActivityPack {
        kind: ActivityKind::TrueFalse,
        prompt: prompt.into(),
        options: Vec::new(),
        answer: serde_json::Value::Bool(answer),
        explanation: explanation.into(),
        concepts: concepts.into_iter().map(str::to_owned).collect(),
    }
}

fn single_choice_activity(
    prompt: &str,
    options: Vec<&str>,
    answer_index: usize,
    explanation: &str,
    concepts: Vec<&str>,
) -> ActivityPack {
    ActivityPack {
        kind: ActivityKind::SingleChoice,
        prompt: prompt.into(),
        options: options.iter().map(|option| (*option).to_owned()).collect(),
        answer: serde_json::Value::String(options[answer_index].to_owned()),
        explanation: explanation.into(),
        concepts: concepts.into_iter().map(str::to_owned).collect(),
    }
}

fn reflection_activity(prompt: &str, concepts: Vec<&str>) -> ActivityPack {
    ActivityPack {
        kind: ActivityKind::Reflection,
        prompt: prompt.into(),
        options: Vec::new(),
        answer: serde_json::Value::Null,
        explanation: String::new(),
        concepts: concepts.into_iter().map(str::to_owned).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_api_types::WebSocketMessage;
    use nomifun_common::UserId;
    use nomifun_knowledge::KnowledgeService;
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

    const SEVEN_SECTIONS: [&str; 7] = [
        "## 描述",
        "## 例子",
        "## 迁移",
        "## 其他",
        "## 关键词",
        "## 验证",
        "## 推广",
    ];

    #[test]
    fn tutorial_lessons_cover_all_seven_sections_in_order() {
        let pack = tutorial_course_pack(KnowledgeBaseId::new());
        assert_eq!(pack.modules.len(), 3);
        let total_lessons: usize = pack.modules.iter().map(|m| m.lessons.len()).sum();
        assert_eq!(total_lessons, 6);
        for module in &pack.modules {
            for lesson in &module.lessons {
                let mut position = 0;
                for section in SEVEN_SECTIONS {
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
                // Every lesson carries at least two objective activities so it
                // can feed diagnostics and the review queue.
                let objective = lesson
                    .activities
                    .iter()
                    .filter(|a| a.kind != ActivityKind::Reflection)
                    .count();
                assert!(objective >= 2, "lesson {} lacks objective activities", lesson.title);
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
}
