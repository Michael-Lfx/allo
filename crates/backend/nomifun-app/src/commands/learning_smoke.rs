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

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;

use nomifun_ai_agent::archiving_completer::ArchivingCompleter;
use nomifun_ai_agent::LiveLearningCompleter;
use nomifun_common::UserId;
use nomifun_db::sqlx;
use nomifun_db::{IUserRepository, SqliteProviderModelRepository, SqliteProviderRepository, SqliteUserRepository};
use nomifun_learning::{LearningCompleter, LearningService};

use crate::cli::{Cli, LearningSmokeCommand};

pub async fn run_learning_smoke(cli: &Cli, command: LearningSmokeCommand) -> Result<ExitCode> {
    match command {
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
        provider_repo: provider_repo as Arc<dyn nomifun_db::IProviderRepository>,
        provider_model_repo,
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
    service.set_generation_dependencies(knowledge_service, pipeline);

    Ok((
        SmokeWiring {
            service,
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
/// - 首批 assumes 全空（空登记表机械地板）；
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
        let stats = wiring.service.growth_run_stats();
        match stats {
            Some(stats) => println!(
                "  round {round}: batch #{seq}, {nodes} nodes, criteria green  \
                 [retry: attempts_used={}, stop_reason={}]",
                stats.attempts_used, stats.stop_reason
            ),
            None => println!("  round {round}: batch #{seq}, {nodes} nodes, criteria green"),
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

/// 批次机械判据审计（s1）：首批 assumes 全空 + 就绪即发布（既往供给覆盖）
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
    if seq == 1 && !batch_assumes.is_empty() {
        violations.push(format!(
            "首批 assumes 必须全空（空登记表机械地板），发现 {} 条 assume 引用",
            batch_assumes.len()
        ));
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
