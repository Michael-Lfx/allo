//! 别名记录与登记单点（j04 = 二轮 g3 机制 + 三轮 g2 数据形态修订）。
//!
//! - [`AliasRecord`]：`aliases_json` 元素的轻量结构化记录
//!   `{name, definition?, attestation?, status}`——learnhub 六字段中
//!   lang/kind/display_priority 在 allo 无消费方，不进（留演进空间）。
//!   读侧**双读**兼容旧字符串形状（迁移前备份等）；写侧恒为新形状。
//! - [`register_alias_tx`]：**登记单点**——全仓唯一合法的事后加名路径，
//!   四保证照 learnhub：宿主存在且非废弃 / 名字全库唯一（活跃占池、退役
//!   不占）/ 幂等（已含活跃同名 = 跳过）/ 复活（同概念退役名 = Reactivated
//!   并按本次提名刷新定义面）。
//! - [`AliasSpec::projection`]：落池动作的构造式——definition = 提名定义、
//!   attestation = 提名出处、status = active。

use sqlx::{Row, Sqlite, Transaction};

use nomifun_common::AppError;

/// 别名记录（`aliases_json` 元素的新形状）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AliasRecord {
    pub name: String,
    /// 该名字的意义（投影角度定义）；`None` = 与概念定义同义。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    /// 名字出处（提名的 source）；`None` = 出处未知。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation: Option<String>,
    /// active | retired（退役不占名池）。
    pub status: &'static str,
}

pub const ALIAS_ACTIVE: &str = "active";
pub const ALIAS_RETIRED: &str = "retired";

/// 解析 `aliases_json`（双读：新形状对象 / 旧形状字符串——旧字符串回填为
/// `{name, status: active}`）。坏元素（非对象非字符串、缺 name、空名）跳过
/// ——别名是展示与解析的辅助面，坏档不让整个概念行失效。
pub fn parse_alias_records(raw: &str) -> Vec<AliasRecord> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(entries) = value.as_array() else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| match entry {
            serde_json::Value::String(name) => Some(AliasRecord {
                name: name.trim().to_owned(),
                definition: None,
                attestation: None,
                status: ALIAS_ACTIVE,
            }),
            serde_json::Value::Object(map) => {
                let name = map
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())?;
                let definition = map
                    .get("definition")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                let attestation = map
                    .get("attestation")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                let status = match map.get("status").and_then(serde_json::Value::as_str) {
                    Some(ALIAS_RETIRED) => ALIAS_RETIRED,
                    _ => ALIAS_ACTIVE,
                };
                Some(AliasRecord {
                    name: name.to_owned(),
                    definition,
                    attestation,
                    status,
                })
            }
            _ => None,
        })
        .collect()
}

/// 登记单点的提名规格。
#[derive(Debug, Clone)]
pub struct AliasSpec {
    pub name: String,
    pub definition: Option<String>,
    pub attestation: Option<String>,
}

impl AliasSpec {
    /// 落池动作的构造式（learnhub `AliasSpec::projection` 同构）：提名即
    /// 投影——definition = 提名定义、attestation = 提名出处、status = active。
    pub fn projection(name: &str, definition: Option<&str>, attestation: Option<&str>) -> Self {
        Self {
            name: name.trim().to_owned(),
            definition: definition.map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned),
            attestation: attestation.map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned),
        }
    }
}

/// 登记单点的结果（观测/测试面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterAliasOutcome {
    /// 已含活跃同名 = 幂等跳过。
    Skipped,
    /// 新增一条活跃别名记录。
    Added,
    /// 同概念退役名复活（Reactived）并按本次提名刷新定义面。
    Reactivated,
}

fn internal(error: sqlx::Error) -> AppError {
    AppError::Internal(error.to_string())
}

/// 登记单点（全仓唯一合法的事后加名路径）。在同一事务内读改写
/// `aliases_json`。四保证：
/// 1. 宿主存在且非废弃（否则错误——退役概念不接新名）；
/// 2. 名字全库唯一：活跃占池、退役不占——撞**他概念**活跃名 = 错误
///    （调用方应先经近邻裁决）；撞本概念 canonical = 幂等跳过；
/// 3. 幂等：本概念已含活跃同名 = 跳过（不改定义面）；
/// 4. 复活：本概念的退役名 = Reactivated + 刷新 definition/attestation。
pub(crate) async fn register_alias_tx(
    transaction: &mut Transaction<'_, Sqlite>,
    concept_id: &str,
    spec: &AliasSpec,
) -> Result<RegisterAliasOutcome, AppError> {
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("alias name must not be empty".into()));
    }
    let key = name.to_lowercase();

    // 保证 1：宿主存在且非废弃。
    let host: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM learning_concept_registry \
         WHERE concept_id = ? AND deprecated = 0",
    )
    .bind(concept_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(internal)?;
    if host.is_none() {
        return Err(AppError::Conflict(format!(
            "alias host concept {concept_id} is missing or deprecated"
        )));
    }

    // 保证 2：活跃占池全库查重（canonical ∪ 活跃别名）。
    let occupant_row: Option<sqlx::sqlite::SqliteRow> = sqlx::query(
        "SELECT concept_id FROM learning_concept_registry reg \
         WHERE (lower(reg.canonical) = ? OR EXISTS (\
             SELECT 1 FROM json_each(reg.aliases_json) je \
             WHERE lower(COALESCE(json_extract(je.value, '$.name'), je.value)) = ? \
               AND COALESCE(json_extract(je.value, '$.status'), 'active') = 'active')) \
           AND reg.deprecated = 0",
    )
    .bind(&key)
    .bind(&key)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(internal)?;
    let occupant: Option<String> = match occupant_row {
        Some(row) => Some(row.try_get::<String, _>("concept_id").map_err(internal)?),
        None => None,
    };
    if let Some(occupant) = occupant {
        if occupant != concept_id {
            return Err(AppError::Conflict(format!(
                "alias name already belongs to another concept: {name}"
            )));
        }
        // 撞本概念 canonical 或已是本概念活跃别名 = 幂等跳过（保证 3）。
        return Ok(RegisterAliasOutcome::Skipped);
    }

    let row: Option<String> = sqlx::query_scalar(
        "SELECT aliases_json FROM learning_concept_registry WHERE concept_id = ?",
    )
    .bind(concept_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(internal)?;
    let Some(aliases_json) = row else {
        return Err(AppError::Conflict(format!(
            "alias host concept {concept_id} is missing"
        )));
    };
    let mut records = parse_alias_records(&aliases_json);

    // 保证 4：复活——本概念的退役名 = 翻 active + 刷新定义面。
    if let Some(record) = records
        .iter_mut()
        .find(|record| record.name.to_lowercase() == key)
    {
        record.status = ALIAS_ACTIVE;
        if spec.definition.is_some() {
            record.definition = spec.definition.clone();
        }
        if spec.attestation.is_some() {
            record.attestation = spec.attestation.clone();
        }
        write_alias_records(transaction, concept_id, &records).await?;
        return Ok(RegisterAliasOutcome::Reactivated);
    }

    // 保证 3 的另一半：活跃同名已查过；这里是全新名字 → 追加记录。
    records.push(AliasRecord {
        name: name.to_owned(),
        definition: spec.definition.clone(),
        attestation: spec.attestation.clone(),
        status: ALIAS_ACTIVE,
    });
    write_alias_records(transaction, concept_id, &records).await?;
    Ok(RegisterAliasOutcome::Added)
}

async fn write_alias_records(
    transaction: &mut Transaction<'_, Sqlite>,
    concept_id: &str,
    records: &[AliasRecord],
) -> Result<(), AppError> {
    let json =
        serde_json::to_string(records).map_err(|error| AppError::Internal(error.to_string()))?;
    sqlx::query(
        "UPDATE learning_concept_registry SET aliases_json = ?, updated_at = ? \
         WHERE concept_id = ?",
    )
    .bind(json)
    .bind(nomifun_common::now_ms())
    .bind(concept_id)
    .execute(&mut **transaction)
    .await
    .map_err(internal)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_alias_records_reads_both_shapes() {
        // 新形状：对象记录（定义/出处可选、状态缺省 active）。
        let new_shape = r#"[
            {"name":"十字相乘","definition":"拆因式的技巧","attestation":"教练提名","status":"active"},
            {"name":"旧名","status":"retired"}
        ]"#;
        let records = parse_alias_records(new_shape);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "十字相乘");
        assert_eq!(records[0].definition.as_deref(), Some("拆因式的技巧"));
        assert_eq!(records[0].status, ALIAS_ACTIVE);
        assert_eq!(records[1].status, ALIAS_RETIRED);
        // 旧形状：字符串回填 active。
        let legacy = r#"["Limits","配方法"]"#;
        let records = parse_alias_records(legacy);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0], AliasRecord {
            name: "Limits".into(), definition: None, attestation: None, status: ALIAS_ACTIVE,
        });
        // 坏档：非数组/坏元素不致命。
        assert!(parse_alias_records("not json").is_empty());
        let mixed = parse_alias_records(r#"[42, {"definition":"no name"}, "好的"]"#);
        assert_eq!(
            mixed,
            vec![AliasRecord {
                name: "好的".into(),
                definition: None,
                attestation: None,
                status: ALIAS_ACTIVE,
            }]
        );
    }

    async fn setup_pool() -> sqlx::SqlitePool {
        use sqlx::migrate::Migrator;
        static MIGRATOR: Migrator = sqlx::migrate!("../nomifun-db/migrations");
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        MIGRATOR.run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO learning_concept_registry \
             (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
             VALUES ('0190f5fe-7c00-7a00-8abc-111111111111', '极限', '[{\"name\":\"Limits\"}]', '无限逼近的值', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_concept_registry \
             (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
             VALUES ('0190f5fe-7c00-7a00-8abc-222222222222', '导数', '[]', '瞬时变化率', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    const HOST: &str = "0190f5fe-7c00-7a00-8abc-111111111111";
    const OTHER: &str = "0190f5fe-7c00-7a00-8abc-222222222222";

    /// 四保证：幂等跳过 / 复活刷新定义面 / 撞他概念活跃名拒绝 / 退役宿主不接新名。
    #[tokio::test]
    async fn register_alias_enforces_the_four_guarantees() {
        let pool = setup_pool().await;

        // Added：全新名字落为活跃记录。
        let mut tx = pool.begin().await.unwrap();
        let outcome = register_alias_tx(
            &mut tx,
            HOST,
            &AliasSpec::projection("趋近", Some("以趋近语言描述极限"), Some("教练提名")),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(outcome, RegisterAliasOutcome::Added);

        // 幂等：已含活跃同名 = Skipped（定义面不改）。
        let mut tx = pool.begin().await.unwrap();
        let outcome = register_alias_tx(&mut tx, HOST, &AliasSpec::projection("趋近", Some("别的东西"), None))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(outcome, RegisterAliasOutcome::Skipped);
        let raw: String = sqlx::query_scalar("SELECT aliases_json FROM learning_concept_registry WHERE concept_id = ?")
            .bind(HOST)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(raw.contains("以趋近语言描述极限"), "definition untouched on skip: {raw}");

        // 撞他概念活跃名 = 拒绝（canonical 侧也算占用）。
        let mut tx = pool.begin().await.unwrap();
        let clash = register_alias_tx(&mut tx, HOST, &AliasSpec::projection("导数", None, None)).await;
        assert!(clash.is_err());
        // 撞他概念的活跃别名同样拒绝。
        let clash = register_alias_tx(&mut tx, OTHER, &AliasSpec::projection("趋近", None, None)).await;
        assert!(clash.is_err());
        tx.rollback().await.unwrap();

        // 复活：退役名翻 active + 刷新定义面。
        sqlx::query(
            "UPDATE learning_concept_registry SET aliases_json = ? WHERE concept_id = ?",
        )
        .bind(r#"[{"name":"趋近","definition":"旧意义","status":"retired"}]"#)
        .bind(HOST)
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let outcome = register_alias_tx(
            &mut tx,
            HOST,
            &AliasSpec::projection("趋近", Some("新意义"), Some("新出处")),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(outcome, RegisterAliasOutcome::Reactivated);
        let raw: String = sqlx::query_scalar("SELECT aliases_json FROM learning_concept_registry WHERE concept_id = ?")
            .bind(HOST)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(raw.contains("\"status\":\"active\""), "{raw}");
        assert!(raw.contains("新意义"), "{raw}");

        // 退役宿主不接新名。
        sqlx::query("UPDATE learning_concept_registry SET deprecated = 1 WHERE concept_id = ?")
            .bind(OTHER)
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let rejected = register_alias_tx(&mut tx, OTHER, &AliasSpec::projection("新名", None, None)).await;
        assert!(rejected.is_err());
    }
}
