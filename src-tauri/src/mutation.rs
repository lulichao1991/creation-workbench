use crate::database::{
    new_id, now, open_database, query_table_json, row_by_id, AppResult, BUSINESS_TABLES,
};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationRequest {
    pub action: String,
    pub entity_type: String,
    pub object_id: Option<String>,
    #[serde(default)]
    pub values: Map<String, Value>,
    pub change_set_id: Option<String>,
    pub change_set_name: Option<String>,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResponse {
    pub object_id: String,
    pub change_set_id: String,
    pub revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMutationRequest {
    pub mutations: Vec<MutationRequest>,
    pub change_set_id: Option<String>,
    pub change_set_name: Option<String>,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMutationResponse {
    pub object_ids: Vec<String>,
    pub change_set_id: String,
    pub revision: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotResponse {
    pub id: String,
    pub revision: i64,
}

struct EntitySpec {
    entity_type: &'static str,
    table: &'static str,
    fields: &'static [&'static str],
    composite: bool,
}

const CONTENT_UNIT_FIELDS: &[&str] = &[
    "project_id",
    "parent_id",
    "type",
    "name",
    "summary",
    "sort_order",
    "maturity",
    "sync_status",
];
const SCRIPT_FIELDS: &[&str] = &[
    "content_unit_id",
    "title",
    "summary",
    "maturity",
    "sync_status",
];
const SCENE_FIELDS: &[&str] = &[
    "script_id",
    "title",
    "sort_order",
    "location_text",
    "time_text",
    "summary",
    "content",
    "maturity",
    "sync_status",
];
const SHOT_FIELDS: &[&str] = &[
    "scene_id",
    "sort_order",
    "title",
    "duration",
    "narrative_purpose",
    "new_information",
    "shot_size",
    "camera_height",
    "camera_direction",
    "composition",
    "camera_movement",
    "subjects",
    "action",
    "dialogue",
    "environment",
    "start_state",
    "end_state",
    "maturity",
    "sync_status",
];
const ASSET_FIELDS: &[&str] = &[
    "project_id",
    "type",
    "name",
    "description",
    "scope_unit_id",
    "maturity",
    "sync_status",
];
const ASSET_MEDIA_FIELDS: &[&str] = &[
    "asset_id",
    "media_type",
    "file_path",
    "label",
    "description",
    "sort_order",
    "is_primary",
    "source_type",
];
const ASSET_REQUIREMENT_FIELDS: &[&str] = &[
    "content_unit_id",
    "asset_id",
    "asset_type",
    "requirement_type",
    "description",
    "prompt_draft",
    "status",
    "created_from_type",
    "created_from_id",
];
const ASSET_REQUIREMENT_SOURCE_FIELDS: &[&str] =
    &["asset_requirement_id", "source_type", "source_id"];
const ASSET_MEDIA_REQUIREMENT_FIELDS: &[&str] = &["asset_media_id", "asset_requirement_id"];
const SHOT_ASSET_FIELDS: &[&str] = &["shot_id", "asset_id", "role"];
const KEYFRAME_FIELDS: &[&str] = &[
    "shot_id",
    "type",
    "file_path",
    "description",
    "prompt_draft",
    "status",
    "sort_order",
];
const GENERATION_TASK_FIELDS: &[&str] = &[
    "content_unit_id",
    "name",
    "target_model",
    "duration",
    "prompt",
    "status",
];
const GENERATION_TASK_SHOT_FIELDS: &[&str] = &["generation_task_id", "shot_id", "sort_order"];
const RELATION_FIELDS: &[&str] = &[
    "project_id",
    "source_type",
    "source_id",
    "relation_type",
    "target_type",
    "target_id",
    "description",
    "importance",
    "status",
];
const PROJECT_FIELDS: &[&str] = &[
    "name",
    "description",
    "structure_type",
    "maturity",
    "sync_status",
];

fn entity_spec(entity_type: &str) -> AppResult<EntitySpec> {
    let spec = match entity_type {
        "project" => EntitySpec {
            entity_type: "project",
            table: "projects",
            fields: PROJECT_FIELDS,
            composite: false,
        },
        "contentUnit" => EntitySpec {
            entity_type: "contentUnit",
            table: "content_units",
            fields: CONTENT_UNIT_FIELDS,
            composite: false,
        },
        "script" => EntitySpec {
            entity_type: "script",
            table: "scripts",
            fields: SCRIPT_FIELDS,
            composite: false,
        },
        "scene" => EntitySpec {
            entity_type: "scene",
            table: "scenes",
            fields: SCENE_FIELDS,
            composite: false,
        },
        "shot" => EntitySpec {
            entity_type: "shot",
            table: "shots",
            fields: SHOT_FIELDS,
            composite: false,
        },
        "asset" => EntitySpec {
            entity_type: "asset",
            table: "assets",
            fields: ASSET_FIELDS,
            composite: false,
        },
        "assetMedia" => EntitySpec {
            entity_type: "assetMedia",
            table: "asset_media",
            fields: ASSET_MEDIA_FIELDS,
            composite: false,
        },
        "assetRequirement" => EntitySpec {
            entity_type: "assetRequirement",
            table: "asset_requirements",
            fields: ASSET_REQUIREMENT_FIELDS,
            composite: false,
        },
        "assetRequirementSource" => EntitySpec {
            entity_type: "assetRequirementSource",
            table: "asset_requirement_sources",
            fields: ASSET_REQUIREMENT_SOURCE_FIELDS,
            composite: false,
        },
        "assetMediaRequirement" => EntitySpec {
            entity_type: "assetMediaRequirement",
            table: "asset_media_requirements",
            fields: ASSET_MEDIA_REQUIREMENT_FIELDS,
            composite: false,
        },
        "shotAsset" => EntitySpec {
            entity_type: "shotAsset",
            table: "shot_assets",
            fields: SHOT_ASSET_FIELDS,
            composite: false,
        },
        "keyframe" => EntitySpec {
            entity_type: "keyframe",
            table: "keyframes",
            fields: KEYFRAME_FIELDS,
            composite: false,
        },
        "generationTask" => EntitySpec {
            entity_type: "generationTask",
            table: "generation_tasks",
            fields: GENERATION_TASK_FIELDS,
            composite: false,
        },
        "generationTaskShot" => EntitySpec {
            entity_type: "generationTaskShot",
            table: "generation_task_shots",
            fields: GENERATION_TASK_SHOT_FIELDS,
            composite: true,
        },
        "relation" => EntitySpec {
            entity_type: "relation",
            table: "relations",
            fields: RELATION_FIELDS,
            composite: false,
        },
        _ => return Err(format!("不支持的对象类型：{entity_type}")),
    };
    Ok(spec)
}

#[tauri::command]
pub fn apply_mutation(
    project_path: String,
    request: MutationRequest,
) -> AppResult<MutationResponse> {
    let result = execute_mutations(project_path, vec![request], None, None, None, None)?;
    Ok(MutationResponse {
        object_id: result.object_ids.into_iter().next().unwrap_or_default(),
        change_set_id: result.change_set_id,
        revision: result.revision,
    })
}

#[tauri::command]
pub fn apply_batch_mutation(
    project_path: String,
    request: BatchMutationRequest,
) -> AppResult<BatchMutationResponse> {
    execute_mutations(
        project_path,
        request.mutations,
        request.change_set_id,
        request.change_set_name,
        request.source_type,
        request.source_id,
    )
}

fn execute_mutations(
    project_path: String,
    requests: Vec<MutationRequest>,
    batch_change_set_id: Option<String>,
    batch_change_set_name: Option<String>,
    batch_source_type: Option<String>,
    batch_source_id: Option<String>,
) -> AppResult<BatchMutationResponse> {
    if requests.is_empty() {
        return Err("批量修改不能为空".into());
    }
    let mut conn = open_database(Path::new(&project_path))?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let project_id: String = tx
        .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let first = requests
        .first()
        .ok_or_else(|| "批量修改不能为空".to_string())?;
    let source_type = batch_source_type
        .as_deref()
        .or(first.source_type.as_deref())
        .unwrap_or("user")
        .to_string();
    let source_id = batch_source_id
        .as_deref()
        .or(first.source_id.as_deref())
        .map(str::to_string);
    let change_set_id = match batch_change_set_id.or_else(|| first.change_set_id.clone()) {
        Some(id) => id,
        None => create_change_set(
            &tx,
            &project_id,
            batch_change_set_name
                .as_deref()
                .or(first.change_set_name.as_deref())
                .unwrap_or("本轮修改"),
            &source_type,
            source_id.as_deref(),
        )?,
    };
    let exists: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM change_sets WHERE id=?1 AND project_id=?2 AND status<>'undone'",
            params![change_set_id, project_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists != 1 {
        return Err("指定的变更集不存在或不属于当前项目".into());
    }

    let mut object_ids = Vec::with_capacity(requests.len());
    let mut affected_task_ids = HashSet::new();
    for request in requests {
        let spec = entity_spec(&request.entity_type)?;
        validate_fields(&spec, &request.values)?;
        collect_affected_tasks(&tx, &request, &mut affected_task_ids)?;
        let operation_source_type = request.source_type.as_deref().unwrap_or(&source_type);
        let operation_source_id = request.source_id.as_deref().or(source_id.as_deref());
        let object_id = match request.action.as_str() {
            "create" => create_object(
                &tx,
                &spec,
                request.object_id,
                request.values,
                &change_set_id,
                operation_source_type,
                operation_source_id,
            )?,
            "patch" | "move" => patch_object(
                &tx,
                &spec,
                request
                    .object_id
                    .ok_or_else(|| "修改对象缺少 objectId".to_string())?,
                request.values,
                &change_set_id,
                operation_source_type,
                operation_source_id,
            )?,
            "delete" => delete_object(
                &tx,
                &spec,
                request
                    .object_id
                    .ok_or_else(|| "删除对象缺少 objectId".to_string())?,
                &change_set_id,
                operation_source_type,
                operation_source_id,
            )?,
            action => return Err(format!("不支持的修改动作：{action}")),
        };
        object_ids.push(object_id);
    }

    for task_id in affected_task_ids {
        recalculate_generation_task_duration(
            &tx,
            &task_id,
            &change_set_id,
            &source_type,
            source_id.as_deref(),
        )?;
    }

    let timestamp = now();
    tx.execute(
        "UPDATE projects SET revision = revision + 1, updated_at=?1 WHERE id=?2",
        params![timestamp, project_id],
    )
    .map_err(|e| e.to_string())?;
    let revision: i64 = tx
        .query_row(
            "SELECT revision FROM projects WHERE id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(BatchMutationResponse {
        object_ids,
        change_set_id,
        revision,
    })
}

fn collect_affected_tasks(
    tx: &Transaction<'_>,
    request: &MutationRequest,
    affected: &mut HashSet<String>,
) -> AppResult<()> {
    if request.entity_type == "generationTaskShot" {
        if let Some(task_id) = request
            .values
            .get("generation_task_id")
            .and_then(Value::as_str)
            .or_else(|| {
                request
                    .object_id
                    .as_deref()
                    .and_then(|id| id.split_once('|').map(|v| v.0))
            })
        {
            affected.insert(task_id.to_string());
        }
    }
    if request.entity_type == "shot"
        && (request.action == "delete" || request.values.contains_key("duration"))
    {
        if let Some(shot_id) = request.object_id.as_deref() {
            let mut stmt = tx
                .prepare("SELECT generation_task_id FROM generation_task_shots WHERE shot_id=?1")
                .map_err(|e| e.to_string())?;
            let ids = stmt
                .query_map([shot_id], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            for id in ids {
                affected.insert(id.map_err(|e| e.to_string())?);
            }
        }
    }
    Ok(())
}

fn recalculate_generation_task_duration(
    tx: &Transaction<'_>,
    task_id: &str,
    change_set_id: &str,
    source_type: &str,
    source_id: Option<&str>,
) -> AppResult<()> {
    if row_by_id(tx, "generation_tasks", task_id)?.is_none() {
        return Ok(());
    }
    let total: f64 = tx
        .query_row(
            "SELECT COALESCE(SUM(shots.duration), 0) FROM generation_task_shots JOIN shots ON shots.id=generation_task_shots.shot_id WHERE generation_task_shots.generation_task_id=?1",
            [task_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut values = Map::new();
    values.insert("duration".into(), Value::from(total));
    let spec = entity_spec("generationTask")?;
    patch_object(
        tx,
        &spec,
        task_id.to_string(),
        values,
        change_set_id,
        source_type,
        source_id,
    )?;
    Ok(())
}

fn create_object(
    tx: &Transaction<'_>,
    spec: &EntitySpec,
    requested_id: Option<String>,
    mut values: Map<String, Value>,
    change_set_id: &str,
    source_type: &str,
    source_id: Option<&str>,
) -> AppResult<String> {
    let object_id = if spec.composite {
        composite_id_from_values(&values)?
    } else {
        requested_id.unwrap_or_else(new_id)
    };
    if spec.entity_type == "contentUnit" {
        validate_content_parent(tx, &object_id, values.get("parent_id"))?;
    }
    if !spec.composite {
        values.insert("id".into(), Value::String(object_id.clone()));
        let timestamp = Value::String(now());
        values.insert("created_at".into(), timestamp.clone());
        values.insert("updated_at".into(), timestamp);
    }
    insert_row(tx, spec.table, &values)?;
    let created =
        select_object(tx, spec, &object_id)?.ok_or_else(|| "创建后无法读取对象".to_string())?;
    record_change(
        tx,
        change_set_id,
        spec.entity_type,
        &object_id,
        "__created__",
        &Value::Null,
        &created,
        source_type,
        source_id,
    )?;
    Ok(object_id)
}

fn patch_object(
    tx: &Transaction<'_>,
    spec: &EntitySpec,
    object_id: String,
    values: Map<String, Value>,
    change_set_id: &str,
    source_type: &str,
    source_id: Option<&str>,
) -> AppResult<String> {
    let before =
        select_object(tx, spec, &object_id)?.ok_or_else(|| format!("对象不存在：{}", object_id))?;
    let before_object = before
        .as_object()
        .ok_or_else(|| "对象数据格式错误".to_string())?;
    for (field, new_value) in &values {
        if spec.entity_type == "contentUnit" && field == "parent_id" {
            validate_content_parent(tx, &object_id, Some(new_value))?;
        }
        let old_value = before_object.get(field).cloned().unwrap_or(Value::Null);
        if old_value == *new_value {
            continue;
        }
        update_field(tx, spec, &object_id, field, new_value)?;
        record_change(
            tx,
            change_set_id,
            spec.entity_type,
            &object_id,
            field,
            &old_value,
            new_value,
            source_type,
            source_id,
        )?;
    }
    if !spec.composite {
        update_field(tx, spec, &object_id, "updated_at", &Value::String(now()))?;
    }
    Ok(object_id)
}

fn delete_object(
    tx: &Transaction<'_>,
    spec: &EntitySpec,
    object_id: String,
    change_set_id: &str,
    source_type: &str,
    source_id: Option<&str>,
) -> AppResult<String> {
    let before =
        select_object(tx, spec, &object_id)?.ok_or_else(|| format!("对象不存在：{}", object_id))?;
    let (where_clause, keys) = object_keys(spec, &object_id, 1)?;
    let sql = format!("DELETE FROM {} WHERE {where_clause}", spec.table);
    let affected = tx
        .execute(&sql, params_from_iter(keys.iter()))
        .map_err(|e| format!("删除失败：{e}"))?;
    if affected != 1 {
        return Err("删除对象时影响的记录数量异常".into());
    }
    record_change(
        tx,
        change_set_id,
        spec.entity_type,
        &object_id,
        "__deleted__",
        &before,
        &Value::Null,
        source_type,
        source_id,
    )?;
    Ok(object_id)
}

fn validate_fields(spec: &EntitySpec, values: &Map<String, Value>) -> AppResult<()> {
    let allowed: HashSet<&str> = spec.fields.iter().copied().collect();
    for field in values.keys() {
        if !allowed.contains(field.as_str()) {
            return Err(format!("{} 不允许修改字段：{field}", spec.entity_type));
        }
    }
    Ok(())
}

fn create_change_set(
    tx: &Transaction<'_>,
    project_id: &str,
    name: &str,
    source_type: &str,
    source_id: Option<&str>,
) -> AppResult<String> {
    let id = new_id();
    let timestamp = now();
    tx.execute(
        "INSERT INTO change_sets (id, project_id, name, source_type, source_id, status, created_at, closed_at) VALUES (?1, ?2, ?3, ?4, ?5, 'closed', ?6, ?6)",
        params![id, project_id, name, source_type, source_id, timestamp],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
fn record_change(
    tx: &Transaction<'_>,
    change_set_id: &str,
    object_type: &str,
    object_id: &str,
    field_name: &str,
    old_value: &Value,
    new_value: &Value,
    source_type: &str,
    source_id: Option<&str>,
) -> AppResult<()> {
    tx.execute(
        "INSERT INTO changes (id, change_set_id, object_type, object_id, field_name, old_value, new_value, source_type, source_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            new_id(),
            change_set_id,
            object_type,
            object_id,
            field_name,
            serde_json::to_string(old_value).map_err(|e| e.to_string())?,
            serde_json::to_string(new_value).map_err(|e| e.to_string())?,
            source_type,
            source_id,
            now(),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn insert_row(tx: &Transaction<'_>, table: &str, values: &Map<String, Value>) -> AppResult<()> {
    let columns: Vec<&String> = values.keys().collect();
    let placeholders = (1..=columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({placeholders})",
        columns
            .iter()
            .map(|value| value.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let params = columns
        .iter()
        .map(|column| json_to_sql(&values[*column]))
        .collect::<Vec<_>>();
    tx.execute(&sql, params_from_iter(params.iter()))
        .map_err(|e| format!("写入 {table} 失败：{e}"))?;
    Ok(())
}

fn update_field(
    tx: &Transaction<'_>,
    spec: &EntitySpec,
    object_id: &str,
    field: &str,
    value: &Value,
) -> AppResult<()> {
    if field != "updated_at" && !spec.fields.contains(&field) {
        return Err(format!("不允许修改字段：{field}"));
    }
    let (where_clause, mut keys) = object_keys(spec, object_id, 2)?;
    let sql = format!("UPDATE {} SET {field}=?1 WHERE {where_clause}", spec.table);
    let mut params = vec![json_to_sql(value)];
    params.append(&mut keys);
    tx.execute(&sql, params_from_iter(params.iter()))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn select_object(
    tx: &Transaction<'_>,
    spec: &EntitySpec,
    object_id: &str,
) -> AppResult<Option<Value>> {
    if !spec.composite {
        return row_by_id(tx, spec.table, object_id);
    }
    let (where_clause, keys) = object_keys(spec, object_id, 1)?;
    let sql = format!(
        "SELECT generation_task_id, shot_id, sort_order FROM {} WHERE {where_clause}",
        spec.table
    );
    let mut stmt = tx.prepare(&sql).map_err(|e| e.to_string())?;
    let result = stmt.query_row(params_from_iter(keys.iter()), |row| {
        Ok(json!({
            "generation_task_id": row.get::<_, String>(0)?,
            "shot_id": row.get::<_, String>(1)?,
            "sort_order": row.get::<_, i64>(2)?,
        }))
    });
    match result {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn object_keys(
    spec: &EntitySpec,
    object_id: &str,
    first_parameter: usize,
) -> AppResult<(String, Vec<SqlValue>)> {
    if spec.composite {
        let (task_id, shot_id) = object_id
            .split_once('|')
            .ok_or_else(|| "组合对象 ID 格式无效".to_string())?;
        Ok((
            format!(
                "generation_task_id=?{} AND shot_id=?{}",
                first_parameter,
                first_parameter + 1
            ),
            vec![
                SqlValue::Text(task_id.into()),
                SqlValue::Text(shot_id.into()),
            ],
        ))
    } else {
        Ok((
            format!("id=?{first_parameter}"),
            vec![SqlValue::Text(object_id.into())],
        ))
    }
}

fn composite_id_from_values(values: &Map<String, Value>) -> AppResult<String> {
    let task_id = values
        .get("generation_task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 generation_task_id".to_string())?;
    let shot_id = values
        .get("shot_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 shot_id".to_string())?;
    Ok(format!("{task_id}|{shot_id}"))
}

fn validate_content_parent(
    tx: &Transaction<'_>,
    object_id: &str,
    parent_value: Option<&Value>,
) -> AppResult<()> {
    let Some(parent_id) = parent_value.and_then(Value::as_str) else {
        return Ok(());
    };
    if parent_id == object_id {
        return Err("内容单元不能成为自己的父级".into());
    }
    let is_descendant: i64 = tx
        .query_row(
            "WITH RECURSIVE descendants(id) AS (
                SELECT id FROM content_units WHERE parent_id=?1
                UNION ALL
                SELECT child.id FROM content_units child JOIN descendants parent ON child.parent_id=parent.id
             ) SELECT COUNT(*) FROM descendants WHERE id=?2",
            params![object_id, parent_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if is_descendant > 0 {
        return Err("不能把内容单元移动到自己的下级".into());
    }
    Ok(())
}

fn json_to_sql(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(value) => SqlValue::Integer(i64::from(*value)),
        Value::Number(value) if value.is_i64() => {
            SqlValue::Integer(value.as_i64().unwrap_or_default())
        }
        Value::Number(value) => SqlValue::Real(value.as_f64().unwrap_or_default()),
        Value::String(value) => SqlValue::Text(value.clone()),
        value => SqlValue::Text(value.to_string()),
    }
}

#[tauri::command]
pub fn list_history(project_path: String) -> AppResult<Value> {
    let conn = open_database(Path::new(&project_path))?;
    Ok(json!({
        "changeSets": query_table_json(&conn, "change_sets")?,
        "changes": query_table_json(&conn, "changes")?,
        "snapshots": query_table_json(&conn, "snapshots")?,
    }))
}

#[tauri::command]
pub fn undo_change_set(project_path: String, change_set_id: String) -> AppResult<i64> {
    let mut conn = open_database(Path::new(&project_path))?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let status: String = tx
        .query_row(
            "SELECT status FROM change_sets WHERE id=?1",
            [&change_set_id],
            |row| row.get(0),
        )
        .map_err(|_| "变更集不存在".to_string())?;
    if status == "undone" {
        return Err("该变更集已经撤销".into());
    }

    let change_rows = {
        let mut stmt = tx
            .prepare("SELECT object_type, object_id, field_name, old_value FROM changes WHERE change_set_id=?1 ORDER BY rowid DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([&change_set_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    for (object_type, object_id, field_name, old_raw) in change_rows {
        let spec = entity_spec(&object_type)?;
        let old_value: Value = old_raw
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or(Value::Null);
        match field_name.as_str() {
            "__created__" => {
                let (where_clause, keys) = object_keys(&spec, &object_id, 1)?;
                tx.execute(
                    &format!("DELETE FROM {} WHERE {where_clause}", spec.table),
                    params_from_iter(keys.iter()),
                )
                .map_err(|e| format!("撤销创建失败：{e}"))?;
            }
            "__deleted__" => {
                insert_row(
                    &tx,
                    spec.table,
                    old_value
                        .as_object()
                        .ok_or_else(|| "删除历史损坏".to_string())?,
                )?;
            }
            _ => update_field(&tx, &spec, &object_id, &field_name, &old_value)?,
        }
    }

    tx.execute(
        "UPDATE change_sets SET status='undone' WHERE id=?1",
        [&change_set_id],
    )
    .map_err(|e| e.to_string())?;
    let timestamp = now();
    tx.execute(
        "UPDATE projects SET revision=revision+1, updated_at=?1",
        [&timestamp],
    )
    .map_err(|e| e.to_string())?;
    let revision: i64 = tx
        .query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(revision)
}

#[tauri::command]
pub fn create_snapshot(
    project_path: String,
    name: String,
    description: String,
) -> AppResult<SnapshotResponse> {
    let mut conn = open_database(Path::new(&project_path))?;
    let mut snapshot = Map::new();
    snapshot.insert(
        "projects".into(),
        Value::Array(query_table_json(&conn, "projects")?),
    );
    for table in BUSINESS_TABLES {
        snapshot.insert(
            (*table).into(),
            Value::Array(query_table_json(&conn, table)?),
        );
    }
    let snapshot_json = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (project_id, revision): (String, i64) = tx
        .query_row("SELECT id, revision FROM projects LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| e.to_string())?;
    let id = new_id();
    tx.execute(
        "INSERT INTO snapshots (id, project_id, scope_type, scope_id, name, description, revision, snapshot_json, created_at) VALUES (?1, ?2, 'project', NULL, ?3, ?4, ?5, ?6, ?7)",
        params![id, project_id, name, description, revision, snapshot_json, now()],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(SnapshotResponse { id, revision })
}

#[tauri::command]
pub fn restore_snapshot(project_path: String, snapshot_id: String) -> AppResult<i64> {
    let mut conn = open_database(Path::new(&project_path))?;
    let snapshot_json: String = conn
        .query_row(
            "SELECT snapshot_json FROM snapshots WHERE id=?1",
            [&snapshot_id],
            |row| row.get(0),
        )
        .map_err(|_| "快照不存在".to_string())?;
    let snapshot: Map<String, Value> =
        serde_json::from_str(&snapshot_json).map_err(|e| format!("快照数据损坏：{e}"))?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let project_id: String = tx
        .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let change_set_id =
        create_change_set(&tx, &project_id, "恢复快照", "snapshot", Some(&snapshot_id))?;

    for table in BUSINESS_TABLES.iter().rev() {
        tx.execute(&format!("DELETE FROM {table}"), [])
            .map_err(|e| format!("清理 {table} 失败：{e}"))?;
    }
    for table in BUSINESS_TABLES {
        if let Some(rows) = snapshot.get(*table).and_then(Value::as_array) {
            for row in rows {
                insert_row(
                    &tx,
                    table,
                    row.as_object()
                        .ok_or_else(|| "快照行格式错误".to_string())?,
                )?;
            }
        }
    }
    if let Some(project) = snapshot
        .get("projects")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_object)
    {
        for field in PROJECT_FIELDS {
            if let Some(value) = project.get(*field) {
                let spec = entity_spec("project")?;
                update_field(&tx, &spec, &project_id, field, value)?;
            }
        }
    }
    record_change(
        &tx,
        &change_set_id,
        "project",
        &project_id,
        "__snapshot_restore__",
        &Value::Null,
        &Value::String(snapshot_id),
        "snapshot",
        None,
    )?;
    tx.execute(
        "UPDATE projects SET revision=revision+1, updated_at=?1",
        [now()],
    )
    .map_err(|e| e.to_string())?;
    let revision: i64 = tx
        .query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{create_project, import_project_file, open_project};
    use crate::database::init_database;
    use std::fs;

    fn request(
        action: &str,
        entity_type: &str,
        object_id: Option<String>,
        values: Value,
    ) -> MutationRequest {
        MutationRequest {
            action: action.into(),
            entity_type: entity_type.into(),
            object_id,
            values: values.as_object().cloned().unwrap_or_default(),
            change_set_id: None,
            change_set_name: Some("测试修改".into()),
            source_type: None,
            source_id: None,
        }
    }

    fn batch(mutations: Vec<MutationRequest>, name: &str) -> BatchMutationRequest {
        BatchMutationRequest {
            mutations,
            change_set_id: None,
            change_set_name: Some(name.into()),
            source_type: None,
            source_id: None,
        }
    }

    #[test]
    fn mutation_records_history_and_undoes_patch() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "测试", "series").unwrap();
        let created = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "contentUnit",
                None,
                json!({
                    "project_id": project.id,
                    "type": "season",
                    "name": "第一季",
                    "sort_order": 0
                }),
            ),
        )
        .unwrap();
        let patched = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "patch",
                "contentUnit",
                Some(created.object_id.clone()),
                json!({"name": "新名称"}),
            ),
        )
        .unwrap();
        undo_change_set(
            temp.path().to_string_lossy().to_string(),
            patched.change_set_id,
        )
        .unwrap();
        let conn = open_database(temp.path()).unwrap();
        let name: String = conn
            .query_row(
                "SELECT name FROM content_units WHERE id=?1",
                [&created.object_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "第一季");
    }

    #[test]
    fn generation_task_batch_is_atomic_and_duration_stays_derived() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "任务测试", "short").unwrap();
        let unit = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "contentUnit",
                None,
                json!({"project_id": project.id, "type": "short", "name": "正片", "sort_order": 0}),
            ),
        )
        .unwrap();
        let script = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "script",
                None,
                json!({"content_unit_id": unit.object_id, "title": "正片"}),
            ),
        )
        .unwrap();
        let scene = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "scene",
                None,
                json!({"script_id": script.object_id, "title": "场01", "sort_order": 0}),
            ),
        )
        .unwrap();
        let shot_a = new_id();
        let shot_b = new_id();
        let task_id = new_id();
        apply_batch_mutation(
            temp.path().to_string_lossy().to_string(),
            batch(
                vec![
                    request("create", "shot", Some(shot_a.clone()), json!({"scene_id": scene.object_id, "title": "A", "sort_order": 0, "duration": 2.0})),
                    request("create", "shot", Some(shot_b.clone()), json!({"scene_id": scene.object_id, "title": "B", "sort_order": 1, "duration": 3.0})),
                    request("create", "generationTask", Some(task_id.clone()), json!({"content_unit_id": unit.object_id, "name": "任务", "duration": 0, "status": "draft"})),
                    request("create", "generationTaskShot", None, json!({"generation_task_id": task_id, "shot_id": shot_a, "sort_order": 0})),
                    request("create", "generationTaskShot", None, json!({"generation_task_id": task_id, "shot_id": shot_b, "sort_order": 1})),
                ],
                "创建任务",
            ),
        )
        .unwrap();
        let conn = open_database(temp.path()).unwrap();
        let duration: f64 = conn
            .query_row(
                "SELECT duration FROM generation_tasks WHERE id=?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(duration, 5.0);
        drop(conn);

        apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "patch",
                "shot",
                Some(shot_a.clone()),
                json!({"duration": 4.0}),
            ),
        )
        .unwrap();
        let conn = open_database(temp.path()).unwrap();
        let duration: f64 = conn
            .query_row(
                "SELECT duration FROM generation_tasks WHERE id=?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(duration, 7.0);
        drop(conn);

        apply_batch_mutation(
            temp.path().to_string_lossy().to_string(),
            batch(
                vec![
                    request(
                        "delete",
                        "generationTaskShot",
                        Some(format!("{task_id}|{shot_a}")),
                        json!({}),
                    ),
                    request(
                        "delete",
                        "generationTaskShot",
                        Some(format!("{task_id}|{shot_b}")),
                        json!({}),
                    ),
                    request("delete", "generationTask", Some(task_id.clone()), json!({})),
                ],
                "删除任务",
            ),
        )
        .unwrap();
        let conn = open_database(temp.path()).unwrap();
        let tasks: i64 = conn
            .query_row("SELECT COUNT(*) FROM generation_tasks", [], |row| {
                row.get(0)
            })
            .unwrap();
        let links: i64 = conn
            .query_row("SELECT COUNT(*) FROM generation_task_shots", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!((tasks, links), (0, 0));
    }

    #[test]
    fn failed_batch_rolls_back_every_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "原子性", "short").unwrap();
        let unit = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "contentUnit",
                None,
                json!({"project_id": project.id, "type": "short", "name": "正片", "sort_order": 0}),
            ),
        )
        .unwrap();
        let result = apply_batch_mutation(
            temp.path().to_string_lossy().to_string(),
            batch(
                vec![
                    request(
                        "patch",
                        "contentUnit",
                        Some(unit.object_id.clone()),
                        json!({"sort_order": 9}),
                    ),
                    request(
                        "patch",
                        "contentUnit",
                        Some("missing".into()),
                        json!({"sort_order": 10}),
                    ),
                ],
                "应回滚",
            ),
        );
        assert!(result.is_err());
        let conn = open_database(temp.path()).unwrap();
        let order: i64 = conn
            .query_row(
                "SELECT sort_order FROM content_units WHERE id=?1",
                [&unit.object_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(order, 0);
    }

    #[test]
    fn undoing_atomic_reorder_restores_unique_orders() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "排序撤销", "series").unwrap();
        let a = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "contentUnit",
                None,
                json!({"project_id": project.id, "type": "season", "name": "A", "sort_order": 0}),
            ),
        )
        .unwrap();
        let b = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "contentUnit",
                None,
                json!({"project_id": project.id, "type": "season", "name": "B", "sort_order": 1}),
            ),
        )
        .unwrap();
        let reordered = apply_batch_mutation(
            temp.path().to_string_lossy().to_string(),
            batch(
                vec![
                    request(
                        "move",
                        "contentUnit",
                        Some(a.object_id.clone()),
                        json!({"sort_order": 1}),
                    ),
                    request(
                        "move",
                        "contentUnit",
                        Some(b.object_id.clone()),
                        json!({"sort_order": 0}),
                    ),
                ],
                "交换顺序",
            ),
        )
        .unwrap();
        undo_change_set(
            temp.path().to_string_lossy().to_string(),
            reordered.change_set_id,
        )
        .unwrap();
        let conn = open_database(temp.path()).unwrap();
        let order_a: i64 = conn
            .query_row(
                "SELECT sort_order FROM content_units WHERE id=?1",
                [&a.object_id],
                |row| row.get(0),
            )
            .unwrap();
        let order_b: i64 = conn
            .query_row(
                "SELECT sort_order FROM content_units WHERE id=?1",
                [&b.object_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((order_a, order_b), (0, 1));
    }

    #[test]
    fn large_project_with_five_hundred_shots_loads_within_budget() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "规模测试", "series").unwrap();
        let season_id = new_id();
        let episode_ids = (0..30).map(|_| new_id()).collect::<Vec<_>>();
        let script_id = new_id();
        let scene_ids = (0..10).map(|_| new_id()).collect::<Vec<_>>();
        let mut mutations = vec![request(
            "create",
            "contentUnit",
            Some(season_id.clone()),
            json!({"project_id": project.id, "type": "season", "name": "第一季", "sort_order": 0}),
        )];
        for (index, id) in episode_ids.iter().enumerate() {
            mutations.push(request("create", "contentUnit", Some(id.clone()), json!({"project_id": project.id, "parent_id": season_id, "type": "episode", "name": format!("EP{:02}", index + 1), "sort_order": index})));
        }
        mutations.push(request(
            "create",
            "script",
            Some(script_id.clone()),
            json!({"content_unit_id": episode_ids[0], "title": "EP01"}),
        ));
        for (index, id) in scene_ids.iter().enumerate() {
            mutations.push(request("create", "scene", Some(id.clone()), json!({"script_id": script_id, "title": format!("场{:02}", index + 1), "sort_order": index})));
        }
        for index in 0..500 {
            mutations.push(request("create", "shot", None, json!({"scene_id": scene_ids[index / 50], "title": format!("镜头{:03}", index + 1), "sort_order": index % 50, "duration": 2.0})));
        }
        let started = std::time::Instant::now();
        apply_batch_mutation(
            temp.path().to_string_lossy().to_string(),
            batch(mutations, "建立规模测试项目"),
        )
        .unwrap();
        let conn = open_database(temp.path()).unwrap();
        let state = crate::database::project_state(&conn).unwrap();
        assert_eq!(state["shots"].as_array().unwrap().len(), 500);
        assert!(started.elapsed().as_secs_f32() < 5.0);
    }

    #[test]
    fn snapshot_restores_business_state() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "测试", "series").unwrap();
        let created = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "asset",
                None,
                json!({"project_id": project.id, "type": "character", "name": "奶牛猫"}),
            ),
        )
        .unwrap();
        let snapshot = create_snapshot(
            temp.path().to_string_lossy().to_string(),
            "资产版本".into(),
            "".into(),
        )
        .unwrap();
        apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "patch",
                "asset",
                Some(created.object_id),
                json!({"name": "已修改"}),
            ),
        )
        .unwrap();
        restore_snapshot(temp.path().to_string_lossy().to_string(), snapshot.id).unwrap();
        let conn = open_database(temp.path()).unwrap();
        let name: String = conn
            .query_row("SELECT name FROM assets LIMIT 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(name, "奶牛猫");
    }

    #[test]
    fn structure_upgrade_preserves_downstream_ids() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "短片升级", "short").unwrap();
        let short = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "contentUnit",
                None,
                json!({"project_id": project.id, "parent_id": null, "type": "short", "name": "正片", "sort_order": 0}),
            ),
        )
        .unwrap();
        let script = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "script",
                None,
                json!({"content_unit_id": short.object_id, "title": "原短片"}),
            ),
        )
        .unwrap();
        let scene = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "scene",
                None,
                json!({"script_id": script.object_id, "title": "场01", "sort_order": 0}),
            ),
        )
        .unwrap();
        let shot = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "shot",
                None,
                json!({"scene_id": scene.object_id, "title": "镜头01", "sort_order": 0}),
            ),
        )
        .unwrap();
        let season = apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "create",
                "contentUnit",
                None,
                json!({"project_id": project.id, "parent_id": null, "type": "season", "name": "第一季", "sort_order": 0}),
            ),
        )
        .unwrap();
        apply_mutation(
            temp.path().to_string_lossy().to_string(),
            request(
                "move",
                "contentUnit",
                Some(short.object_id.clone()),
                json!({"parent_id": season.object_id, "type": "episode", "name": "EP01", "sort_order": 0}),
            ),
        )
        .unwrap();

        let conn = open_database(temp.path()).unwrap();
        let preserved_shot: String = conn
            .query_row(
                "SELECT id FROM shots WHERE id=?1",
                [&shot.object_id],
                |row| row.get(0),
            )
            .unwrap();
        let upgraded_parent: String = conn
            .query_row(
                "SELECT parent_id FROM content_units WHERE id=?1",
                [&short.object_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(preserved_shot, shot.object_id);
        assert_eq!(upgraded_parent, season.object_id);
    }

    #[test]
    fn goal_01_to_12_end_to_end_acceptance() {
        let root = tempfile::tempdir().unwrap();
        let project = create_project(
            root.path().to_string_lossy().to_string(),
            "智斗游戏".into(),
            "multi-season".into(),
        )
        .unwrap();
        let project_path = project.path.clone();

        let season = apply_mutation(
            project_path.clone(),
            request(
                "create",
                "contentUnit",
                None,
                json!({"project_id": project.id, "parent_id": null, "type": "season", "name": "第一季", "sort_order": 0}),
            ),
        )
        .unwrap();
        let mut episodes = Vec::new();
        for index in 0..30 {
            let episode = apply_mutation(
                project_path.clone(),
                request(
                    "create",
                    "contentUnit",
                    None,
                    json!({
                        "project_id": project.id,
                        "parent_id": season.object_id,
                        "type": "episode",
                        "name": format!("EP{:02}", index + 1),
                        "sort_order": index
                    }),
                ),
            )
            .unwrap();
            episodes.push(episode.object_id);
        }

        let script = apply_mutation(
            project_path.clone(),
            request(
                "create",
                "script",
                None,
                json!({"content_unit_id": episodes[0], "title": "EP01"}),
            ),
        )
        .unwrap();
        let mut scenes = Vec::new();
        for index in 0..3 {
            scenes.push(
                apply_mutation(
                    project_path.clone(),
                    request(
                        "create",
                        "scene",
                        None,
                        json!({
                            "script_id": script.object_id,
                            "title": format!("场 {:02}", index + 1),
                            "location_text": "游戏大厅",
                            "time_text": "日",
                            "content": format!("第 {} 场剧本文本", index + 1),
                            "sort_order": index
                        }),
                    ),
                )
                .unwrap()
                .object_id,
            );
        }

        let mut shots = Vec::new();
        for index in 0..10 {
            let scene_index = if index < 4 {
                0
            } else if index < 7 {
                1
            } else {
                2
            };
            shots.push(
                apply_mutation(
                    project_path.clone(),
                    request(
                        "create",
                        "shot",
                        None,
                        json!({
                            "scene_id": scenes[scene_index],
                            "sort_order": index % 4,
                            "title": format!("镜头 {:02}", index + 1),
                            "duration": 2.0,
                            "narrative_purpose": "推进游戏",
                            "subjects": "奶牛猫",
                            "action": "观察广播屏"
                        }),
                    ),
                )
                .unwrap()
                .object_id,
            );
        }
        let shot_04_id = shots[3].clone();
        apply_mutation(
            project_path.clone(),
            request(
                "patch",
                "shot",
                Some(shot_04_id.clone()),
                json!({
                    "duration": 3.5,
                    "shot_size": "中远景",
                    "composition": "三只猫构成不稳定三角，广播屏压在画面上方",
                    "dialogue": "规则并没有说出口。"
                }),
            ),
        )
        .unwrap();
        apply_mutation(
            project_path.clone(),
            request(
                "move",
                "shot",
                Some(shot_04_id.clone()),
                json!({"sort_order": 0}),
            ),
        )
        .unwrap();

        let asset_specs = [
            ("奶牛猫", "character"),
            ("大黄狗", "character"),
            ("暹罗猫", "character"),
            ("游戏大厅", "location"),
            ("广播屏", "prop"),
        ];
        let mut assets = Vec::new();
        for (name, asset_type) in asset_specs {
            assets.push(
                apply_mutation(
                    project_path.clone(),
                    request(
                        "create",
                        "asset",
                        None,
                        json!({
                            "project_id": project.id,
                            "type": asset_type,
                            "name": name,
                            "description": format!("{name}的视觉定义"),
                            "scope_unit_id": season.object_id
                        }),
                    ),
                )
                .unwrap()
                .object_id,
            );
        }
        let mut requirements = Vec::new();
        for (index, requirement_type) in ["标准主图", "背面"].iter().enumerate() {
            let requirement = apply_mutation(
                project_path.clone(),
                request(
                    "create",
                    "assetRequirement",
                    None,
                    json!({
                        "content_unit_id": episodes[0],
                        "asset_id": assets[0],
                        "asset_type": "character",
                        "requirement_type": requirement_type,
                        "description": "由分镜反推",
                        "prompt_draft": format!("奶牛猫 {requirement_type} 专业提示词"),
                        "status": "planned",
                        "created_from_type": "shot",
                        "created_from_id": shots[index + 3]
                    }),
                ),
            )
            .unwrap();
            apply_mutation(
                project_path.clone(),
                request(
                    "create",
                    "assetRequirementSource",
                    None,
                    json!({"asset_requirement_id": requirement.object_id.clone(), "source_type": "shot", "source_id": shots[index + 3]}),
                ),
            )
            .unwrap();
            requirements.push(requirement.object_id);
        }

        let source_image = root.path().join("test.png");
        fs::write(&source_image, [137, 80, 78, 71, 13, 10, 26, 10]).unwrap();
        let asset_image = import_project_file(
            project_path.clone(),
            source_image.to_string_lossy().to_string(),
            "character".into(),
        )
        .unwrap();
        let asset_media = apply_mutation(
            project_path.clone(),
            request(
                "create",
                "assetMedia",
                None,
                json!({
                    "asset_id": assets[0],
                    "media_type": "image",
                    "file_path": asset_image,
                    "label": "标准主图",
                    "is_primary": 1,
                    "source_type": "manual"
                }),
            ),
        )
        .unwrap();
        apply_mutation(
            project_path.clone(),
            request(
                "create",
                "assetMediaRequirement",
                None,
                json!({"asset_media_id": asset_media.object_id, "asset_requirement_id": requirements[0]}),
            ),
        )
        .unwrap();
        apply_mutation(
            project_path.clone(),
            request(
                "create",
                "shotAsset",
                None,
                json!({"shot_id": shot_04_id.clone(), "asset_id": assets[0], "role": "subject"}),
            ),
        )
        .unwrap();

        let keyframe = apply_mutation(
            project_path.clone(),
            request(
                "create",
                "keyframe",
                None,
                json!({
                    "shot_id": shot_04_id,
                    "type": "single",
                    "description": "广播屏压迫三只动物",
                    "prompt_draft": "中远景，游戏大厅，压迫式构图",
                    "status": "planned",
                    "sort_order": 0
                }),
            ),
        )
        .unwrap();
        let keyframe_image = import_project_file(
            project_path.clone(),
            source_image.to_string_lossy().to_string(),
            "keyframe".into(),
        )
        .unwrap();
        apply_mutation(
            project_path.clone(),
            request(
                "patch",
                "keyframe",
                Some(keyframe.object_id),
                json!({"file_path": keyframe_image, "status": "ready"}),
            ),
        )
        .unwrap();

        let generation_task = apply_mutation(
            project_path.clone(),
            request(
                "create",
                "generationTask",
                None,
                json!({
                    "content_unit_id": episodes[0],
                    "name": "生成任务01–05",
                    "target_model": "通用视频模型",
                    "duration": 11.5,
                    "prompt": "依据镜头01–05和正式视觉资产生成连续视频。",
                    "status": "draft"
                }),
            ),
        )
        .unwrap();
        for (index, shot_id) in shots.iter().take(5).enumerate() {
            apply_mutation(
                project_path.clone(),
                request(
                    "create",
                    "generationTaskShot",
                    None,
                    json!({
                        "generation_task_id": generation_task.object_id,
                        "shot_id": shot_id,
                        "sort_order": index
                    }),
                ),
            )
            .unwrap();
        }
        apply_mutation(
            project_path.clone(),
            request(
                "create",
                "relation",
                None,
                json!({
                    "project_id": project.id,
                    "source_type": "contentUnit",
                    "source_id": episodes[0],
                    "relation_type": "主线推进",
                    "target_type": "contentUnit",
                    "target_id": episodes[1],
                    "description": "EP01 的发现推动 EP02",
                    "importance": 2
                }),
            ),
        )
        .unwrap();

        let snapshot = create_snapshot(
            project_path.clone(),
            "第一版完整制作结构".into(),
            "端到端验收快照".into(),
        )
        .unwrap();
        apply_mutation(
            project_path.clone(),
            request(
                "patch",
                "shot",
                Some(shot_04_id.clone()),
                json!({"composition": "临时错误构图"}),
            ),
        )
        .unwrap();
        restore_snapshot(project_path.clone(), snapshot.id).unwrap();

        let reopened = open_project(project_path.clone()).unwrap();
        assert_eq!(reopened.name, "智斗游戏");
        let conn = open_database(Path::new(&project_path)).unwrap();
        let episode_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM content_units WHERE type='episode'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let scene_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM scenes", [], |row| row.get(0))
            .unwrap();
        let shot_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM shots", [], |row| row.get(0))
            .unwrap();
        let asset_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        let task_shot_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM generation_task_shots", [], |row| {
                row.get(0)
            })
            .unwrap();
        let shot_asset_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM shot_assets", [], |row| row.get(0))
            .unwrap();
        let requirement_source_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM asset_requirement_sources",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let media_requirement_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM asset_media_requirements", [], |row| {
                row.get(0)
            })
            .unwrap();
        let restored_composition: String = conn
            .query_row(
                "SELECT composition FROM shots WHERE id=?1",
                [&shot_04_id],
                |row| row.get(0),
            )
            .unwrap();
        let stable_id: String = conn
            .query_row("SELECT id FROM shots WHERE title='镜头 04'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let imported_asset_path: String = conn
            .query_row("SELECT file_path FROM asset_media LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        let foreign_key_violations: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(episode_count, 30);
        assert_eq!(scene_count, 3);
        assert_eq!(shot_count, 10);
        assert_eq!(asset_count, 5);
        assert_eq!(task_shot_count, 5);
        assert_eq!(shot_asset_count, 1);
        assert_eq!(requirement_source_count, 2);
        assert_eq!(media_requirement_count, 1);
        assert_eq!(stable_id, shot_04_id);
        assert_eq!(
            restored_composition,
            "三只猫构成不稳定三角，广播屏压在画面上方"
        );
        assert_eq!(integrity, "ok");
        assert_eq!(foreign_key_violations, 0);
        assert!(Path::new(&project_path).join(imported_asset_path).is_file());
    }
}
