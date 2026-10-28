//! `nomicore learning-smoke`（wayfinder 票 s1）：学习结构生成管线的无头真机
//! 冒烟入口。与应用同接线（DB pool → `LearningService::new` +
//! `set_generation_dependencies`；provider 解析复用 `resolve_provider_config`
//! 现链，不建第二套）；缺省**从零临时 SQLite**（跑 nomifun-db 迁移到临时
//! 文件，learnhub smoke-loop「新建干净临时目录」纪律），`--db` 可指开发库
//! （须已是 v3 库；本命令不做 v3 门禁——provider 凭据就在库里，临时库没有
//! 可用 provider 时命令会如实失败）。
//!
//! - `structure`：双产物生成 → gd.* 契约门 + 门口对账 → `structure_json`
//!   落列；机械判据 = 落列后重读快照、本地重验双契约与对账（失败非零退出）。
//!   运行必挂 [`nomifun_ai_agent::ArchivingCompleter`]（观测存档
//!   `<data-dir>/observation/learning-smoke/<run>/`）。
//! - `json`：json_object 能力读数（合法 JSON 率 / 空内容 / 怪癖分类）；
//!   **读数面非门禁**——唯一硬判据 = 全灭 0/n；探测调用直调内层 completer，
//!   不混入管线档案（learnhub smoke-json 纪律）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};

use nomifun_ai_agent::archiving_completer::ArchivingCompleter;
use nomifun_ai_agent::LiveLearningCompleter;
use nomifun_common::{ProviderId, UserId};
use nomifun_db::sqlx;
use nomifun_db::{IUserRepository, SqliteProviderModelRepository, SqliteProviderRepository, SqliteUserRepository};
use nomifun_learning::{
    CourseGenerationMode, CourseKind, GenerateCourseRequest, GenerateLessonRequest,
    LearningCompleter, LearningService,
};

use crate::cli::{Cli, LearningSmokeCommand};

pub async fn run_learning_smoke(cli: &Cli, command: LearningSmokeCommand) -> Result<ExitCode> {
    // 冒烟是唯一的无头真机面：门级剪枝/归一/落库失败只走 tracing。不初始化
    // 订阅者，CLI 进程里这些留痕全部丢失（审计纪律要求可见）。
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::io::stderr)
        .try_init();
    match command {
        LearningSmokeCommand::Xp { db } => run_xp(cli, db).await,
        LearningSmokeCommand::Quiz { db, provider, model } => {
            run_quiz(cli, db, provider, model).await
        }
        LearningSmokeCommand::Content {
            db,
            provider,
            model,
        } => run_content(cli, db, provider, model).await,
        LearningSmokeCommand::Course {
            dir,
            goal,
            name,
            lessons,
            triage_only,
            plan_only,
            report_json,
            db,
            provider,
            model,
        } => {
            run_course(
                cli,
                dir,
                goal,
                name,
                lessons,
                triage_only,
                plan_only,
                report_json,
                db,
                provider,
                model,
            )
            .await
        }
        LearningSmokeCommand::Mount {
            db,
            provider,
            model,
        } => run_mount(cli, db, provider, model).await,
        LearningSmokeCommand::Repair {
            db,
            provider,
            model,
        } => run_repair(cli, db, provider, model).await,
        LearningSmokeCommand::Maintenance {
            db,
            provider,
            model,
        } => run_maintenance(cli, db, provider, model).await,
        LearningSmokeCommand::Basis {
            db,
            provider,
            model,
        } => run_basis(cli, db, provider, model).await,
        LearningSmokeCommand::Structure {
            goal,
            proficiency,
            basis,
            db,
            provider,
            model,
        } => {
            run_structure(cli, goal, proficiency, basis, db, provider, model).await
        }
        LearningSmokeCommand::Json {
            db,
            provider,
            model,
            calls,
        } => run_json(cli, db, provider, model, calls).await,
        LearningSmokeCommand::Grow {
            goal,
            batches,
            db,
            provider,
            model,
        } => run_grow(cli, goal, batches, db, provider, model, false).await,
        LearningSmokeCommand::Loop {
            goal,
            rounds,
            db,
            provider,
            model,
        } => run_grow(cli, goal, rounds, db, provider, model, true).await,
    }
}

struct SmokeWiring {
    service: Arc<LearningService>,
    /// 传统课程冒烟档要先把本地 md 目录写成知识库，再交给生成管线采样。
    knowledge: Arc<nomifun_knowledge::KnowledgeService>,
    pool: nomifun_db::sqlx::SqlitePool,
    owner: UserId,
    /// 保持临时目录存活到命令结束（缺省从零临时库时非空）。
    _temp: Option<tempfile::TempDir>,
}

/// 与应用相同的组合根接线：开库（临时或 --db）→ provider 仓库 + 加密键 →
/// LiveLearningCompleter（可包 ArchivingCompleter）→ LearningService。
/// 返回 (wiring, 管线 completer 即 Archiving 挂载结果, 内层 live completer)。
async fn wire(
    cli: &Cli,
    db: Option<&Path>,
    provider: Option<&str>,
    model: Option<&str>,
    archive: bool,
) -> Result<(
    SmokeWiring,
    Option<Arc<ArchivingCompleter>>,
    Arc<dyn LearningCompleter>,
)> {
    if provider.is_some() != model.is_some() {
        anyhow::bail!("--provider and --model must be provided together");
    }
    let (pool, _temp) = match db {
        Some(path) => {
            let database = nomifun_db::init_database(path).await?;
            (database.pool().clone(), None)
        }
        None => {
            let temp = tempfile::tempdir()?;
            let database =
                nomifun_db::init_database(temp.path().join("smoke.sqlite").as_path()).await?;
            (database.pool().clone(), Some(temp))
        }
    };
    let owner_id = owner_id_string(&pool).await;

    // 加密键：与应用同链（JWT_SECRET env → 库内安装者 jwt_secret → 固定回退）。
    // 回退分支只发生在无凭据可解的临时库上——键不匹配任何存量密文，无碍。
    let user_repo: Arc<dyn IUserRepository> = Arc::new(SqliteUserRepository::new(pool.clone()));
    let secret = match std::env::var("JWT_SECRET").ok().filter(|secret| !secret.is_empty()) {
        Some(secret) => secret,
        None => match user_repo.get_system_user().await? {
            Some(owner) if !owner.jwt_secret.as_deref().unwrap_or_default().is_empty() => {
                owner.jwt_secret.unwrap()
            }
            _ => "learning-smoke-no-credentials".to_owned(),
        },
    };
    let encryption_key =
        crate::config::load_or_create_data_encryption_key(&cli.data_dir, &secret)?;

    let provider_repo = Arc::new(SqliteProviderRepository::new(pool.clone()));
    let provider_model_repo = Arc::new(SqliteProviderModelRepository::new(pool.clone()));
    let live: Arc<dyn LearningCompleter> = Arc::new(LiveLearningCompleter {
        provider_repo: provider_repo.clone() as Arc<dyn nomifun_db::IProviderRepository>,
        provider_model_repo: provider_model_repo.clone(),
        encryption_key,
        workspace: cli.data_dir.clone(),
    });
    let archiving = if archive {
        let obs_root = cli.data_dir.join("observation").join("learning-smoke");
        let archiving = Arc::new(ArchivingCompleter::new(live.clone(), &obs_root)?);
        println!("observation archive: {}", archiving.run_dir().display());
        Some(archiving.clone())
    } else {
        None
    };
    let pipeline: Arc<dyn LearningCompleter> = archiving
        .clone()
        .map(|archiving| archiving as Arc<dyn LearningCompleter>)
        .unwrap_or_else(|| live.clone());

    let knowledge_service = Arc::new(nomifun_knowledge::KnowledgeService::new(
        Arc::new(nomifun_db::SqliteKnowledgeRepository::new(pool.clone())),
        cli.data_dir.clone().as_path(),
        nomifun_knowledge::KnowledgeEventEmitter::new(
            Arc::new(NoopEventSink),
            Arc::from(owner_id.as_str()),
        ),
    ));
    let service = Arc::new(LearningService::new(pool.clone()));
    service.set_generation_dependencies(knowledge_service.clone(), pipeline);
    // 铸名门外部通行名证据（m11 双臂开关面）：接线 DDG 无 key 搜索提供方；
    // `NOMI_LEARNING_MINT_RESEARCH=off` 臂在机制内零搜索（off = 现行为）。
    // 接线失败 = 零搜索（fail-open 同律），只留一行提示。
    match flowy_web::provider::DuckDuckGoSearchProvider::try_new() {
        Ok(provider) => {
            service.set_mint_search_provider(std::sync::Arc::new(provider));
        }
        Err(error) => {
            eprintln!("[mint-research] search provider unavailable, zero searches: {error}");
        }
    }
    // 传统课程（知识库 → 大纲）走与应用同款的双循环大纲引擎；此前的
    // smoke 档只覆盖学习图/课时内容，这条管线一直没有无头真机入口。
    service.set_course_outline_engine(Arc::new(
        nomifun_ai_agent::LiveCourseOutlineAgentEngine {
            service: service.clone(),
            deps: nomifun_ai_agent::OneShotDeps {
                provider_repo: provider_repo.clone() as Arc<dyn nomifun_db::IProviderRepository>,
                provider_model_repo: provider_model_repo.clone(),
                encryption_key,
                workspace: cli.data_dir.clone(),
            },
        },
    ));
    // 课时内容 agent 引擎（j07 content 冒烟需要）：与应用同款接线——引擎
    // 自行解析 provider（OneShotDeps），不经管线 completer。首轮观测钩子把
    // system/user 追加进本次运行的 calls.jsonl（k03/k05 双臂读数面）。
    let lesson_observer = archiving.as_ref().map(|archiving| {
        let path = archiving.run_dir().join("calls.jsonl");
        Arc::new(move |record: serde_json::Value| {
            use std::io::Write;
            let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            else {
                return;
            };
            let _ = writeln!(file, "{record}");
        }) as Arc<dyn Fn(serde_json::Value) + Send + Sync>
    });
    service.set_lesson_engine(Arc::new(
        nomifun_ai_agent::LiveLessonContentAgentEngine {
            service: service.clone(),
            deps: nomifun_ai_agent::OneShotDeps {
                provider_repo: provider_repo.clone() as Arc<dyn nomifun_db::IProviderRepository>,
                provider_model_repo,
                encryption_key,
                workspace: cli.data_dir.clone(),
            },
            round_logs: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            prompt_observer: lesson_observer,
        },
    ));
    // 长任务观测旁路（与应用同形）：begin/finish JSONL 落观测目录。
    {
        let observation_dir = cli.data_dir.join("observation").join("learning-smoke");
        service.set_generation_task_observer(Arc::new(move |line: &str| {
            use std::io::Write;
            let _ = std::fs::create_dir_all(&observation_dir);
            let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(observation_dir.join("learning-tasks.jsonl"))
            else {
                return;
            };
            let _ = writeln!(file, "{line}");
        }));
    }

    Ok((
        SmokeWiring {
            service,
            knowledge: knowledge_service,
            pool,
            owner: UserId::parse(&owner_id)?,
            _temp,
        },
        archiving,
        live,
    ))
}

async fn owner_id_string(pool: &nomifun_db::sqlx::SqlitePool) -> String {
    nomifun_db::installation_owner_id(pool).await.unwrap_or_else(|_| {
        // init_database 保证安装者行存在；此兜底仅为类型完整。
        String::new()
    })
}

struct NoopEventSink;

impl nomifun_realtime::UserEventSink for NoopEventSink {
    fn send_to_user(
        &self,
        _user_id: &str,
        _event: nomifun_api_types::WebSocketMessage<serde_json::Value>,
    ) {
    }
}

/// 递归收集目录下的 Markdown 文件，返回 `(库内相对路径, 正文)`，按路径
/// 字典序排序——与采样器的结构信号同一口径（课程式库的 `01-`…`09-`
/// 前缀就是阅读顺序）。
fn collect_markdown_files(root: &Path) -> Result<Vec<(String, String)>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
        let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)?
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                walk(root, &path, out)?;
            } else if file_type.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            {
                let relative = path
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/");
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                out.push((relative, content));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

/// 多库采样路径带 `{库名}/rel` 命名空间；单库是裸路径。两种形状都归一到
/// 库内相对路径，才能和上传清单比对。
fn strip_kb_namespace<'a>(path: &'a str, kb_name: &str) -> &'a str {
    path.strip_prefix(kb_name)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
}

fn chapter_key(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn chapter_number(path: &str) -> Option<u32> {
    let head = chapter_key(path).split('-').next()?;
    // 两种常见的教材命名：`01-标题.md` 与 `ch01-01-标题.md`。
    let digits = head.strip_prefix("ch").unwrap_or(head);
    digits.parse().ok()
}

/// 有 N 个课时要抽看正文时，按课程顺序均匀取点；只要一节就取正中，
/// 避免总看“前言/安装”这类入口课而看不到后半本教材的接地情况。
fn lesson_picks(total: usize, wanted: usize) -> Vec<usize> {
    if total == 0 || wanted == 0 {
        return Vec::new();
    }
    if wanted >= total {
        return (0..total).collect();
    }
    if wanted == 1 {
        return vec![total / 2];
    }
    (0..wanted)
        .map(|index| index * (total - 1) / (wanted - 1))
        .collect()
}

/// `learning-smoke course`：传统课程（知识库 → 大纲）真机冒烟。先用本地
/// md 目录建一个受管知识库，再走与应用同款的 `generate_course`（目标感知
/// 采样 + 双循环大纲 + 导入），最后按需生成若干课时正文。机械判据只锁
/// “管线走通 + 来源可解析”；章节覆盖率、顺序与采样披露作为负优化读数
/// 打印——完整教材被截断/乱序是产品问题，不应被冒烟入口藏起来。
#[allow(clippy::too_many_arguments)]
async fn run_course(
    cli: &Cli,
    dir: PathBuf,
    goal: String,
    name: Option<String>,
    lessons: usize,
    triage_only: bool,
    plan_only: bool,
    report_json: Option<PathBuf>,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
) -> Result<ExitCode> {
    if goal.trim().is_empty() {
        eprintln!("--goal must not be empty");
        return Ok(ExitCode::from(2));
    }
    if !dir.is_dir() {
        eprintln!("--dir must be an existing directory: {}", dir.display());
        return Ok(ExitCode::from(2));
    }
    let files = collect_markdown_files(&dir)?;
    if files.is_empty() {
        eprintln!("--dir contains no markdown files: {}", dir.display());
        return Ok(ExitCode::from(2));
    }
    let total_bytes: usize = files.iter().map(|(_, content)| content.len()).sum();

    let (wiring, _archiving, _live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), true).await?;
    let base_name = name.unwrap_or_else(|| {
        dir.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "learning-smoke-kb".to_owned())
    });
    let base = wiring
        .knowledge
        .create_base(&base_name, "learning-smoke course KB", None, None)
        .await?;
    let kb_name = base.name.clone();
    let kb_id = base.knowledge_base_id.clone();
    let written = wiring
        .knowledge
        .upload_files_batch(kb_id.as_str(), &files)
        .await?;
    println!(
        "[1/3] knowledge base {} ({}): {} files, {} bytes written",
        kb_id.as_str(),
        kb_name,
        written,
        total_bytes
    );

    // t5 变体：--triage-only / --plan-only 不建课,直接跑清点(分诊[/结构
    // 计划])并打印读数;报告(可选)按档位落 triage[/plan]。
    if triage_only || plan_only {
        let completer: Arc<dyn LearningCompleter> = _archiving
            .clone()
            .map(|archiving| archiving as Arc<dyn LearningCompleter>)
            .unwrap_or_else(|| _live.clone());
        return run_course_stage_reports(
            &wiring,
            completer,
            &kb_id,
            goal.trim(),
            plan_only,
            report_json.as_deref(),
        )
        .await;
    }

    let uploaded_paths: BTreeSet<String> =
        files.iter().map(|(path, _)| path.clone()).collect();
    let provider_id = provider
        .as_deref()
        .map(ProviderId::parse)
        .transpose()
        .map_err(|error| anyhow::anyhow!("invalid provider id: {error}"))?;
    let request = GenerateCourseRequest {
        knowledge_base_id: None,
        knowledge_base_ids: vec![kb_id],
        description: None,
        domain: None,
        provider_id: provider_id.clone(),
        model: model.clone(),
        mode: CourseGenerationMode::OnDemand,
        course_kind: CourseKind::Traditional,
        proficiency_statement: None,
        learner_basis: None,
        teaching_style: None,
        goal: Some(goal.trim().to_owned()),
    };
    println!("[2/3] goal-aware sampling + outline generation …");
    let detail = match wiring.service.generate_course(&wiring.owner, request).await {
        Ok(detail) => detail,
        Err(error) => {
            eprintln!("course generation FAILED: {error}");
            return Ok(ExitCode::from(2));
        }
    };
    println!(
        "[3/3] course {} — {} ({} modules, {} lessons)",
        detail.course.id.as_str(),
        detail.course.title,
        detail.modules.len(),
        detail.course.total_lessons
    );
    // t5 报告（可选）：full 档落的是库里的快照信封（triage/plan/coverage
    // 四件套原样），便于与 --triage-only/--plan-only 档对账；回退/描述流
    // 建的课没有信封——如实写 null,不伪造。
    if let Some(report_path) = report_json.as_deref() {
        let snapshot_json: Option<String> = nomifun_db::sqlx::query_scalar(
            "SELECT course_source_plan_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(detail.course.id.as_str())
        .fetch_optional(&wiring.pool)
        .await
        .map_err(|error| anyhow::anyhow!("failed to read course source plan: {error}"))?
        .flatten();
        let report = match snapshot_json.as_deref().map(serde_json::from_str::<serde_json::Value>) {
            Some(Ok(envelope)) => envelope,
            Some(Err(error)) => {
                return Err(anyhow::anyhow!("course source plan snapshot unreadable: {error}"))
            }
            None => serde_json::json!({
                "course_id": detail.course.id.as_str(),
                "source_plan": serde_json::Value::Null,
                "note": "course built without the plan pipeline (fallback or description flow)",
            }),
        };
        write_course_report(Some(report_path), &report)?;
        println!("report: {}", report_path.display());
    }
    match detail.course.sampling.as_ref() {
        Some(sampling) => {
            println!(
                "sampling: total={} sampled={} coherence={} low_score={} goal_fallback={}",
                sampling.total_files,
                sampling.sampled_files,
                sampling.coherence_files,
                sampling.low_score_files,
                sampling.goal_fallback
            );
            if !sampling.low_score_examples.is_empty() {
                println!(
                    "  low-score examples: {}",
                    sampling.low_score_examples.join(", ")
                );
            }
        }
        None => println!("sampling: <none>"),
    }

    let mut all_lessons = Vec::new();
    let mut referenced_chapters = BTreeSet::new();
    let mut unresolved_sources = Vec::new();
    let mut lessons_without_source = Vec::new();
    let mut module_centers: Vec<(usize, u32, String)> = Vec::new();
    for (module_index, module) in detail.modules.iter().enumerate() {
        println!("\n## module: {}", module.title);
        let mut module_chapters: Vec<u32> = Vec::new();
        for lesson in &module.lessons {
            let sources: Vec<&nomifun_learning::SourceSpan> = if lesson.sources.is_empty() {
                lesson.source.iter().collect()
            } else {
                lesson.sources.iter().collect()
            };
            let mut relative_paths = Vec::new();
            for source in &sources {
                let relative = strip_kb_namespace(&source.path, &kb_name);
                if !uploaded_paths.contains(relative) {
                    unresolved_sources.push(format!("{} -> {}", lesson.title, source.path));
                }
                let key = chapter_key(relative).to_owned();
                referenced_chapters.insert(key);
                if let Some(number) = chapter_number(relative) {
                    // 模块的结构位置用“被引章节的中位数”：低章多半是前置
                    // 引用（讲 rehash 回引第 03 章），高章多半是对照
                    // （讲 HashMap 顺带提 TreeMap），两者都不该把模块
                    // 整体抬到错误的位置。
                    module_chapters.push(number);
                }
                relative_paths.push(relative.to_owned());
            }
            if sources.is_empty() {
                lessons_without_source.push(lesson.title.clone());
            }
            println!(
                "- [{}] {}  <- {}",
                lesson.id.as_str(),
                lesson.title,
                if relative_paths.is_empty() {
                    "<no source>".to_owned()
                } else {
                    relative_paths.join(", ")
                }
            );
            all_lessons.push(lesson);
        }
        if !module_chapters.is_empty() {
            module_chapters.sort_unstable();
            let median = module_chapters[module_chapters.len() / 2];
            module_centers.push((module_index + 1, median, module.title.clone()));
        }
    }

    let uploaded_chapters: BTreeSet<String> = uploaded_paths
        .iter()
        .map(|path| chapter_key(path).to_owned())
        .collect();
    let missing_chapters: Vec<&String> = uploaded_chapters
        .difference(&referenced_chapters)
        .collect();
    // 前言/目录不是教学内容；把“正文章覆盖”单独列一行，避免 18/19 的
    // 读数被误读成正文缺一章。
    let content_chapters: BTreeSet<String> = uploaded_chapters
        .iter()
        .filter(|chapter| chapter_number(chapter).is_some_and(|number| number >= 2))
        .cloned()
        .collect();
    let missing_content: Vec<&String> = content_chapters
        .difference(&referenced_chapters)
        .collect();
    let mut order_violations = Vec::new();
    let mut highest_seen = 0u32;
    for (position, number, title) in &module_centers {
        if *number < highest_seen {
            order_violations.push(format!(
                "module {position} ({title}) centers on chapter {number:02} after a module centered on chapter {highest_seen:02}"
            ));
        }
        highest_seen = highest_seen.max(*number);
    }
    println!("\n--- negative-optimization readings ---");
    println!(
        "file coverage: {}/{} referenced (missing: {})",
        uploaded_chapters.len() - missing_chapters.len(),
        uploaded_chapters.len(),
        if missing_chapters.is_empty() {
            "<none>".to_owned()
        } else {
            missing_chapters
                .iter()
                .map(|chapter| chapter.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        "content coverage (chapters 02+): {}/{} referenced (missing: {})",
        content_chapters.len() - missing_content.len(),
        content_chapters.len(),
        if missing_content.is_empty() {
            "<none>".to_owned()
        } else {
            missing_content
                .iter()
                .map(|chapter| chapter.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        "module order violations: {}",
        if order_violations.is_empty() {
            "<none>".to_owned()
        } else {
            order_violations.join("; ")
        }
    );
    println!(
        "lessons without a source: {}",
        if lessons_without_source.is_empty() {
            "<none>".to_owned()
        } else {
            lessons_without_source.join(", ")
        }
    );
    println!(
        "unresolved source paths: {}",
        if unresolved_sources.is_empty() {
            "<none>".to_owned()
        } else {
            unresolved_sources.join("; ")
        }
    );

    if detail.modules.is_empty() || all_lessons.is_empty() {
        eprintln!("course has no modules/lessons");
        return Ok(ExitCode::from(2));
    }
    if !unresolved_sources.is_empty() {
        eprintln!("course references source paths that do not exist in the uploaded base");
        return Ok(ExitCode::from(2));
    }
    if !lessons_without_source.is_empty() {
        eprintln!("kb-flow course has lessons without a source");
        return Ok(ExitCode::from(2));
    }

    let picks = lesson_picks(all_lessons.len(), lessons);
    let mut body_failures = 0usize;
    for (ordinal, index) in picks.iter().enumerate() {
        let lesson = all_lessons[*index];
        println!(
            "\n### lesson body {}/{}: [{}] {}",
            ordinal + 1,
            picks.len(),
            lesson.id.as_str(),
            lesson.title
        );
        let request = GenerateLessonRequest {
            provider_id: provider_id.clone(),
            model: model.clone(),
            feedback: None,
        };
        match wiring
            .service
            .generate_lesson_content(&wiring.owner, &lesson.id, &request)
            .await
        {
            Ok(view) => {
                let body_chars: usize = view
                    .sections
                    .iter()
                    .map(|section| section.body_md.chars().count())
                    .sum();
                println!(
                    "generated={} sections={} body_chars={} activities={}",
                    view.generated,
                    view.sections.len(),
                    body_chars,
                    view.activities.len()
                );
                for section in &view.sections {
                    println!(
                        "  - {} [{}] {} chars{}",
                        section.title,
                        section.section_key,
                        section.body_md.chars().count(),
                        if section.degraded { " degraded" } else { "" }
                    );
                }
                let preview: String = view
                    .sections
                    .iter()
                    .flat_map(|section| section.body_md.chars())
                    .take(600)
                    .collect();
                println!("  preview: {}", preview.replace('\n', " "));
            }
            Err(error) => {
                body_failures += 1;
                eprintln!("lesson body generation FAILED for {}: {error}", lesson.title);
            }
        }
    }
    if body_failures > 0 {
        return Ok(ExitCode::from(2));
    }

    println!("\nEXIT=0");
    Ok(ExitCode::SUCCESS)
}

// ── t5 冒烟变体：--triage-only / --plan-only / --report-json ─────────────

/// t5 冒烟变体：清点+分诊（--triage-only）或再跑结构计划（--plan-only），
/// 打印 counts/排除理由/uncertain/chapters/coverage 台账/gaps。读数面非
/// 门禁：triage 档有 warning 也 EXIT=0（票面）；plan 档计划不可用 EXIT=2
/// （该档的目的就是看计划）。
async fn run_course_stage_reports(
    wiring: &SmokeWiring,
    completer: Arc<dyn LearningCompleter>,
    kb_id: &str,
    goal: &str,
    plan_only: bool,
    report_json: Option<&Path>,
) -> Result<ExitCode> {
    let kb_id: nomifun_common::KnowledgeBaseId = kb_id
        .parse()
        .map_err(|error| anyhow::anyhow!("invalid kb id: {error}"))?;
    let (inventory, triage) =
        nomifun_learning::run_kb_triage(&wiring.knowledge, &[kb_id.clone()], goal, Some(completer.as_ref()))
            .await
            .map_err(|error| anyhow::anyhow!("triage failed: {error}"))?;
    print_triage_readout(&triage);
    let mut report = course_report_json(&triage, None);
    if plan_only {
        let plan = nomifun_learning::run_kb_structure(
            &wiring.knowledge,
            &inventory,
            &triage,
            goal,
            Some(completer.as_ref()),
            None,
        )
        .await
        .map_err(|error| anyhow::anyhow!("structure discovery failed: {error}"))?;
        match plan.filter(|plan| plan.is_usable()) {
            Some(plan) => {
                print_plan_readout(&plan);
                report = course_report_json(&triage, Some(&plan));
            }
            None => {
                eprintln!("structure plan unusable (no chapters / degraded empty)");
                write_course_report(report_json, &report)?;
                return Ok(ExitCode::from(2));
            }
        }
    }
    write_course_report(report_json, &report)?;
    println!("\nEXIT=0");
    Ok(ExitCode::SUCCESS)
}

/// 分诊读数：七角色计数 + degraded + 排除/重复/uncertain 逐条理由——
/// 「跳过与不确定可见」是无头冒烟的第一读数。
fn print_triage_readout(triage: &nomifun_learning::KbTriageReport) {
    use nomifun_learning::KbTriageRole;
    let counts = &triage.counts;
    let total = counts.core
        + counts.prerequisite
        + counts.practice
        + counts.mixed
        + counts.noise
        + counts.duplicate
        + counts.uncertain;
    println!(
        "triage: total={total} core={} prerequisite={} practice={} mixed={} noise={} duplicate={} uncertain={} degraded={}",
        counts.core,
        counts.prerequisite,
        counts.practice,
        counts.mixed,
        counts.noise,
        counts.duplicate,
        counts.uncertain,
        triage.degraded
    );
    for warning in &triage.warnings {
        println!("  warning: {warning}");
    }
    if !triage.excluded.is_empty() {
        println!("excluded ({}):", triage.excluded.len());
        for entry in &triage.excluded {
            println!(
                "  - [{}] {} :: {}",
                entry.role.as_str(),
                entry.path,
                entry.reason
            );
        }
    }
    if !triage.duplicates.is_empty() {
        println!("duplicates ({}):", triage.duplicates.len());
        for duplicate in &triage.duplicates {
            println!(
                "  - {} -> {} ({})",
                duplicate.path, duplicate.duplicate_of, duplicate.canonical_reason
            );
        }
    }
    let uncertain: Vec<&nomifun_learning::KbTriageFile> = triage
        .files
        .iter()
        .filter(|file| file.role == KbTriageRole::Uncertain)
        .collect();
    if !uncertain.is_empty() {
        println!("uncertain ({}):", uncertain.len());
        for file in uncertain {
            println!("  - {} :: {}", file.rel_path, file.reason);
        }
    }
}

/// 结构计划读数：章/概念/边 + 逐章锚点 + 覆盖台账 + gaps。
fn print_plan_readout(plan: &nomifun_learning::SourcePlan) {
    use nomifun_learning::{FileCoverageStatus, TopicStatus};
    println!(
        "plan: chapters={} concepts={} edges={} degraded={} warnings={}",
        plan.chapters.len(),
        plan.concept_graph.nodes.len(),
        plan.concept_graph.edges.len(),
        plan.degraded,
        plan.warnings.len()
    );
    for (index, chapter) in plan.chapters.iter().enumerate() {
        println!(
            "  {}. {} — anchors={} covers={} type={:?}",
            index + 1,
            chapter.title,
            chapter.source_anchors.len(),
            chapter.covers.len(),
            chapter.content_type
        );
    }
    if !plan.evidence.gaps.is_empty() {
        println!("gaps ({}):", plan.evidence.gaps.len());
        for gap in &plan.evidence.gaps {
            println!("  - {gap}");
        }
    }
    let ledger = &plan.coverage;
    let covered = ledger
        .topics
        .iter()
        .filter(|topic| topic.status == TopicStatus::Covered)
        .count();
    let skipped = ledger
        .topics
        .iter()
        .filter(|topic| topic.status == TopicStatus::Skipped)
        .count();
    let uncertain = ledger
        .topics
        .iter()
        .filter(|topic| topic.status == TopicStatus::Uncertain)
        .count();
    println!(
        "coverage ledger: topics covered={covered} skipped={skipped} uncertain={uncertain}; files covered={} partial={} skipped={} uncertain={}; excluded={}",
        ledger
            .files
            .iter()
            .filter(|file| file.status == FileCoverageStatus::Covered)
            .count(),
        ledger
            .files
            .iter()
            .filter(|file| file.status == FileCoverageStatus::Partial)
            .count(),
        ledger
            .files
            .iter()
            .filter(|file| file.status == FileCoverageStatus::Skipped)
            .count(),
        ledger
            .files
            .iter()
            .filter(|file| file.status == FileCoverageStatus::Uncertain)
            .count(),
        ledger.excluded.len()
    );
    for entry in &ledger.excluded {
        println!(
            "  excluded: [{}] {} :: {}",
            entry.role.as_str(),
            entry.rel_path,
            entry.reason
        );
    }
}

/// 报告 JSON：triage 档 `{triage}`，plan 档 `{triage, plan}`（t1/t2 类型
/// 原样序列化,字段形状与库内信封一致,便于对账）。
fn course_report_json(
    triage: &nomifun_learning::KbTriageReport,
    plan: Option<&nomifun_learning::SourcePlan>,
) -> serde_json::Value {
    let mut report = serde_json::json!({ "triage": triage });
    if let Some(plan) = plan {
        report["plan"] = serde_json::json!(plan);
    }
    report
}

/// 报告落盘（pretty JSON；建父目录）。路径缺席 = 调用方没要报告,no-op。
fn write_course_report(path: Option<&Path>, report: &serde_json::Value) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create report dir {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(report).context("serialize course report")?;
    std::fs::write(path, body).with_context(|| format!("write report {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod course_report_tests {
    use super::course_report_json;
    use nomifun_learning::{KbTriageCounts, KbTriageReport};

    fn triage_fixture() -> KbTriageReport {
        KbTriageReport {
            files: Vec::new(),
            counts: KbTriageCounts {
                core: 2,
                noise: 1,
                ..KbTriageCounts::default()
            },
            excluded: Vec::new(),
            duplicates: Vec::new(),
            degraded: false,
            warnings: Vec::new(),
            batch_repair_count: 0,
            still_missing_count: 0,
        }
    }

    /// triage 档只有 triage 面;plan 档带 plan 面(章/概念/覆盖台账原样)。
    #[test]
    fn course_report_carries_triage_and_optional_plan() {
        let triage_only = course_report_json(&triage_fixture(), None);
        assert!(triage_only.get("triage").is_some(), "triage 档必须有 triage 面");
        assert!(
            triage_only.get("plan").is_none(),
            "triage 档不带 plan 面"
        );
        assert_eq!(triage_only["triage"]["counts"]["core"], serde_json::json!(2));

        let mut plan = nomifun_learning::SourcePlan::default();
        plan.chapters = vec![nomifun_learning::PlannedChapter {
            title: "排序入门".to_owned(),
            summary: String::new(),
            learning_objectives: Vec::new(),
            covers: Vec::new(),
            prerequisites: Vec::new(),
            source_anchors: Vec::new(),
            content_type: nomifun_learning::ChapterContentType::Concept,
        }];
        let with_plan = course_report_json(&triage_fixture(), Some(&plan));
        assert_eq!(with_plan["plan"]["chapters"].as_array().unwrap().len(), 1);
        assert!(with_plan["triage"].is_object(), "triage 面恒在");
    }
}

/// `learning-smoke structure`：从零（或 --db）立一门最小学习图课程，跑结构
/// 生成管线，机械判据自检。
#[allow(clippy::too_many_arguments)]
async fn run_structure(
    cli: &Cli,
    goal: String,
    proficiency: Option<String>,
    basis: Option<String>,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
) -> Result<ExitCode> {
    if goal.trim().is_empty() {
        eprintln!("--goal must not be empty");
        return Ok(ExitCode::from(2));
    }
    let (wiring, _archiving, _live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), true).await?;
    let model_override = provider
        .as_deref()
        .zip(model.as_deref())
        .map(|(provider, model)| {
            (
                nomifun_common::ProviderId::parse(provider).expect("provider id"),
                model.to_owned(),
            )
        });

    // 最小课程行（HTTP 建课的事务子集——冒烟对象是生成管线本身）。
    let course_id = nomifun_common::LearningCourseId::new();
    let module_id = nomifun_common::LearningModuleId::new();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, created_at, updated_at) \
         VALUES (?, ?, '', 'general', 1, 'learning_graph', ?, '', 1, 1)",
    )
    .bind(course_id.as_str())
    .bind(goal.trim())
    .bind(goal.trim())
    .execute(&wiring.pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(module_id.as_str())
    .bind(course_id.as_str())
    .execute(&wiring.pool)
    .await?;

    println!("[1/2] generating structure + terrain (json_object, ≤3 clause-feedback rounds)…");
    let input = nomifun_learning::StructureInput::new(
        &goal,
        proficiency.as_deref(),
        basis.as_deref(),
    );
    if let Err(error) = wiring
        .service
        .run_structure_generation(&course_id, &wiring.owner, model_override.as_ref().map(|(provider, model)| (provider, model.as_str())), &input)
        .await
    {
        eprintln!("structure generation FAILED: {error}");
        return Ok(ExitCode::from(2));
    }

    println!("[2/2] mechanical criteria: re-read snapshot and re-verify both contracts + door reconciliation…");
    let snapshot: String = sqlx::query_scalar(
        "SELECT structure_json FROM learning_courses WHERE course_id = ?",
    )
    .bind(course_id.as_str())
    .fetch_one(&wiring.pool)
    .await?;
    let envelope: serde_json::Value = serde_json::from_str(&snapshot)?;
    let structure = envelope
        .get("structure")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("snapshot envelope missing structure"))?;
    let terrain = envelope
        .get("terrain")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("snapshot envelope missing terrain"))?;
    let mut violations = nomifun_learning::validate_structure_artifact(&structure);
    violations.extend(nomifun_learning::validate_terrain_artifact(&terrain));
    violations.extend(nomifun_learning::reconcile_structure_doors(&structure, &terrain));
    let red: Vec<_> = violations.iter().filter(|violation| !violation.warning).collect();
    for violation in &red {
        eprintln!("CRITERION VIOLATION [{}] {}", violation.code, violation.message);
    }
    if !red.is_empty() {
        return Ok(ExitCode::from(2));
    }
    let blocks = structure["blocks"].as_array().map(Vec::len).unwrap_or(0);
    let spines = structure["spines"].as_array().map(Vec::len).unwrap_or(0);
    let edges = structure["edges"].as_array().map(Vec::len).unwrap_or(0);
    let doors = terrain["doors"].as_array().map(Vec::len).unwrap_or(0);
    println!(
        "OK  course={}  spines={spines}  blocks={blocks}  edges={edges}  doors={doors}",
        course_id.as_str()
    );
    Ok(ExitCode::SUCCESS)
}

/// `learning-smoke json`：json_object 能力读数。探测调用直调内层 completer
/// （不挂 ArchivingCompleter）；唯一硬判据 = 全灭 0/n。
async fn run_json(
    cli: &Cli,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
    calls: usize,
) -> Result<ExitCode> {
    if calls == 0 {
        eprintln!("--calls must be ≥ 1");
        return Ok(ExitCode::from(2));
    }
    let (_wiring, _archiving, live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), false).await?;
    let model_override = provider
        .as_deref()
        .zip(model.as_deref())
        .map(|(provider, model)| {
            (
                nomifun_common::ProviderId::parse(provider).expect("provider id"),
                model.to_owned(),
            )
        });
    // 探测调用直调内层 completer（不挂 Archiving）——非管线调用，不入档案。
    let completer: &Arc<dyn LearningCompleter> = &live;

    const PROBE: &str = "只回复一个 JSON 对象：{\"ok\": true, \"probe\": \"json_object capability\"}。不要解释、不要代码围栏。";
    let mut valid_objects = 0usize;
    let mut empty = 0usize;
    let mut malformed = 0usize;
    for index in 1..=calls {
        let outcome = completer
            .complete_json(
                model_override.as_ref().map(|(provider, model)| (provider.as_str(), model.as_str())),
                "你是 JSON 探测器。",
                PROBE,
                256,
                None, // 探测调用用 provider 默认温度（读数面不掺工程旋钮）
            )
            .await;
        match outcome {
            Ok(reply) => {
                let trimmed = reply.trim();
                if trimmed.is_empty() {
                    empty += 1;
                    println!("  probe {index}: EMPTY");
                } else {
                    match serde_json::from_str::<serde_json::Value>(trimmed) {
                        Ok(value) if value.is_object() => {
                            valid_objects += 1;
                            println!("  probe {index}: valid object");
                        }
                        Ok(_) => {
                            malformed += 1;
                            println!("  probe {index}: valid JSON, not an object");
                        }
                        Err(error) => {
                            malformed += 1;
                            let head: String = trimmed.chars().take(80).collect();
                            println!("  probe {index}: parse error ({error}); head: {head}");
                        }
                    }
                }
            }
            Err(error) => {
                malformed += 1;
                println!("  probe {index}: call failed: {error}");
            }
        }
    }
    println!(
        "json_object read-out: {valid_objects}/{calls} valid objects (empty {empty}, malformed {malformed})"
    );
    if valid_objects == 0 {
        eprintln!("all {calls} probes failed — the provider/model cannot deliver json_object");
        return Ok(ExitCode::from(2));
    }
    Ok(ExitCode::SUCCESS)
}

/// `learning-smoke grow` / `loop`：生长冒烟（loop = 结构生成先行）。
/// s1 判据（失败非零退出）：
/// - 首批 assumes 全空（仅空登记表机械地板；账非空时由就绪即发布判据验覆盖）；
/// - 每批「就绪即发布」——批内节点的每条 assumes 都被**既往供给**（更早
///   批次或其他课程的 teaches）以不低于所假定档位覆盖（同批不算）；
/// - 铸名不撞：登记表 canonical ∪ 别名全局唯一；
/// - loop 追加：structure_json 落列（结构产物在列）。
/// 批间用确定性学习者替身：程序化把全部未完成节点标为 completed（完成事件
/// 自动触发生长，与真实学习者路径同线）。
#[allow(clippy::too_many_arguments)]
async fn run_grow(
    cli: &Cli,
    goal: String,
    batches: usize,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
    with_structure: bool,
) -> Result<ExitCode> {
    if goal.trim().is_empty() {
        eprintln!("--goal must not be empty");
        return Ok(ExitCode::from(2));
    }
    if batches == 0 {
        eprintln!("--batches/--rounds must be ≥ 1");
        return Ok(ExitCode::from(2));
    }
    let (wiring, _archiving, _live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), true).await?;
    let model_override = provider
        .as_deref()
        .zip(model.as_deref())
        .map(|(provider, model)| {
            (
                nomifun_common::ProviderId::parse(provider).expect("provider id"),
                model.to_owned(),
            )
        });

    // 最小课程行（HTTP 建课的事务子集——冒烟对象是生长管线本身）。
    let course_id = nomifun_common::LearningCourseId::new();
    let module_id = nomifun_common::LearningModuleId::new();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          created_at, updated_at) \
         VALUES (?, ?, '', 'general', 1, 'learning_graph', ?, 1, 1)",
    )
    .bind(course_id.as_str())
    .bind(goal.trim())
    .bind(goal.trim())
    .execute(&wiring.pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(module_id.as_str())
    .bind(course_id.as_str())
    .execute(&wiring.pool)
    .await?;

    if with_structure {
        println!("[structure] generating knowledge structure (gd.* contract, ≤3 rounds)…");
        let input = nomifun_learning::StructureInput::new(&goal, None, None);
        if let Err(error) = wiring
            .service
            .run_structure_generation(
                &course_id,
                &wiring.owner,
                model_override
                    .as_ref()
                    .map(|(provider, model)| (provider, model.as_str())),
                &input,
            )
            .await
        {
            eprintln!("structure generation FAILED: {error}");
            return Ok(ExitCode::from(2));
        }
    }

    for round in 1..=batches {
        // 自动补批判定读数（m10，读数面非门禁）：裁决结果 + ready 数——
        // 双臂对照卡数据源（on 臂 due + 批后 ready 上升；off 臂 disabled
        // 如实）。读不到不拦流程（冒烟对象是生长管线本身）。
        match wiring.service.growth_verdict(&wiring.owner, &course_id).await {
            Ok(view) => println!(
                "[auto-refill] round {round} pre  verdict={} ready={} trigger={} enabled={} stalled={}",
                view.verdict, view.ready_count, view.trigger, view.enabled, view.stalled
            ),
            Err(error) => println!("[auto-refill] round {round} pre verdict readout unavailable: {error}"),
        }
        if round > 1 {
            // 确定性学习者替身：首节点完成、其余跳过（两种满足路径都过，
            // 进度事件自动触发下一轮生长——与真实学习者同线）。
            let stand_in = complete_all_nodes(&wiring, &course_id).await?;
            println!("[stand-in] drove {stand_in} nodes (1 completed / rest skipped) to trigger the next round");
        } else {
            wiring
                .service
                .kick_growth(
                    &wiring.owner,
                    &course_id,
                    true,
                    model_override
                        .as_ref()
                        .map(|(provider, model)| (provider, model.as_str())),
                )
                .await?;
        }
        if !wait_applied_batches(&wiring.pool, &course_id, round as i64).await? {
            eprintln!("round {round}: growth batch did not land (applied) in time");
            return Ok(ExitCode::from(2));
        }
        println!("[grow] round {round} batch applied — auditing criteria…");
        let violations = audit_batch(&wiring.pool, &course_id, round as i64).await?;
        for violation in &violations {
            eprintln!("CRITERION VIOLATION [round {round}] {violation}");
        }
        if !violations.is_empty() {
            return Ok(ExitCode::from(2));
        }
        let (nodes, seq) = batch_summary(&wiring.pool, &course_id, round as i64).await?;
        // 重试读数（二轮 t1，读数面非门禁）：最近一次生长的轮次与停因。
        // 挂号表读数行（m3）：rows/invalid/retried/degraded——机械校验观测面。
        // 碎片裁决读数行（m9 补接）：total/rows/unhandled/degraded——同款观测面。
        let stats = wiring.service.growth_run_stats();
        match stats {
            Some(stats) => println!(
                "  round {round}: batch #{seq}, {nodes} nodes, criteria green  \
                 [retry: attempts_used={}, stop_reason={}]  \
                 [provenance: rows={} invalid={} retried={} degraded={}]  \
                 [fragments: total={} rows={} unhandled={} degraded={}]  \
                 [review_state: {} injections={}]",
                stats.attempts_used,
                stats.stop_reason,
                stats.review_provenance_rows,
                stats.review_provenance_invalid,
                stats.review_provenance_retried,
                stats.review_provenance_degraded,
                stats.review_fragments_total,
                stats.review_fragments_rows,
                stats.review_fragments_unhandled,
                stats.review_fragments_degraded,
                if stats.review_state { "on" } else { "off" },
                stats.review_state_injections
            ),
            None => println!("  round {round}: batch #{seq}, {nodes} nodes, criteria green"),
        }
        // 批后对照读数（m10）：ready 前后并排的「后」半张——对照卡看批后
        // 水位回升（on 臂），off 臂此行同样如实（verdict=disabled）。
        match wiring.service.growth_verdict(&wiring.owner, &course_id).await {
            Ok(view) => println!(
                "[auto-refill] round {round} post verdict={} ready={} (对照 pre 行看批后水位)",
                view.verdict, view.ready_count
            ),
            Err(error) => println!("[auto-refill] round {round} post verdict readout unavailable: {error}"),
        }
        // 铸名门外部证据读数（m11，读数面非门禁）：搜索/命中/扩邻/降级；
        // off 臂（`NOMI_LEARNING_MINT_RESEARCH=off`）显式零 = 零搜索证明。
        if let Some(mint) = wiring.service.mint_research_stats() {
            println!(
                "  [mint-research: searches={}, evidence_hits={}, expansions={}, degraded={}]",
                mint.searches, mint.evidence_hits, mint.expansions, mint.degraded
            );
        }
    }

    if with_structure {
        let snapshot: Option<String> = sqlx::query_scalar(
            "SELECT structure_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&wiring.pool)
        .await?;
        if snapshot.as_deref().unwrap_or_default().trim().is_empty() {
            eprintln!("CRITERION VIOLATION [loop] structure_json is missing after structure generation");
            return Ok(ExitCode::from(2));
        }
    }
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lessons l \
         JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
    )
    .bind(course_id.as_str())
    .fetch_one(&wiring.pool)
    .await?;
    println!(
        "OK  course={}  batches={batches}  nodes={total}  ({} mode)",
        course_id.as_str(),
        if with_structure { "loop" } else { "grow" }
    );
    Ok(ExitCode::SUCCESS)
}

/// 确定性学习者替身：把全部未满足节点程序化标为 completed（触发自动生长）。
async fn complete_all_nodes(
    wiring: &SmokeWiring,
    course_id: &nomifun_common::LearningCourseId,
) -> Result<usize> {
    let lesson_ids: Vec<String> = sqlx::query_scalar(
        "SELECT l.lesson_id FROM learning_lessons l \
         JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
    )
    .bind(course_id.as_str())
    .fetch_all(&wiring.pool)
    .await?;
    let mut driven = 0usize;
    for (index, lesson_id) in lesson_ids.into_iter().enumerate() {
        let lesson: nomifun_common::LearningLessonId =
            nomifun_common::LearningLessonId::parse(&lesson_id)?;
        // 确定性学习者替身：首节点 completed、其余 skipped——两种满足路径
        // 都程序化过一遍（完成/跳过都会触发生长的自动检查；重复赋值幂等）。
        let status = if index == 0 {
            nomifun_learning::LessonStatus::Completed
        } else {
            nomifun_learning::LessonStatus::Skipped
        };
        let flipped = wiring
            .service
            .update_lesson_progress(&lesson, &wiring.owner, status)
            .await;
        match flipped {
            Ok(()) => driven += 1,
            Err(error) => eprintln!("stand-in: lesson {lesson_id} progress update failed: {error}"),
        }
    }
    Ok(driven)
}

/// 等待第 `target` 个批次落定（applied），最多 15 分钟。
async fn wait_applied_batches(
    pool: &nomifun_db::sqlx::SqlitePool,
    course_id: &nomifun_common::LearningCourseId,
    target: i64,
) -> Result<bool> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15 * 60);
    loop {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_growth_batches \
             WHERE course_id = ? AND status = 'applied'",
        )
        .bind(course_id.as_str())
        .fetch_one(pool)
        .await?;
        if count >= target {
            return Ok(true);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

async fn batch_summary(
    pool: &nomifun_db::sqlx::SqlitePool,
    course_id: &nomifun_common::LearningCourseId,
    seq: i64,
) -> Result<(usize, i64)> {
    let node_ids_json: String = sqlx::query_scalar(
        "SELECT node_ids_json FROM learning_growth_batches \
         WHERE course_id = ? AND seq = ? AND status = 'applied'",
    )
    .bind(course_id.as_str())
    .bind(seq)
    .fetch_one(pool)
    .await?;
    let node_ids: Vec<String> = serde_json::from_str(&node_ids_json).unwrap_or_default();
    Ok((node_ids.len(), seq))
}

/// 批次机械判据审计（s1）：首批 assumes 地板（空账全空/非空账验覆盖） + 就绪即发布（既往供给覆盖）
/// + 铸名全局唯一。
async fn audit_batch(
    pool: &nomifun_db::sqlx::SqlitePool,
    course_id: &nomifun_common::LearningCourseId,
    seq: i64,
) -> Result<Vec<String>> {
    let mut violations = Vec::new();
    let node_ids_json: String = sqlx::query_scalar(
        "SELECT node_ids_json FROM learning_growth_batches \
         WHERE course_id = ? AND seq = ? AND status = 'applied'",
    )
    .bind(course_id.as_str())
    .bind(seq)
    .fetch_one(pool)
    .await?;
    let node_ids: Vec<String> = serde_json::from_str(&node_ids_json).unwrap_or_default();

    // 批内节点的 assumes。
    let mut batch_assumes: Vec<(String, String, String)> = Vec::new(); // (lesson, concept, tier)
    for lesson_id in &node_ids {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT concept_id, tier FROM learning_lesson_concepts \
             WHERE role = 'assumes' AND lesson_id = ?",
        )
        .bind(lesson_id)
        .fetch_all(pool)
        .await?;
        for (concept_id, tier) in rows {
            batch_assumes.push((lesson_id.clone(), concept_id, tier));
        }
    }
    // 首批机械地板（s1）：仅当登记表/已教账为空（缺省临时库从零合龙）时，
    // 首批 assumes 必须全空。`--db` 指开发库时账是跨课程全局账、非空是常态
    // ——账内概念可被首批合法引用，其覆盖性由下面的「就绪即发布」判据验证
    // （该判据本就含其他课程的 teaches）。
    if seq == 1 && !batch_assumes.is_empty() {
        let taught_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_lesson_concepts lc \
             JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_courses c ON c.course_id = m.course_id AND c.deleted_at IS NULL \
             WHERE lc.role = 'teaches'",
        )
        .fetch_one(pool)
        .await?;
        if taught_rows == 0 {
            violations.push(format!(
                "首批 assumes 必须全空（空登记表机械地板），发现 {} 条 assume 引用",
                batch_assumes.len()
            ));
        }
    }

    // 就绪即发布：本批节点的每条 assumes 必须被「既往供给」（本课程更早批次
    // 或其他课程的 teaches）以不低于所假定档位覆盖；同批（含更晚批）不算。
    if !batch_assumes.is_empty() {
        // lesson → 本课程批次 seq（仅本课程视图）。
        let mut lesson_seq: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        let batch_rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT seq, node_ids_json FROM learning_growth_batches \
             WHERE course_id = ? AND status = 'applied'",
        )
        .bind(course_id.as_str())
        .fetch_all(pool)
        .await?;
        for (batch_seq, ids_json) in batch_rows {
            for lesson_id in serde_json::from_str::<Vec<String>>(&ids_json).unwrap_or_default() {
                lesson_seq.insert(lesson_id, batch_seq);
            }
        }
        // 全局供给行：(lesson, concept, tier, course)。
        let teaches_rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT lc.lesson_id, lc.concept_id, lc.tier, m.course_id \
             FROM learning_lesson_concepts lc \
             JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_courses c ON c.course_id = m.course_id AND c.deleted_at IS NULL \
             WHERE lc.role = 'teaches'",
        )
        .fetch_all(pool)
        .await?;
        let rank = |tier: &str| match tier {
            "teach" => 2,
            "apply" => 1,
            _ => 0,
        };
        for (lesson_id, concept_id, tier) in &batch_assumes {
            let covered = teaches_rows.iter().any(
                |(teacher_lesson, teacher_concept, teacher_tier, teacher_course)| {
                    if teacher_concept != concept_id || rank(teacher_tier) < rank(tier) {
                        return false;
                    }
                    // 其他课程供给恒有效；本课程供给必须来自更早批次。
                    if teacher_course != course_id.as_str() {
                        return true;
                    }
                    lesson_seq
                        .get(teacher_lesson)
                        .map(|teacher_seq| *teacher_seq < seq)
                        .unwrap_or(false)
                },
            );
            if !covered {
                violations.push(format!(
                    "就绪即发布被破坏：批次 #{seq} 节点 {lesson_id} 假定概念 {concept_id}@{tier}，既往供给未覆盖"
                ));
            }
        }
    }

    // 铸名不撞：登记表 canonical ∪ 别名全局唯一（大小写折叠）。
    let registry_rows: Vec<(String, String)> =
        sqlx::query_as("SELECT canonical, aliases_json FROM learning_concept_registry")
            .fetch_all(pool)
            .await?;
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (canonical, aliases_json) in &registry_rows {
        let mut names = vec![canonical.trim().to_lowercase()];
        for alias in serde_json::from_str::<Vec<String>>(aliases_json).unwrap_or_default() {
            let alias = alias.trim().to_lowercase();
            if !alias.is_empty() {
                names.push(alias);
            }
        }
        for name in names {
            if let Some(owner) = seen.get(&name) {
                if owner != canonical {
                    violations.push(format!(
                        "铸名撞名：「{name}」同时属于「{owner}」与「{canonical}」"
                    ));
                }
            } else {
                seen.insert(name, canonical.clone());
            }
        }
    }
    Ok(violations)
}

/// `learning-smoke xp`（j05）：XP 读数面冒烟。从零临时库种一门单课时课程
/// （est 25 + 一道单选），走真实管线结算两笔——教新线（练习全对完成课时 =
/// full 27）与复习线（卡到期答对自评 good = full 5）——然后打印读数与日历
/// 快照。读数面非门禁：唯一硬判据是两笔结算经真实服务路径落账。
async fn run_xp(cli: &Cli, db: Option<PathBuf>) -> Result<ExitCode> {
    let (wiring, _archiving, _live) = wire(cli, db.as_deref(), None, None, false).await?;
    let owner = wiring.owner.clone();
    let service = wiring.service.clone();

    // 种子课程：单课时（est 25）+ 单选题，报名为 owner。
    let course_id = nomifun_common::LearningCourseId::new().into_string();
    let module_id = nomifun_common::LearningModuleId::new().into_string();
    let lesson_id = nomifun_common::LearningLessonId::new().into_string();
    let activity_id = nomifun_common::LearningActivityId::new().into_string();
    let enrollment_id = nomifun_common::LearningEnrollmentId::new().into_string();
    let now = nomifun_common::now_ms();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, 'XP smoke course', '', 'general', 1, ?, ?)",
    )
    .bind(&course_id)
    .bind(now)
    .bind(now)
    .execute(&wiring.pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, 'Module', '', 0)",
    )
    .bind(&module_id)
    .bind(&course_id)
    .execute(&wiring.pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_lessons (lesson_id, module_id, title, summary, position, estimated_minutes) \
         VALUES (?, ?, 'Vectors', '', 0, 25)",
    )
    .bind(&lesson_id)
    .bind(&module_id)
    .execute(&wiring.pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_enrollments \
         (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&enrollment_id)
    .bind(owner.as_str())
    .bind(&course_id)
    .bind(now)
    .bind(now)
    .execute(&wiring.pool)
    .await?;
    let config = serde_json::json!({
        "answer": "4",
        "options": ["3", "4"],
        "explanation": "",
        "distractors": [],
        "matches": []
    });
    sqlx::query(
        "INSERT INTO learning_activities \
         (activity_id, lesson_id, kind, prompt, config_json, position) \
         VALUES (?, ?, 'single_choice', '2 + 2 = ?', ?, 0)",
    )
    .bind(&activity_id)
    .bind(&lesson_id)
    .bind(serde_json::to_string(&config)?.as_str())
    .execute(&wiring.pool)
    .await?;

    // 教新线：练习全对 → 完成课时 → full（25 + bonus 2）。
    let activity = nomifun_common::LearningActivityId::parse(&activity_id)?;
    let lesson = nomifun_common::LearningLessonId::parse(&lesson_id)?;
    let attempt = service
        .submit_attempt(&activity, &owner, serde_json::json!("4"), None, None)
        .await?;
    anyhow::ensure!(attempt.passed, "smoke attempt must pass");
    service
        .update_lesson_progress(&lesson, &owner, nomifun_learning::LessonStatus::Completed)
        .await?;

    // 复习线：卡重列到期 → 答对 → 自评 good → full（base 3 + bonus 2）。
    sqlx::query("UPDATE learning_review_items SET due_at = ?")
        .bind(nomifun_common::now_ms() - 1_000)
        .execute(&wiring.pool)
        .await?;
    let due = service
        .due_reviews(&owner, 10, &[], true, false, &[])
        .await?;
    anyhow::ensure!(due.len() == 1, "expected exactly one seeded card");
    let card = due[0].id.clone();
    let answer = service
        .answer_review(&card, &owner, serde_json::json!("4"), false, Some(9_000))
        .await?;
    anyhow::ensure!(answer.correct, "smoke answer must be correct");
    service.rate_review(&card, &owner, nomifun_learning::ReviewRating::Good).await?;

    // 读数/日历快照打印（读数面非门禁）。
    let reading = service.xp_reading(&owner).await?;
    let calendar = service.xp_calendar(&owner, 12).await?;
    println!(
        "xp reading: {}",
        serde_json::to_string_pretty(&reading)?
    );
    let today = calendar.days.last().map(|day| (day.review_day, day.xp));
    println!(
        "xp calendar: {} buckets, today {today:?}, nonzero days {}",
        calendar.days.len(),
        calendar.days.iter().filter(|day| day.xp != 0).count(),
    );

    // 机械自检（只报告，不改退出码）：教新 27 + 复习 5 = 32。
    let settled: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN kind = 'node-completed' THEN 1 ELSE 0 END), 0), \
         COALESCE(SUM(CASE WHEN kind = 'review-completed' THEN 1 ELSE 0 END), 0) \
         FROM learning_xp_events",
    )
    .fetch_one(&wiring.pool)
    .await?;
    println!(
        "xp events: total {} (node {} / review {}) — expected today_xp 32, got {}",
        settled.0,
        settled.1,
        settled.2,
        reading.today_xp
    );
    Ok(ExitCode::SUCCESS)
}

/// `learning-smoke quiz`（q2 载体 + q3 质量增量）：quiz 回合全链路冒烟（零
/// provider 依赖——变式生成无 provider 时如实跳过）。从零临时库种一门 K 节点
/// 课程（est 25/节点，各带一道单选），走真实管线逐节点结算后开测，机械判据
/// （失败非零退出）：
/// ① K−1 结算时 start Conflict（附 N/K）、恰 K 到期；
/// ② 弱先排序——末节点首卡推成错答（escalation 档）后必须先出；无 provider
///    变式如实跳过（bank-only）；start 投影判底零字段；grade 判分并下发判底；
/// ③ complete 逐节点分级落账——full/partial 各至少一笔、恒非负、账本总额与
///    回合行对账一致（盘面复验）；
/// ④ 校准读面——被真实推进卡的校准点带预测 R，分桶非空；
/// ⑤ 同日折叠——第二轮（同计划）全折叠零计薪，账本零新增；
/// ⑥ 首写为终——重复 complete Conflict。
/// 变式的 LLM 调用落观测档案（ArchivingCompleter；有 provider 时生效）。
async fn run_quiz(
    cli: &Cli,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
) -> Result<ExitCode> {
    use std::collections::HashMap;

    if provider.is_some() != model.is_some() {
        anyhow::bail!("--provider and --model must be provided together");
    }
    let k = nomifun_learning::QUIZ_TRIGGER_SETTLEMENTS;
    // k04：读数从零开始（进程内计数器，多次运行同一进程时不吃陈旧数）。
    nomifun_learning::qb_readings::reset();
    // 变式站的 LLM 调用落观测档案（structure 冒烟同纪律）：挂 ArchivingCompleter。
    let (wiring, _archiving, _live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), true).await?;
    // 双环境判据（t5）：显式 provider 或指向带凭据库（--db）= 变式在场要求；
    // 零凭据临时库 = 变式诚实跳过路径。在场与否以 plan 实况为准（凭据在
    // 调用时解析），要求在场而一条都没生成 = 硬失败。
    let require_variants = provider.is_some() || db.is_some();
    let owner = wiring.owner.clone();
    let service = wiring.service.clone();
    let now = nomifun_common::now_ms();

    // 种子课程：K 节点（est 25），每节点一道单选（answer = "B"），报名 owner。
    let course_id = nomifun_common::LearningCourseId::new().into_string();
    let module_id = nomifun_common::LearningModuleId::new().into_string();
    let enrollment_id = nomifun_common::LearningEnrollmentId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, 'quiz round smoke', '', 'general', 1, ?, ?)",
    )
    .bind(&course_id)
    .bind(now)
    .bind(now)
    .execute(&wiring.pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, 'Module', '', 0)",
    )
    .bind(&module_id)
    .bind(&course_id)
    .execute(&wiring.pool)
    .await?;
    let config =
        r#"{"answer":"B","options":["A","B"],"explanation":"smoke","distractors":[],"matches":[]}"#;
    let mut lesson_ids = Vec::new();
    let mut activity_ids = Vec::new();
    for index in 0..k {
        let lesson_id = nomifun_common::LearningLessonId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, position, estimated_minutes) \
             VALUES (?, ?, ?, '', ?, 25)",
        )
        .bind(&lesson_id)
        .bind(&module_id)
        .bind(format!("Node {index}"))
        .bind(index)
        .execute(&wiring.pool)
        .await?;
        let activity_id = nomifun_common::LearningActivityId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_activities \
             (activity_id, lesson_id, kind, prompt, config_json, position) \
             VALUES (?, ?, 'single_choice', ?, ?, 0)",
        )
        .bind(&activity_id)
        .bind(&lesson_id)
        .bind(format!("What is {index} + {index}?"))
        .bind(config)
        .execute(&wiring.pool)
        .await?;
        lesson_ids.push(lesson_id);
        activity_ids.push(activity_id);
    }
    sqlx::query(
        "INSERT INTO learning_enrollments \
         (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&enrollment_id)
    .bind(owner.as_str())
    .bind(&course_id)
    .bind(now)
    .bind(now)
    .execute(&wiring.pool)
    .await?;
    let course = nomifun_common::LearningCourseId::parse(&course_id)?;
    let complete = async |lesson_id: &str| -> anyhow::Result<()> {
        let lesson = nomifun_common::LearningLessonId::parse(lesson_id)
            .map_err(|e| anyhow::anyhow!("lesson id: {e}"))?;
        Ok(service
            .update_lesson_progress(&lesson, &owner, nomifun_learning::LessonStatus::Completed)
            .await?)
    };

    // ① 触发门：K−1 未到期（Conflict 附 N/K），恰 K 到期。
    for lesson_id in &lesson_ids[..(k as usize - 1)] {
        complete(lesson_id).await?;
    }
    let error = service.quiz_start(&owner, &course).await.unwrap_err();
    anyhow::ensure!(
        matches!(error, nomifun_common::AppError::Conflict(ref m) if m.contains(&format!("quiz not due: {}/", k - 1))),
        "K−1 settlements must not be due: {error:?}"
    );
    complete(lesson_ids.last().expect("k ≥ 1")).await?;
    println!("[1/6] trigger gate: not due at {}/K, due at K", k - 1);

    // ② 弱先：把末节点的首卡推成「最近一次真实推进 = 错答」（escalation 档），
    // 开测后它必须先出（交错序第 1 槽首位）。无 provider 时变式如实跳过
    // （每节点只出旧题——learnhub「失败顺延」的回合内化）。
    let weak_lesson = lesson_ids[(k as usize - 1)].clone();
    let weak_item: String = sqlx::query_scalar(
        "SELECT r.review_item_id FROM learning_review_items r \
         JOIN learning_activities a ON a.activity_id = r.activity_id \
         WHERE a.lesson_id = ? ORDER BY r.id LIMIT 1",
    )
    .bind(&weak_lesson)
    .fetch_one(&wiring.pool)
    .await?;
    sqlx::query("UPDATE learning_review_items SET due_at = ? WHERE review_item_id = ?")
        .bind(nomifun_common::now_ms() - 1_000)
        .bind(&weak_item)
        .execute(&wiring.pool)
        .await?;
    let weak_answer = service
        .answer_review(
            &nomifun_common::LearningReviewItemId::parse(&weak_item)?,
            &owner,
            serde_json::json!("A"),
            false,
            Some(1_000),
        )
        .await?;
    anyhow::ensure!(
        !weak_answer.correct,
        "wrong answer must grade incorrect (auto Again = escalation sample)"
    );
    let round = service.quiz_start(&owner, &course).await?;
    anyhow::ensure!(
        round.questions[0].node_id.as_str() == weak_lesson,
        "escalated node must come first, got {}",
        round.questions[0].node_id
    );
    let plan_json_started: String =
        sqlx::query_scalar("SELECT plan_json FROM learning_quiz_rounds WHERE round_id = ?")
            .bind(&round.round_id)
            .fetch_one(&wiring.pool)
            .await?;
    // 双环境分支（t5）：在场与否以 plan 实况为准——凭据在调用时解析，冒烟
    // 不预判。在场 = 每节点 bank + 恰一变式（题数 > bank 题数）；不在场且
    // 要求在场（--provider 或 --db 指带凭据库）= 硬失败；不在场且零凭据
    // 临时库 = 变式诚实跳过路径（q2/q3 判据原样）。
    let plan_items: Vec<serde_json::Value> = serde_json::from_str(&plan_json_started)?;
    let is_variant =
        |item: &serde_json::Value| item.get("origin").and_then(|v| v.as_str()) == Some("variant");
    let variant_count = plan_items.iter().filter(|item| is_variant(item)).count();
    // k04 读数行（非门禁）：题面防雷同门的确定性计数器——off 臂 sim_hits
    // 与 dropped 恒 0（门不在），on 臂为门实际命中数。
    let qb = nomifun_learning::qb_readings::snapshot();
    println!(
        "qb_similarity read-out: pools={k} pool_checks={} sim_hits={} dropped={} retries={} variants_generated={variant_count}",
        qb.pool_checks, qb.sim_hits, qb.dropped, qb.retries
    );
    let variants_present = variant_count > 0;
    // 每条计划项的判分底：bank 题 = 种子 config（answer "B"）；变式题 =
    // payload.config_json 的 answer（判底随计划行内下发，不回库）。
    let item_answer = |item: &serde_json::Value| -> serde_json::Value {
        if is_variant(item) {
            let config: serde_json::Value = serde_json::from_str(
                item.pointer("/variant/config_json")
                    .and_then(|v| v.as_str())
                    .expect("variant payload carries config_json"),
            )
            .expect("variant config_json parses");
            config.get("answer").cloned().expect("variant config carries answer")
        } else {
            serde_json::json!("B")
        }
    };
    if variants_present {
        let mut per_node: HashMap<&str, usize> = HashMap::new();
        for item in &plan_items {
            *per_node
                .entry(item.get("node").and_then(|v| v.as_str()).unwrap_or_default())
                .or_default() += 1;
        }
        anyhow::ensure!(
            per_node.len() == k as usize && per_node.values().all(|count| *count == 2),
            "variants present: every node must carry bank + one variant ({variant_count} variants over {} nodes)",
            per_node.len()
        );
        anyhow::ensure!(
            round.questions.len() == 2 * k as usize,
            "variants present: per-node question count must exceed bank stock, got {}",
            round.questions.len()
        );
    } else {
        anyhow::ensure!(
            !require_variants,
            "credentials expected (explicit --provider/--model or --db) but no quiz variant was generated"
        );
        anyhow::ensure!(
            round.questions.len() == k as usize,
            "no provider: bank-only plan (one question per node), got {}",
            round.questions.len()
        );
        anyhow::ensure!(
            !plan_json_started.contains("\"origin\":\"variant\""),
            "no provider: variants must be skipped honestly"
        );
    }
    let view_json = serde_json::to_string(&round)?;
    anyhow::ensure!(
        !view_json.contains("explanation") && !view_json.contains("\"answer\""),
        "start projection leaked judge fields"
    );
    let graded = service
        .quiz_grade(
            &owner,
            &course,
            &round.round_id,
            round.questions[0].activity_id.as_str(),
            &serde_json::json!("B"),
        )
        .await?;
    anyhow::ensure!(
        graded.correct && graded.answer == serde_json::json!("B"),
        "grade must reveal the judge bottom after grading"
    );
    if variants_present {
        // 变式判底随 grade 下发：变式题判分输入来自 plan 内 payload，答对
        // 揭示的 answer 与 payload 一致（判分已定，揭示不破持答案红线）。
        let variant_item = plan_items
            .iter()
            .find(|item| is_variant(item))
            .expect("variant item present");
        let variant_activity = variant_item
            .get("activity")
            .and_then(|v| v.as_str())
            .expect("variant activity id")
            .to_owned();
        let expected = item_answer(variant_item);
        let graded_variant = service
            .quiz_grade(&owner, &course, &round.round_id, &variant_activity, &expected)
            .await?;
        anyhow::ensure!(
            graded_variant.correct && graded_variant.answer == expected,
            "variant judge bottom must flow through grade: {:?}",
            graded_variant
        );
    }
    println!(
        "[2/6] weak-first (escalated node first), {} sanitized start",
        if variants_present {
            format!("{variant_count} variants planned (bank + variant per node),")
        } else {
            "variants skipped honestly without provider,".to_owned()
        }
    );

    // ③ 全卷作答：弱节点与其前一节点的题答错（partial 0×2）、其余全对
    //（full 3×4 = 12）。变式在场时答满全计划（bank + 变式都作答——quiz 必
    // 作答是测量语义）；错误节点的变式答一个非正确项。断言按 node 定位
    //（计划序是弱先序）。
    let wrong_nodes: Vec<String> = vec![
        weak_lesson.clone(),
        round
            .questions
            .iter()
            .find(|q| q.activity_id.as_str() == activity_ids[(k as usize - 2)])
            .map(|q| q.node_id.to_string())
            .expect("second wrong node planned"),
    ];
    let mut answers = HashMap::new();
    for item in &plan_items {
        let activity = item
            .get("activity")
            .and_then(|v| v.as_str())
            .expect("plan activity id");
        let node = item.get("node").and_then(|v| v.as_str()).expect("plan node");
        let correct_answer = item_answer(item);
        let response = if wrong_nodes.iter().any(|wrong| wrong == node) {
            if is_variant(item) {
                item.pointer("/variant/options")
                    .and_then(|v| v.as_array())
                    .and_then(|options| {
                        options
                            .iter()
                            .map(|option| option.as_str().unwrap_or_default())
                            .find(|option| serde_json::json!(*option) != correct_answer)
                            .map(|option| serde_json::json!(option))
                    })
                    .unwrap_or_else(|| serde_json::json!("__wrong__"))
            } else {
                serde_json::json!("A")
            }
        } else {
            correct_answer
        };
        answers.insert(activity.to_owned(), response);
    }
    let report = service
        .quiz_complete(&owner, &course, &round.round_id, &answers)
        .await?;
    // 结算档位随答题量：full = 2 + 该节点题数（no-provider 每节点 1 题 =
    // 3；变式在场 2 题 = 4），partial = 0。4 full + 2 partial。
    let per_node_items = if variants_present { 2 } else { 1 };
    let full_xp = 2 + per_node_items;
    anyhow::ensure!(
        report.earned_xp == full_xp * 4,
        "4 full + 2 partial must settle {}, got {}",
        full_xp * 4,
        report.earned_xp
    );
    anyhow::ensure!(
        report.covered.iter().all(|node| node.earned_xp.unwrap_or(0) >= 0),
        "quiz settlements are never negative"
    );
    for entry in &report.covered {
        let expected = if wrong_nodes.iter().any(|wrong| *wrong == entry.node_id.to_string()) {
            Some(0)
        } else {
            Some(full_xp)
        };
        anyhow::ensure!(
            entry.earned_xp == expected,
            "tier check for node {}: expected {expected:?}, got {:?}",
            entry.node_id,
            entry.earned_xp
        );
    }
    // 盘面复验：账本与回合行从磁盘重读，对账服务面读数（repair 模式）。
    // 账本按本课程过滤——dev 库会累积历史冒烟课程的行，全局口径只在从零
    // 临时库上成立。
    let (events, ledger_total): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(earned_xp), 0) FROM learning_xp_events \
         WHERE kind = 'quiz-completed' AND course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(&wiring.pool)
    .await?;
    anyhow::ensure!(
        (events, ledger_total) == (k, full_xp * 4),
        "ledger re-read must match the report ({events} events, {ledger_total} xp)"
    );
    let (status, row_earned): (String, i64) = sqlx::query_as(
        "SELECT status, earned_xp FROM learning_quiz_rounds WHERE round_id = ?",
    )
    .bind(&round.round_id)
    .fetch_one(&wiring.pool)
    .await?;
    anyhow::ensure!(
        status == "completed" && row_earned == full_xp * 4,
        "round row must be completed with earned {} (got {status}/{row_earned})", full_xp * 4
    );
    println!("[3/6] settled per node: 4 full ({full_xp} xp each) + 2 partial (0 xp, never negative); ledger re-read matches");

    // 校准读面：弱节点首卡被真实推进过（stability 落值）→ 其校准点带预测
    // R，分桶读面非空（预测均值 vs 实际正确率；读数面非门禁）。
    let calibration = service.quiz_calibration(&owner).await?;
    anyhow::ensure!(
        calibration.iter().any(|bin| bin.count >= 1),
        "calibration bins must carry the pushed card's point"
    );
    println!(
        "[4/6] calibration: {} bin(s), top count {} (predicted R vs actual)",
        calibration.len(),
        calibration.iter().map(|bin| bin.count).max().unwrap_or(0)
    );

    // ④ 同日折叠：第二轮（同计划直插 active）全对作答 → 全折叠零计薪。
    // 复用同一 plan_json（变式在场时含变式判分底）→ 折叠结算全程零 LLM
    // （变式不重铸：判底随计划行流转，不回库不重生成）。
    let plan_json: String = sqlx::query_scalar(
        "SELECT plan_json FROM learning_quiz_rounds WHERE round_id = ?",
    )
    .bind(&round.round_id)
    .fetch_one(&wiring.pool)
    .await?;
    let round_2 = nomifun_common::generate_id();
    sqlx::query(
        "INSERT INTO learning_quiz_rounds \
         (round_id, user_id, course_id, started_at, status, plan_json) \
         VALUES (?, ?, ?, ?, 'active', ?)",
    )
    .bind(&round_2)
    .bind(owner.as_str())
    .bind(&course_id)
    .bind(nomifun_common::now_ms())
    .bind(&plan_json)
    .execute(&wiring.pool)
    .await?;
    let mut all_correct = HashMap::new();
    for item in &plan_items {
        let activity = item
            .get("activity")
            .and_then(|v| v.as_str())
            .expect("plan activity id");
        all_correct.insert(activity.to_owned(), item_answer(item));
    }
    let folded = service
        .quiz_complete(&owner, &course, &round_2, &all_correct)
        .await?;
    anyhow::ensure!(
        folded.earned_xp == 0
            && folded.covered.iter().all(|node| node.earned_xp.is_none()),
        "same-day re-quiz must fold to zero earned with coverage kept"
    );
    let (events_after, total_after): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(earned_xp), 0) FROM learning_xp_events \
         WHERE kind = 'quiz-completed' AND course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(&wiring.pool)
    .await?;
    anyhow::ensure!(
        (events_after, total_after) == (k, full_xp * 4),
        "folded round must not double-pay (got {events_after}/{total_after})"
    );

    // ⑤ 首写为终：重复 complete Conflict。
    let again = service
        .quiz_complete(&owner, &course, &round.round_id, &answers)
        .await
        .unwrap_err();
    anyhow::ensure!(
        matches!(again, nomifun_common::AppError::Conflict(_)),
        "repeat complete must be rejected (first write wins): {again:?}"
    );
    println!("[5/6] same-day fold (zero double-pay) confirmed");
    println!("[6/6] first-write-wins: repeat complete rejected");

    let progress = service.quiz_progress(&owner, &course).await?;
    println!(
        "OK  course={course_id}  settled={} earned={}  progress after quiz: {}/{} due={}",
        report.covered.len(),
        report.earned_xp,
        progress.settled_count,
        progress.trigger_count,
        progress.due
    );
    Ok(ExitCode::SUCCESS)
}

/// `learning-smoke content`（j07 + n3 判据升级）：内容成色冒烟。立一门两
/// 节点学习图课程（概念挂接 + 别名池 + 引用名 + 已会声明锚 + 先行节点两节
/// 正文），为汇合节点（assumes 前置 + teaches 两概念）生成正文——概念面
/// 投影、课程已知概念行（Known Names）、teaches 档位行、先例摘要随真实管
/// 线注入。硬判据（全进 EXIT，learnhub smoke-content 六判据对齐，n2 D7）：
/// ①输入面名单行（两面归档 user 含「课程已知概念」且命中布景概念——
/// stale-binary 必红）②条款在场（规划面四条款 + 写作面两条款）③契约面
/// （管线走通并发布 + est 铸值 == 节点 est）⑤先例行 + 先行节标题。读数
/// （软）：字数/小节数/题目数/首现与用名标注/超纲专名抽查（人读对照卡，
/// 超纲面无机械门）。无凭据临时库走到生成即如实非零。
#[allow(clippy::too_many_arguments)]
async fn run_content(
    cli: &Cli,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
) -> Result<ExitCode> {
    if provider.is_some() != model.is_some() {
        anyhow::bail!("--provider and --model must be provided together");
    }
    let (wiring, archiving, _live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), true).await?;
    let owner = wiring.owner.clone();
    let service = wiring.service.clone();
    let now = nomifun_common::now_ms();
    // 最小两节点课程：L1 教 α；L2 assumes α（引用名 Alpha）teaches β；
    // β 带活跃别名「贝塔系数」。概念面投影的数据源全部真实落库。
    let course_id = nomifun_common::LearningCourseId::new().into_string();
    let module_id = nomifun_common::LearningModuleId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, created_at, updated_at) \
         VALUES (?, '内容成色冒烟', '', 'general', 1, 'learning_graph', \
                 '理解衍生品的基础计量工具', '', ?, ?)",
    )
    .bind(&course_id)
    .bind(now)
    .bind(now)
    .execute(&wiring.pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(&module_id)
    .bind(&course_id)
    .execute(&wiring.pool)
    .await?;
    let lesson_a = nomifun_common::LearningLessonId::new().into_string();
    let lesson_b = nomifun_common::LearningLessonId::new().into_string();
    for (lesson_id, title, purpose, position) in [
        (&lesson_a, "收益率与对数收益率", "建立收益率概念并能相互换算", 0),
        (&lesson_b, "波动率的直观刻画", "用收益分布理解波动率的度量动机", 1),
    ] {
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
              content_generated) VALUES (?, ?, ?, '', ?, ?, 20, 0)",
        )
        .bind(lesson_id)
        .bind(&module_id)
        .bind(title)
        .bind(purpose)
        .bind(position)
        .execute(&wiring.pool)
        .await?;
    }
    let concept_alpha = nomifun_common::LearningConceptId::new().into_string();
    let concept_beta = nomifun_common::LearningConceptId::new().into_string();
    for (concept_id, canonical, aliases, definition) in [
        (
            &concept_alpha,
            "对数收益率",
            serde_json::json!([]),
            "价格比取对数的一阶近似收益度量",
        ),
        (
            &concept_beta,
            "历史波动率",
            serde_json::json!([{"name": "贝塔系数", "status": "active"}]),
            "收益序列离散程度的年化度量",
        ),
    ] {
        sqlx::query(
            "INSERT INTO learning_concept_registry \
             (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(concept_id)
        .bind(canonical)
        .bind(aliases.to_string())
        .bind(definition)
        .bind(now)
        .bind(now)
        .execute(&wiring.pool)
        .await?;
    }
    // 挂接：L1 teaches α；L2 assumes α（引用名 Alpha）+ teaches β。
    for (lesson_id, role, concept_id, tier, ref_name) in [
        (&lesson_a, "teaches", &concept_alpha, "apply", None),
        (&lesson_b, "assumes", &concept_alpha, "apply", Some("Alpha")),
        (&lesson_b, "teaches", &concept_beta, "know", None),
    ] {
        sqlx::query(
            "INSERT INTO learning_lesson_concepts \
             (lesson_id, concept_id, role, tier, ref_name) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(lesson_id)
        .bind(concept_id)
        .bind(role)
        .bind(tier)
        .bind(ref_name)
        .execute(&wiring.pool)
        .await?;
    }
    // 已会声明锚（m2，n3 布景对齐 learnhub 声明档案节）：学习者声明已会 α
    // ——Known Names 的声明半边（∪ 去重：α 同时被 L1 教出）。
    sqlx::query(
        "INSERT INTO learning_basis_declarations \
         (course_id, doors_json, blocks_json, concept_ids_json, declared_at, note) \
         VALUES (?, '[]', '[]', ?, ?, 'content smoke 布景声明')",
    )
    .bind(&course_id)
    .bind(serde_json::json!([concept_alpha]).to_string())
    .bind(now)
    .execute(&wiring.pool)
    .await?;
    // 先行节点正文两节（n3 布景对齐 learnhub 先行节点节）：L1 落两节
    // ready 正文——先例摘要的先行节标题判据面（#180）。
    for (section_key, kind, title, points, position) in [
        (
            "s1",
            "concept",
            "概念：对数收益的定义",
            "价格比取对数的一阶近似",
            0,
        ),
        (
            "s2",
            "example",
            "例题：两日收益的对数换算",
            "相邻两日收益相加等于区间收益",
            1,
        ),
    ] {
        sqlx::query(
            "INSERT INTO learning_lesson_sections \
             (section_key, lesson_id, kind, title, points, visual, body_md, degraded, \
              status, version, position, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, '表格', ?, 0, 'ready', 1, ?, ?, ?)",
        )
        .bind(section_key)
        .bind(&lesson_a)
        .bind(kind)
        .bind(title)
        .bind(points)
        .bind(format!("{title}\n\n布景正文（先行节点，供先例摘要引用节标题）。"))
        .bind(position)
        .bind(now)
        .bind(now)
        .execute(&wiring.pool)
        .await?;
    }
    sqlx::query("UPDATE learning_lessons SET content_generated = 1 WHERE lesson_id = ?")
        .bind(&lesson_a)
        .execute(&wiring.pool)
        .await?;

    println!("[1/2] generating node B content (known names / tier / precedents ride the real pipeline)…");
    let request = nomifun_learning::GenerateLessonRequest {
        provider_id: provider
            .as_deref()
            .map(nomifun_common::ProviderId::parse)
            .transpose()?,
        model: model.clone(),
        feedback: None,
    };
    let lesson_b_id = nomifun_common::LearningLessonId::parse(&lesson_b)?;
    // 硬判据③（契约面，管线半边）= 管线真实走通并发布：生成失败如实非零
    // 退出（无凭据临时库走到这里即报 no enabled provider——structure 冒烟
    // 同纪律）。
    let view = match service
        .generate_lesson_content(&owner, &lesson_b_id, &request)
        .await
    {
        Ok(view) => view,
        Err(error) => {
            eprintln!("content generation FAILED: {error}");
            return Ok(ExitCode::from(2));
        }
    };

    println!("[2/2] hard criteria (D7: input face / clauses / contract / precedents)…");
    let mut failed: Vec<String> = Vec::new();
    // 观测存档行：user 字段 = 输入面（两面装配事实），system 字段 = 条款面。
    let archive = archiving.as_ref().expect("content smoke wires archiving");
    let calls = std::fs::read_to_string(archive.run_dir().join("calls.jsonl"))
        .unwrap_or_default();
    let rows: Vec<serde_json::Value> = calls
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect();
    let user_texts: Vec<&str> = rows
        .iter()
        .filter_map(|value| value.get("user").and_then(|v| v.as_str()))
        .collect();
    let system_texts: Vec<&str> = rows
        .iter()
        .filter_map(|value| value.get("system").and_then(|v| v.as_str()))
        .collect();

    // 判据①：输入面名单行——user 归档含「课程已知概念」行且命中布景概念
    // （声明 ∪ 已教并集；stale-binary 必红：装配面旧档无此行）。
    let known_calls = user_texts
        .iter()
        .filter(|user| {
            user.contains("课程已知概念")
                && user.contains("对数收益率")
                && user.contains("历史波动率")
        })
        .count();
    if known_calls == 0 {
        failed.push(
            "判据1红：归档 user 缺「课程已知概念」名单行或未命中布景概念（对数收益率/历史波动率）——#177 输入面"
                .into(),
        );
    }
    // 判据②：条款在场——规划面四条款 + 写作面两条款（stale-binary 门）。
    let system_joined = system_texts.join("\n");
    for term in [
        "构件词边界",
        "练习形态按档位",
        "取材池",
        "意图画像与容量预算",
        "表述工具在场",
        "规划即雄心",
    ] {
        if !system_joined.contains(term) {
            failed.push(format!("判据2红：system 归档缺条款「{term}」——二进制可能是旧档"));
        }
    }
    // 判据③（铸值半边）：est 铸值 == 节点 est（20；草稿以节点尺寸锚开盘）。
    if view.est != 20 {
        failed.push(format!(
            "判据3红：est={}，应为节点 est 20（铸值面）",
            view.est
        ));
    }
    // 判据⑤：先例摘要 + 先行节标题在 user 归档（#180 输入面）。
    let precedent_calls = user_texts
        .iter()
        .filter(|user| {
            user.contains("已建立先例")
                && user.contains("概念：对数收益的定义")
                && user.contains("收益率与对数收益率")
        })
        .count();
    if precedent_calls == 0 {
        failed.push(
            "判据5红：归档 user 缺先例摘要或先行节标题（概念：对数收益的定义 / 收益率与对数收益率）——#180 输入面"
                .into(),
        );
    }

    // 读数面（非门禁）：字数 / 小节 / 题目 / 首现与用名标注。
    let body_chars: usize = view
        .sections
        .iter()
        .map(|section| section.body_md.chars().filter(|c| !c.is_whitespace()).count())
        .sum();
    let joined: String = view
        .sections
        .iter()
        .map(|section| section.body_md.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    println!(
        "content read-out: sections={} activities={} body_chars={body_chars} est={}",
        view.sections.len(),
        view.activities.len(),
        view.est,
    );
    println!(
        "faces: mentions_log_returns={} mentions_beta_canonical={} mentions_alias={}",
        joined.contains("对数收益率"),
        joined.contains("历史波动率"),
        joined.contains("贝塔系数"),
    );
    // k03 读数面（非门禁）：预算行与超容自报在观测存档里的在场性——双臂
    // 对照的机械读数（off 臂预算行 0）。
    {
        // 预算行按注入原文（"- 时长预算："）在 **user 字段** 计数——系统
        // 提示词里的说明句同样以该词开头，不能整行 grep。
        let budget_line_calls = user_texts
            .iter()
            .filter(|user| user.contains("- 时长预算："))
            .count();
        // 超容自报按发布观测行计数（实际产物，非 schema 提及；缺席合法）。
        let capacity_notes = rows
            .iter()
            .filter(|value| {
                value.get("method").and_then(|v| v.as_str()) == Some("lesson_agent_publish")
            })
            .filter_map(|value| value.get("capacity_note").cloned())
            .filter(|note| !note.is_null())
            .count();
        println!(
            "est_budget read-out: budget_line_calls={budget_line_calls} capacity_note_events={capacity_notes}"
        );
        // k05 读数面（非门禁）：四条纪律短语在存档里的在场性（off 臂 0）。
        let discipline_hits = [
            "具象替身先（R10）",
            "受众语域（R11）",
            "表述工具在场（R12）",
            "断言-例子一致（R13）",
        ]
        .iter()
        .map(|phrase| {
            (
                *phrase,
                calls.lines().filter(|line| line.contains(phrase)).count(),
            )
        })
        .collect::<Vec<_>>();
        println!(
            "content_discipline read-out: {}",
            discipline_hits
                .iter()
                .map(|(phrase, count)| format!("{phrase}={count}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        // n3 读数面（非门禁）：档位行与先例行在场计数（双臂对照）。
        let tier_line_calls = user_texts
            .iter()
            .filter(|user| user.contains("节点档位"))
            .count();
        let known_line_calls = user_texts
            .iter()
            .filter(|user| user.contains("课程已知概念"))
            .count();
        let precedent_line_calls = user_texts
            .iter()
            .filter(|user| user.contains("已建立先例"))
            .count();
        println!(
            "boundary read-out: known_line_calls={known_line_calls} tier_line_calls={tier_line_calls} precedent_line_calls={precedent_line_calls}"
        );
        // 超纲专名抽查（软，人工判读）：正文工作词应取自名单（对数收益率/
        // 历史波动率/贝塔系数），超纲专名（如 布朗运动/伊藤引理/GARCH/维纳
        // 过程）出现 = 读数红，回 #177/#178 续裁（对照卡人读清单）。
        let overshoot = ["布朗运动", "伊藤引理", "GARCH", "维纳过程"]
            .iter()
            .filter(|term| joined.contains(**term))
            .count();
        println!(
            "overshoot read-out: known-domain words only; out-of-bounds proper nouns hit={overshoot} (human card)"
        );
    }
    println!(
        "observation: {}/observation/learning-smoke (ArchivingCompleter + task JSONL)",
        cli.data_dir.display()
    );
    if !failed.is_empty() {
        for failure in &failed {
            eprintln!("{failure}");
        }
        return Ok(ExitCode::FAILURE);
    }
    println!("\n== 判据全过（EXIT=0） ==");
    Ok(ExitCode::SUCCESS)
}

/// `learning-smoke mount`（j08）：结构挂载冒烟。从零库立一门带结构快照的
/// 学习图课程（两块 + 三节点），跑真实挂载回合，机械判据：①落位（状态
/// mounted、三节点全评估）②hash 锚定（挂载行 hash = 结构字节 FNV-1a）
/// ③条数一致；随后**重跑幂等**——待挂集空必须 skipped，否则 EXIT=4。
/// 读数面非门禁；无凭据临时库如实失败（structure 冒烟同纪律）。
async fn run_mount(
    cli: &Cli,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
) -> Result<ExitCode> {
    if provider.is_some() != model.is_some() {
        anyhow::bail!("--provider and --model must be provided together");
    }
    // 与应用同接线（completer 注入是挂载回合的硬依赖）。
    let (wiring, _archiving, _live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), false).await?;
    let owner = wiring.owner.clone();
    let service = wiring.service.clone();
    let pool = wiring.pool.clone();

    // 种子：图课程（结构快照两块）+ 三节点。
    let course_id = nomifun_common::LearningCourseId::new().into_string();
    let module_id = nomifun_common::LearningModuleId::new().into_string();
    let now = nomifun_common::now_ms();
    let structure = serde_json::json!({
        "goal": "挂载冒烟目标",
        "spines": [{ "name": "主干一", "definition": "d", "key_points": ["k"], "instance": "i" }],
        "blocks": [
            { "name": "块一", "slug": "block-01", "spine": "主干一", "stage": "入门", "note": "第一块" },
            { "name": "块二", "slug": "block-02", "spine": "主干一", "stage": "进阶", "note": "第二块" }
        ],
        "edges": []
    });
    let envelope = serde_json::json!({
        "version": 1,
        "generated_at": now,
        "structure": structure,
        "terrain": { "doors": [] },
    });
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, structure_json, created_at, updated_at) \
         VALUES (?, '挂载冒烟', '', 'general', 1, 'learning_graph', '挂载冒烟目标', '', ?, ?, ?)",
    )
    .bind(&course_id)
    .bind(serde_json::to_string(&envelope)?.as_str())
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(&module_id)
    .bind(&course_id)
    .execute(&pool)
    .await?;
    for (position, title) in ["节点一", "节点二", "节点三"].iter().enumerate() {
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes) \
             VALUES (?, ?, ?, '', 'p', ?, 10)",
        )
        .bind(nomifun_common::LearningLessonId::new().into_string())
        .bind(&module_id)
        .bind(title)
        .bind(position as i64)
        .execute(&pool)
        .await?;
    }

    let model_override = match provider.as_deref().zip(model.as_deref()) {
        Some((p, m)) => Some((nomifun_common::ProviderId::parse(p)?, m.to_owned())),
        None => None,
    };
    let override_ref =
        model_override.as_ref().map(|(provider, model)| (provider, model.as_str()));

    println!("[1/2] first mount round (real pipeline)…");
    let first = service
        .run_structure_mount(&owner, &nomifun_common::LearningCourseId::parse(&course_id)?, override_ref)
        .await?;
    println!(
        "mount read-out: status={} pending={} mounted={} outside={} total_entries={} hash={}",
        first.status,
        first.pending_nodes,
        first.mounted_nodes,
        first.outside_nodes,
        first.total_entries,
        first.structure_hash
    );
    anyhow::ensure!(first.status == "mounted", "first round must mount, got {}", first.status);
    anyhow::ensure!(
        first.pending_nodes == 3 && first.mounted_nodes + first.outside_nodes == 3,
        "three nodes must all be evaluated"
    );
    // hash 锚定：挂载行 hash = 结构字节 FNV-1a。
    let structure_raw: String =
        sqlx::query_scalar("SELECT structure_json FROM learning_courses WHERE course_id = ?")
            .bind(&course_id)
            .fetch_one(&pool)
            .await?;
    let (row_hash, mounts_json): (i64, String) = sqlx::query_as(
        "SELECT structure_hash, mounts_json FROM learning_structure_mounts WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(&pool)
    .await?;
    let expected = nomifun_learning::learning_graph_hash(structure_raw.as_bytes());
    anyhow::ensure!(
        row_hash as u64 == expected,
        "mount row hash {} != fnv1a(structure) {expected}",
        row_hash
    );
    let entries: serde_json::Value = serde_json::from_str(&mounts_json)?;
    anyhow::ensure!(
        entries.as_array().is_some_and(|items| items.len() == 3),
        "mount entries must cover all three nodes"
    );

    println!("[2/2] idempotency: second round must be skipped (pending set empty)…");
    let second = service
        .run_structure_mount(&owner, &nomifun_common::LearningCourseId::parse(&course_id)?, override_ref)
        .await?;
    println!(
        "second round: status={} pending={} total_entries={}",
        second.status, second.pending_nodes, second.total_entries
    );
    if second.status != "skipped" {
        eprintln!("IDEMPOTENCY VIOLATION: second round must be skipped, got {}", second.status);
        return Ok(ExitCode::from(4));
    }
    anyhow::ensure!(
        second.total_entries == first.total_entries,
        "skipped round must not touch the mount row"
    );
    println!("OK  course={course_id}");
    Ok(ExitCode::SUCCESS)
}

/// `learning-smoke repair`（j09）：结构修补冒烟。从零库立一门带结构快照的
/// 学习图课程并落一条 findings 留痕，跑真实修补回合（skip/noop 照印），
/// 机械判据：①**盘面复验**——回合后从磁盘重读结构快照，独立跑全量 gd.*
/// validator（零放宽）②**级联标记**——结构已改（old≠new hash）时挂载必
/// 须判 stale，否则 EXIT=3。读数面非门禁；无凭据临时库如实失败。
/// 合法结构种子信封（修补/维护两档共用；t1 修法 1 的 6 主干 × 42 块盘面，
/// 逐项过 gd.* 全量 validator——回合无论 applied 还是 noop 都不会被条款门
/// 误杀）。
fn valid_structure_seed_envelope(goal: &str, now: i64) -> serde_json::Value {
    let spine_names = ["主干一", "主干二", "主干三", "主干四", "主干五", "主干六"];
    let stages = ["入门", "进阶", "高级"];
    let mut blocks = Vec::new();
    for (spine_index, spine) in spine_names.iter().enumerate() {
        for block_index in 0..7 {
            let ordinal = spine_index * 7 + block_index;
            blocks.push(serde_json::json!({
                "name": format!("块{ordinal:02}"),
                "slug": format!("block-{ordinal:02}"),
                "spine": spine,
                "stage": stages[ordinal % stages.len()],
                "note": format!("第 {ordinal} 个主题的一句话描述"),
            }));
        }
    }
    let structure = serde_json::json!({
        "goal": goal,
        "goal_type": "capability",
        "proficiency_statement": "达成后能独立复述本结构的修补判据",
        "domain_type": "理论",
        "form_reference": "Flowy 学习结构模板 v1",
        "source_tier": "名录",
        "generated_by": "learning-smoke-seed",
        "disclaimer": "冒烟种子为静态合法盘面，仅供回合验证",
        "domain": {
            "core_definition": "冒烟课程的合法结构基线",
            "includes": ["结构判据", "修补操作", "留痕通道"],
            "excludes": ["真实课程内容", "学习者数据", "生产库"],
            "prerequisite_doors": ["G1"],
            "audience": "冒烟操作者",
            "scenarios": ["回合验证", "盘面复验"],
            "granularity": "每块一个可独立教学的主题",
        },
        "spines": spine_names.iter().map(|spine| serde_json::json!({
            "name": spine,
            "definition": format!("{spine}的一句话定义"),
            "key_points": ["要点一"],
            "instance": format!("达标{spine}时能独立完成一次复述"),
        })).collect::<Vec<_>>(),
        "blocks": blocks,
        "edges": [
            { "source": "块01", "target": "块00", "type": "assumes", "tier": "know", "note": "先认后用" },
            { "source": "块08", "target": "块09", "type": "composes", "tier": "use", "note": "组装" },
            { "source": "块15", "target": "块16", "type": "contrasts", "tier": "know", "note": "对照" },
        ],
        "gaps": [
            { "item": "应用情境覆盖不足", "direction": "补情境块" },
            { "item": "图像法未展开", "direction": "补图像判读" },
            { "item": "近似解法缺位", "direction": "补估算策略" },
        ],
        "quality": [
            { "check": "主干数在 6–12 档", "status": "met" },
            { "check": "块名一物一名", "status": "met" },
            { "check": "边端点全部可解析", "status": "met" },
            { "check": "无顺序承诺", "status": "met" },
            { "check": "盲区已标注", "status": "met" },
        ],
    });
    serde_json::json!({
        "version": 1,
        "generated_at": now,
        "structure": structure,
        "terrain": {
            "goal": goal,
            "learner_basis_assumed": "已具备结构判据的基线认识",
            "generated_by": "learning-smoke-seed",
            "disclaimer": "冒烟种子地形，仅供门口对账",
            "doors": [
                { "id": "G1", "criterion": "能读懂结构快照的块与边", "kind": "hard" },
            ],
        },
    })
}

async fn run_repair(
    cli: &Cli,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
) -> Result<ExitCode> {
    if provider.is_some() != model.is_some() {
        anyhow::bail!("--provider and --model must be provided together");
    }
    let (wiring, _archiving, _live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), false).await?;
    let owner = wiring.owner.clone();
    let service = wiring.service.clone();
    let pool = wiring.pool.clone();

    // 种子：全量合法 gd.* 盘面（t1 修法 1——对齐 structure_contract 金样形状：
    // 6 主干 × 42 块、八标量键齐、gaps/quality 齐、门口 G1 地形对账）+ 一条
    // findings 留痕（触发门槛）。旧的最小种子（2 块、零 frontmatter）从两条
    // 路都是死路：非空 ops 应用后全量复核必拒、noop 盘面原样复验必拒——种
    // 子合法化后 applied（修补 op 应用后仍合法）与 noop（盘面没动）都可行。
    let course_id = nomifun_common::LearningCourseId::new().into_string();
    let now = nomifun_common::now_ms();
    let envelope = valid_structure_seed_envelope("修补冒烟目标", now);
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, structure_json, created_at, updated_at) \
         VALUES (?, '修补冒烟', '', 'general', 1, 'learning_graph', '修补冒烟目标', '', ?, ?, ?)",
    )
    .bind(&course_id)
    .bind(serde_json::to_string(&envelope)?.as_str())
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_structure_findings \
         (ts, course_id, source, node, field, issue, advice) \
         VALUES (?, ?, 'concept-review', '', '', '冒烟留痕：结构缺少一条对照边', '补一条 contrasts 边连接两块')",
    )
    .bind(now)
    .bind(&course_id)
    .execute(&pool)
    .await?;

    let model_override = match provider.as_deref().zip(model.as_deref()) {
        Some((p, m)) => Some((nomifun_common::ProviderId::parse(p)?, m.to_owned())),
        None => None,
    };
    let override_ref =
        model_override.as_ref().map(|(provider, model)| (provider, model.as_str()));

    println!("[1/2] repair round (real pipeline)…");
    let course = nomifun_common::LearningCourseId::parse(&course_id)?;
    let first = service.run_structure_repair(&owner, &course, override_ref).await?;
    println!(
        "repair read-out: status={} applied={} cascade={} mounts_stale={} old={} new={}",
        first.status,
        first.applied.len(),
        first.cascade.len(),
        first.mounts_stale,
        first.old_hash,
        first.new_hash
    );
    anyhow::ensure!(
        first.status == "applied" || first.status == "noop",
        "first round must run (applied | noop), got {}",
        first.status
    );

    println!("[2/2] disk re-read + independent full gd.* validation + cascade marker…");
    // 盘面复验：从磁盘重读快照信封，独立跑全量 gd.* validator（零放宽）。
    let disk_raw: String =
        sqlx::query_scalar("SELECT structure_json FROM learning_courses WHERE course_id = ?")
            .bind(&course_id)
            .fetch_one(&pool)
            .await?;
    let envelope: serde_json::Value = serde_json::from_str(&disk_raw)?;
    let structure = envelope
        .get("structure")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("snapshot envelope missing structure"))?;
    let terrain = envelope
        .get("terrain")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("snapshot envelope missing terrain"))?;
    let mut violations = nomifun_learning::validate_structure_artifact(&structure);
    violations.extend(nomifun_learning::reconcile_structure_doors(&structure, &terrain));
    let red: Vec<_> = violations.iter().filter(|violation| !violation.warning).collect();
    for violation in &red {
        eprintln!("CRITERION VIOLATION [{}] {}", violation.code, violation.message);
    }
    anyhow::ensure!(red.is_empty(), "disk re-read failed full gd.* validation");

    // 级联标记：结构已改时挂载必须判 stale（读侧 hash 比对单点兜住）。
    let new_hash = nomifun_learning::learning_graph_hash(disk_raw.as_bytes());
    if first.old_hash != first.new_hash {
        let view = service.structure_view(&course).await?;
        let stale = view.mounts.as_ref().map(|mounts| mounts.stale).unwrap_or(false);
        anyhow::ensure!(
            stale,
            "CASCADE MARKER VIOLATION: structure changed but mounts not stale"
        );
        println!("cascade marker: mounts correctly flagged stale");
    } else if first.status == "applied" {
        eprintln!("CASCADE MARKER VIOLATION: applied round must change the hash");
        return Ok(ExitCode::from(3));
    }
    let _ = new_hash;
    println!("OK  course={course_id}");
    Ok(ExitCode::SUCCESS)
}

/// `learning-smoke maintenance`（k02）：自动维护回合冒烟。种子 = 一门合法盘面
/// 图课程（两条 findings 留痕、无挂载行）+ 一门归档课（enabled = 0，同带留痕）。
/// 机械判据：
/// ① 纯读 needs 判定正确（未消费 2、待挂集非空、stale）；
/// ② 回合 1 消费留痕并推进水位（applied/noop —— 依赖凭据，无凭据如实非零）；
/// ③ 回合 2 零工作：idle = 1、零课程行动、观测存档零新增 LLM 行；
/// ④ 归档课不在扫描面（scanned 只含启用课，归档课 eligible = false）；
/// ⑤ 设置两键读侧生效（写入后 maintenance_settings 反射）。
async fn run_maintenance(
    cli: &Cli,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
) -> Result<ExitCode> {
    if provider.is_some() != model.is_some() {
        anyhow::bail!("--provider and --model must be provided together");
    }
    // 挂载回合可能被 tick 触发：需要 completer 链（archiving 供零 LLM 判据）。
    let (wiring, archiving, _live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), true).await?;
    let owner = wiring.owner.clone();
    let service = wiring.service.clone();
    let pool = wiring.pool.clone();
    let now = nomifun_common::now_ms();

    // 种子 A：启用课（合法盘面 + 2 条留痕 + 无挂载行）。
    let course_id = nomifun_common::LearningCourseId::new().into_string();
    let envelope = valid_structure_seed_envelope("维护冒烟目标", now);
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, structure_json, created_at, updated_at) \
         VALUES (?, '维护冒烟', '', 'general', 1, 'learning_graph', '维护冒烟目标', '', ?, ?, ?)",
    )
    .bind(&course_id)
    .bind(serde_json::to_string(&envelope)?.as_str())
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await?;
    let module_id = nomifun_common::LearningModuleId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(&module_id)
    .bind(&course_id)
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_lessons \
         (lesson_id, module_id, title, summary, purpose, position, estimated_minutes) \
         VALUES (?, ?, '维护节点', '', 'p', 0, 15)",
    )
    .bind(nomifun_common::LearningLessonId::new().into_string())
    .bind(&module_id)
    .execute(&pool)
    .await?;
    for (index, issue) in ["留痕一：缺一条对照边", "留痕二：块粒度偏粗"].iter().enumerate() {
        sqlx::query(
            "INSERT INTO learning_structure_findings \
             (ts, course_id, source, node, field, issue, advice) \
             VALUES (?, ?, 'concept-review', '', '', ?, '按建议修补')",
        )
        .bind(now + index as i64)
        .bind(&course_id)
        .bind(*issue)
        .execute(&pool)
        .await?;
    }

    // 种子 B：归档课（enabled = 0）——同带留痕，必须不在扫描面。
    let archived_id = nomifun_common::LearningCourseId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, structure_json, enabled, created_at, updated_at) \
         VALUES (?, '归档维护课', '', 'general', 1, 'learning_graph', '维护冒烟目标', '', ?, 0, ?, ?)",
    )
    .bind(&archived_id)
    .bind(serde_json::to_string(&envelope)?.as_str())
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_structure_findings \
         (ts, course_id, source, node, field, issue, advice) \
         VALUES (?, ?, 'concept-review', '', '', '归档课留痕', '不应被扫')",
    )
    .bind(now)
    .bind(&archived_id)
    .execute(&pool)
    .await?;

    // ⑤ 设置两键读侧生效。
    for (key, value) in [
        ("learning.maintenanceAutoEnabled", "true"),
        ("learning.maintenanceIntervalMinutes", "45"),
    ] {
        sqlx::query(
            "INSERT INTO client_preferences (key, value, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .bind(now)
        .execute(&pool)
        .await?;
    }
    let settings = service.maintenance_settings().await;
    println!(
        "settings read-out: enabled={} interval_minutes={}",
        settings.enabled, settings.interval_minutes
    );
    anyhow::ensure!(settings.enabled, "maintenanceAutoEnabled must read back true");
    anyhow::ensure!(
        settings.interval_minutes == 45,
        "maintenanceIntervalMinutes must read back 45, got {}",
        settings.interval_minutes
    );

    // ① 纯读 needs 判定（零 LLM）。
    let course = nomifun_common::LearningCourseId::parse(&course_id)?;
    let archived = nomifun_common::LearningCourseId::parse(&archived_id)?;
    let needs = service.maintenance_needs(&course).await?;
    println!(
        "needs read-out: eligible={} unconsumed_findings={} pending_mounts={} mounts_stale={} needs_repair={} needs_mount={}",
        needs.eligible,
        needs.unconsumed_findings,
        needs.pending_mounts,
        needs.mounts_stale,
        needs.needs_repair,
        needs.needs_mount
    );
    anyhow::ensure!(needs.eligible, "seeded course must be eligible");
    anyhow::ensure!(
        needs.unconsumed_findings == 2,
        "expected 2 unconsumed findings, got {}",
        needs.unconsumed_findings
    );
    anyhow::ensure!(needs.mounts_stale, "no mount row = stale");
    anyhow::ensure!(needs.needs_repair && needs.needs_mount);
    let archived_needs = service.maintenance_needs(&archived).await?;
    anyhow::ensure!(!archived_needs.eligible, "archived course must not be eligible");

    // ② 回合 1（真管线；无凭据如实非零——mount/repair 档先例）。
    println!("[1/2] maintenance tick (real pipeline)…");
    let first = match service.run_maintenance_tick(&owner, None).await {
        Ok(report) => report,
        Err(error) => {
            eprintln!("maintenance tick FAILED: {error}");
            return Ok(ExitCode::from(2));
        }
    };
    println!(
        "tick read-out: scanned={} idle={} worked={}",
        first.scanned,
        first.idle,
        first.courses.len()
    );
    // m7 任务资源闸读数（对照卡数据源）：tick 提交的 mount/repair 过闸占并发
    // 账。判据形态——on 臂 running_peak ≤ limit 恒成立；off 臂 queued 恒 0、
    // running 可超 limit（直跑形态读数）。
    let gate = service.generation_gate_readout();
    println!(
        "gate read-out: enabled={} limit={} running={} running_peak={} queued={} queued_peak={}",
        gate.enabled,
        gate.limit,
        gate.running,
        gate.running_peak,
        gate.queued,
        gate.queued_peak
    );
    if gate.enabled {
        anyhow::ensure!(
            gate.running_peak <= gate.limit,
            "gated arm running_peak must stay within the cap (got {} > {})",
            gate.running_peak,
            gate.limit
        );
    } else {
        anyhow::ensure!(
            gate.queued_peak == 0,
            "off arm must never queue (queued_peak={})",
            gate.queued_peak
        );
    }
    anyhow::ensure!(
        first.scanned == 1,
        "archived course must not be scanned (scanned={})",
        first.scanned
    );
    anyhow::ensure!(
        first.courses.len() == 1,
        "expected one course to be worked, got {}",
        first.courses.len()
    );
    let outcome = &first.courses[0];
    println!(
        "course read-out: repair={:?} mount={:?} skipped_busy={} error={:?}",
        outcome.repair, outcome.mount, outcome.skipped_busy, outcome.error
    );
    if let Some(error) = outcome.error.as_deref() {
        eprintln!("maintenance round FAILED for course: {error}");
        return Ok(ExitCode::from(2));
    }
    anyhow::ensure!(
        outcome.error.is_none(),
        "first tick must not record an error"
    );
    anyhow::ensure!(
        outcome.repair.as_deref() == Some("applied") || outcome.repair.as_deref() == Some("noop"),
        "repair must run to applied|noop, got {:?}",
        outcome.repair
    );
    let watermark: Option<i64> = sqlx::query_scalar(
        "SELECT findings_watermark FROM learning_maintenance_state WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_optional(&pool)
    .await?;
    // 开关臂分支：on 臂 = 水位推进 + 二回合零工作；off 臂 = 不推水位 +
    // 二回合全量重读重跑 LLM（旧行为臂，两种终态各自成立）。
    let watermark_enabled = std::env::var(nomifun_learning::FINDINGS_WATERMARK_SWITCH_ENV)
        .map(|value| !nomifun_learning::is_findings_watermark_disabled_value(&value))
        .unwrap_or(true);
    println!("watermark read-out: enabled={watermark_enabled} value={watermark:?}");
    if watermark_enabled {
        anyhow::ensure!(
            watermark == Some(2),
            "watermark must advance to the max consumed finding id (got {watermark:?})"
        );
    } else {
        anyhow::ensure!(
            watermark.is_none(),
            "off arm must not advance the watermark (got {watermark:?})"
        );
    }

    // ③ 回合 2：on 臂零工作零 LLM 零写；off 臂重读同一批留痕并重跑 LLM。
    println!("[2/2] second tick (on: expect idle; off: expect re-read)…");
    let archive_lines = |archiving: &Option<Arc<ArchivingCompleter>>| {
        archiving
            .as_ref()
            .and_then(|completer| {
                std::fs::read_to_string(completer.run_dir().join("calls.jsonl")).ok()
            })
            .map(|content| content.lines().count())
            .unwrap_or(0)
    };
    let before = archive_lines(&archiving);
    let second = service.run_maintenance_tick(&owner, None).await?;
    let after = archive_lines(&archiving);
    println!(
        "tick2 read-out: scanned={} idle={} worked={} llm_calls_before={before} llm_calls_after={after}",
        second.scanned,
        second.idle,
        second.courses.len()
    );
    // m7 闸读数（收尾行）：两回合全部退出后在跑/排队应归零（许可不随异常
    // 退出泄漏的 smoke 面投影）。
    let gate_after = service.generation_gate_readout();
    println!(
        "gate read-out after tick2: running={} running_peak={} queued={} queued_peak={}",
        gate_after.running,
        gate_after.running_peak,
        gate_after.queued,
        gate_after.queued_peak
    );
    anyhow::ensure!(
        gate_after.running == 0 && gate_after.queued == 0,
        "gate must drain after ticks (running={} queued={})",
        gate_after.running,
        gate_after.queued
    );
    if watermark_enabled {
        anyhow::ensure!(
            second.courses.is_empty(),
            "second tick must be zero-work (got {} courses)",
            second.courses.len()
        );
        anyhow::ensure!(second.idle >= 1, "second tick must count the course as idle");
        anyhow::ensure!(
            after == before,
            "second tick must issue zero LLM calls ({before} -> {after})"
        );
    } else {
        // off 臂：留痕未消费 → 二回合重新工作并重跑 LLM（全量重读旧行为）。
        anyhow::ensure!(
            second.courses.len() == 1,
            "off arm second tick must re-work the course (got {})",
            second.courses.len()
        );
        anyhow::ensure!(
            after > before,
            "off arm second tick must re-run the model ({before} -> {after})"
        );
    }
    let watermark_after: Option<i64> = sqlx::query_scalar(
        "SELECT findings_watermark FROM learning_maintenance_state WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_optional(&pool)
    .await?;
    anyhow::ensure!(
        watermark_after == watermark,
        "second tick must not write the watermark"
    );

    println!("OK  course={course_id} archived={archived_id}");
    Ok(ExitCode::SUCCESS)
}

/// `learning-smoke basis`（m2，ADR-0032 移植）：已会声明冒烟。种子 = 一门带
/// 地形门口（G1/G2）与 door_refs 标注块的图课程 + 控制节点（无 assumes，恒
/// 就绪）。机械判据：
/// ① off 臂：`NOMI_LEARNING_BASIS_DECLARATION=off` 时声明端点如实拒绝（零 LLM）；
/// ② 声明（真管线）：登记表增行且**零 learning_lessons / growth_batches
///    写入**（计数守恒——只铸概念不落节点）、锚行落库、声明概念入概念表读侧；
/// ③ 下游就绪投影翻转：assume 声明概念的节点由未就绪翻就绪（控制节点恒就绪）；
/// ④ 幂等重跑：noop、零 LLM（观测存档行数守恒）；
/// ⑤ 撤销：锚行清零、投影复原、概念留池（来源标记保留不抹）。
/// 无凭据时步骤 ② 的声明调用如实非零（mount/repair 档先例）。
async fn run_basis(
    cli: &Cli,
    db: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
) -> Result<ExitCode> {
    if provider.is_some() != model.is_some() {
        anyhow::bail!("--provider and --model must be provided together");
    }
    // 声明走真管线（需要 LLM）；archiving 供零 LLM 判据与调用档案。
    let (wiring, archiving, _live) =
        wire(cli, db.as_deref(), provider.as_deref(), model.as_deref(), true).await?;
    let owner = wiring.owner.clone();
    let service = wiring.service.clone();
    let pool = wiring.pool.clone();
    let now = nomifun_common::now_ms();

    // 种子：图课程（地形门口 G1/G2 + 块 door_refs）+ 控制节点。
    let course_id = nomifun_common::LearningCourseId::new().into_string();
    let envelope = serde_json::json!({
        "version": 1,
        "generated_at": now,
        "structure": {
            "goal": "已会声明冒烟目标",
            "goal_type": "capability",
            "proficiency_statement": "能独立复述声明判据",
            "domain": {
                "core_definition": "声明冒烟",
                "includes": ["判读"],
                "excludes": [],
                "prerequisite_doors": ["G1", "G2"],
                "audience": "冒烟操作者",
                "scenarios": ["声明"],
                "granularity": "主题级",
            },
            "spines": [
                { "name": "主干一", "definition": "d", "key_points": ["k"], "instance": "i" }
            ],
            "blocks": [
                { "name": "块甲", "slug": "block-a", "spine": "主干一", "stage": "入门", "note": "第一块", "door_refs": ["G1"] },
                { "name": "块乙", "slug": "block-b", "spine": "主干一", "stage": "入门", "note": "第二块", "door_refs": ["G1", "G2"] }
            ],
            "edges": []
        },
        "terrain": {
            "learner_basis_assumed": "已具备基础判读",
            "disclaimer": "冒烟种子地形，仅供门口对账",
            "doors": [
                { "id": "G1", "criterion": "判据一", "kind": "hard" },
                { "id": "G2", "criterion": "判据二", "kind": "self_check" }
            ]
        },
    });
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, structure_json, created_at, updated_at) \
         VALUES (?, '声明冒烟', '', 'general', 1, 'learning_graph', '已会声明冒烟目标', '', ?, ?, ?)",
    )
    .bind(&course_id)
    .bind(serde_json::to_string(&envelope)?)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await?;
    let module_id = nomifun_common::LearningModuleId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(&module_id)
    .bind(&course_id)
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_lessons \
         (lesson_id, module_id, title, summary, purpose, position, estimated_minutes) \
         VALUES (?, ?, '控制节点', '', 'p', 0, 10)",
    )
    .bind(nomifun_common::LearningLessonId::new().into_string())
    .bind(&module_id)
    .execute(&pool)
    .await?;

    let course = nomifun_common::LearningCourseId::parse(&course_id)?;
    let llm_lines = || {
        archiving
            .as_ref()
            .and_then(|completer| {
                std::fs::read_to_string(completer.run_dir().join("calls.jsonl")).ok()
            })
            .map(|content| content.lines().count())
            .unwrap_or(0)
    };

    // ① off 臂：开关 off → 声明端点如实拒绝（零 LLM 零写）。
    println!("[1/5] off arm: switch rejects the declare endpoint…");
    let saved = std::env::var(nomifun_learning::BASIS_DECLARATION_SWITCH_ENV).ok();
    unsafe {
        std::env::set_var(nomifun_learning::BASIS_DECLARATION_SWITCH_ENV, "off");
    }
    let off_error = service
        .declare_basis(
            &owner,
            &course,
            nomifun_learning::DeclareBasisRequest {
                doors: vec!["G1".into()],
                provider_id: None,
                model: None,
            },
        )
        .await
        .err();
    match saved {
        Some(value) => unsafe {
            std::env::set_var(nomifun_learning::BASIS_DECLARATION_SWITCH_ENV, value)
        },
        None => unsafe {
            std::env::remove_var(nomifun_learning::BASIS_DECLARATION_SWITCH_ENV)
        },
    }
    let Some(off_error) = off_error else {
        anyhow::bail!("off arm must reject the declare endpoint");
    };
    anyhow::ensure!(
        off_error.to_string().contains("已会声明通道已关闭"),
        "off arm must reject loudly, got {off_error}"
    );
    let anchors_after_off: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_basis_declarations")
            .fetch_one(&pool)
            .await?;
    anyhow::ensure!(anchors_after_off == 0, "off arm must write nothing");
    println!("  off arm rejected: {off_error}");

    // ② 声明（真管线；无凭据如实非零）。
    println!("[2/5] declare G1+G2 (real pipeline)…");
    let registry_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
            .fetch_one(&pool)
            .await?;
    let (lessons_before, batches_before) = node_and_batch_counts(&pool, &course_id).await?;
    let declared = match service
        .declare_basis(
            &owner,
            &course,
            nomifun_learning::DeclareBasisRequest {
                doors: vec!["G1".into(), "G2".into()],
                provider_id: provider
                    .as_deref()
                    .map(nomifun_common::ProviderId::parse)
                    .transpose()?,
                model: model.clone(),
            },
        )
        .await
    {
        Ok(declared) => declared,
        Err(error) => {
            // 无凭据：声明调用如实非零（判据面 = 管线不可用不被藏起来）。
            eprintln!("declare FAILED (no credentials?): {error}");
            return Ok(ExitCode::from(2));
        }
    };
    println!(
        "declare read-out: doors={:?} blocks={:?} minted={} reused={} noop={}",
        declared.doors, declared.blocks, declared.minted, declared.reused, declared.noop
    );
    anyhow::ensure!(!declared.noop, "first declare must not be noop");
    anyhow::ensure!(
        !declared.doors.is_empty() && !declared.blocks.is_empty(),
        "first declare must hit doors and blocks"
    );
    anyhow::ensure!(
        !declared.concepts.is_empty(),
        "declare must mint or reuse at least one concept"
    );
    let registry_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
            .fetch_one(&pool)
            .await?;
    anyhow::ensure!(
        registry_after > registry_before,
        "registry must grow ({registry_before} -> {registry_after})"
    );
    let (lessons_after, batches_after) = node_and_batch_counts(&pool, &course_id).await?;
    anyhow::ensure!(
        (lessons_before, batches_before) == (lessons_after, batches_after)
            && lessons_after == 1
            && batches_after == 0,
        "zero node/batch writes required (lessons {lessons_before}->{lessons_after}, batches {batches_before}->{batches_after})"
    );
    let anchors: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_basis_declarations WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(&pool)
    .await?;
    anyhow::ensure!(anchors == 1, "one anchor row expected, got {anchors}");
    // 声明概念入概念表读侧（来源徽标 + declared 旗标）。
    let concept_rows = service.graph_concepts(&course, &owner).await?;
    anyhow::ensure!(
        !concept_rows.is_empty() && concept_rows.iter().all(|row| row.declared),
        "declared concepts must surface in the concept table read-side"
    );
    // 观测存档：声明调用档案在场。判据断在铸造系统提示词的立场句
    // （DECLARATION_SYSTEM，声明调用独有）上——任务字面量 `basis-declaration`
    // 到不了存档：它是观测面注册表键（PromptSpec::task），装配
    // （structure_prompts::assemble）剥离携带它的头注后只发正文；Archiver
    // 存档行只记方法名与模型面字段（system/user/response），LearningCompleter
    // 特征不带任务上下文，声明又是同步端点（不进生成任务注册表，
    // learning-tasks.jsonl 的 begin/finish 观测只覆盖长任务）——该字面量
    // 按实现恒不在场（真机 m2 对照卡缺口①），故以系统提示词签名为判据
    // （按 system 字段逐行计，k03 读数面同款纪律）。
    if let Some(archive) = archiving.as_ref() {
        let calls = std::fs::read_to_string(archive.run_dir().join("calls.jsonl"))
            .unwrap_or_default();
        let declaration_calls = calls
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|value| {
                value
                    .get("system")
                    .and_then(|system| system.as_str())
                    .is_some_and(|system| system.contains(nomifun_learning::DECLARATION_SYSTEM))
            })
            .count();
        anyhow::ensure!(
            declaration_calls >= 1,
            "observation archive must contain the declaration mint call (system = DECLARATION_SYSTEM)"
        );
        println!("archive read-out: declaration_calls={declaration_calls}");
    }

    // ③ 下游就绪投影翻转：assume 声明概念的节点由未就绪翻就绪。
    println!("[3/5] downstream readiness flips via the declared concept…");
    let minted_id = declared
        .concepts
        .iter()
        .find(|concept| concept.minted)
        .map(|concept| concept.concept_id.clone())
        .unwrap_or_else(|| declared.concepts[0].concept_id.clone());
    let downstream_id = nomifun_common::LearningLessonId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_lessons \
         (lesson_id, module_id, title, summary, purpose, position, estimated_minutes) \
         VALUES (?, ?, '下游节点', '', 'assumes the declared concept', 1, 10)",
    )
    .bind(&downstream_id)
    .bind(&module_id)
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO learning_lesson_concepts (lesson_id, concept_id, role, tier) \
         VALUES (?, ?, 'assumes', 'know')",
    )
    .bind(&downstream_id)
    .bind(&minted_id)
    .execute(&pool)
    .await?;
    let detail = service.course_detail(&course, Some(&owner)).await?;
    let recommended: Vec<String> = detail
        .graph
        .as_ref()
        .map(|graph| {
            graph
                .recommended
                .iter()
                .map(|lesson| lesson.as_str().to_owned())
                .collect()
        })
        .unwrap_or_default();
    anyhow::ensure!(
        recommended.iter().any(|lesson| lesson == &downstream_id),
        "downstream node must be ready after declaration (recommended={recommended:?})"
    );

    // ④ 幂等重跑：noop、零 LLM。
    println!("[4/5] idempotent rerun (noop, zero LLM)…");
    let calls_before = llm_lines();
    let rerun = service
        .declare_basis(
            &owner,
            &course,
            nomifun_learning::DeclareBasisRequest {
                doors: vec!["G1".into(), "G2".into()],
                provider_id: None,
                model: None,
            },
        )
        .await?;
    let calls_after = llm_lines();
    println!(
        "rerun read-out: noop={} llm_calls {calls_before}->{calls_after}",
        rerun.noop
    );
    anyhow::ensure!(rerun.noop, "rerun must be noop");
    anyhow::ensure!(
        rerun.doors.is_empty() && rerun.concepts.is_empty(),
        "noop rerun must carry no doors/concepts"
    );
    anyhow::ensure!(
        calls_after == calls_before,
        "noop rerun must issue zero LLM calls ({calls_before} -> {calls_after})"
    );

    // ⑤ 撤销：锚行清零、投影复原、概念留池。
    println!("[5/5] revoke (anchors deleted, projection restored, concepts kept)…");
    let revoked = service.revoke_basis(&owner, &course).await?;
    anyhow::ensure!(revoked == 1, "revoke must delete the one anchor row");
    let anchors_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_basis_declarations WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(&pool)
    .await?;
    anyhow::ensure!(anchors_after == 0, "anchors must be gone after revoke");
    let registry_final: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
            .fetch_one(&pool)
            .await?;
    anyhow::ensure!(
        registry_final == registry_after,
        "concepts must stay in the registry after revoke（声明可撤、概念不灭）"
    );
    let source_marks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_concept_registry WHERE source = 'basis-declaration'",
    )
    .fetch_one(&pool)
    .await?;
    anyhow::ensure!(
        source_marks >= 1,
        "declaration source marks must be preserved"
    );
    let detail = service.course_detail(&course, Some(&owner)).await?;
    let recommended: Vec<String> = detail
        .graph
        .as_ref()
        .map(|graph| {
            graph
                .recommended
                .iter()
                .map(|lesson| lesson.as_str().to_owned())
                .collect()
        })
        .unwrap_or_default();
    anyhow::ensure!(
        !recommended.iter().any(|lesson| lesson == &downstream_id),
        "downstream node must lose readiness after revoke (projection restored)"
    );
    // 幂等空撤销。
    let revoked_again = service.revoke_basis(&owner, &course).await?;
    anyhow::ensure!(revoked_again == 0, "second revoke must be a no-op");

    println!("OK  course={course_id}");
    Ok(ExitCode::SUCCESS)
}

/// 课程内节点数与生长批行数（计数守恒判据的读数）。
async fn node_and_batch_counts(
    pool: &nomifun_db::sqlx::SqlitePool,
    course_id: &str,
) -> anyhow::Result<(i64, i64)> {
    let lessons: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lessons l \
         JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
    )
    .bind(course_id)
    .fetch_one(pool)
    .await?;
    let batches: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_growth_batches WHERE course_id = ?")
            .bind(course_id)
            .fetch_one(pool)
            .await?;
    Ok((lessons, batches))
}
