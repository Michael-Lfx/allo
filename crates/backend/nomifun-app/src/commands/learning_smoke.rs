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
