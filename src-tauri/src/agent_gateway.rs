use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};
use std::path::Path;

use crate::app_database::load_feature_flags;
use crate::context::{
    neighbor_refs, object_value, parent_ref, search_project, ObjectRef, SelectionSnapshot,
};
use crate::database::{now, open_database, AppResult};
use crate::memory::{active_global_memories, active_project_memories, MemoryContextEntry};

const MAX_TOOL_RESULT_BYTES: usize = 64 * 1024;
const MAX_AUDIT_TEXT_BYTES: usize = 4 * 1024;

#[derive(Debug)]
pub struct ToolGatewayRequest {
    pub tool_call_id: String,
    pub task_id: String,
    pub session_id: String,
    pub tool_name: String,
    pub arguments: Value,
}

pub fn execute_tool(
    project_path: &Path,
    app_data_dir: Option<&Path>,
    request: ToolGatewayRequest,
) -> AppResult<Value> {
    let mut conn = open_database(project_path)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (session_id, agent_type, selection_json, write_scope_json): (
        String,
        String,
        String,
        String,
    ) = tx
        .query_row(
            "SELECT session_id, agent_type, selection_json, write_scope_json
             FROM agent_tasks WHERE id=?1",
            [&request.task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: AgentTask 不存在".to_string())?;
    if session_id != request.session_id {
        return Err("TOOL_SCOPE_DENIED: Tool Call 与 AgentSession 不匹配".into());
    }
    let (project_id, revision): (String, i64) = tx
        .query_row("SELECT id, revision FROM projects LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| e.to_string())?;
    let started_at = now();
    tx.execute(
        "INSERT INTO agent_tool_calls
         (id, task_id, session_id, agent_type, tool_name, arguments_json,
          project_revision, status, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8)",
        params![
            request.tool_call_id,
            request.task_id,
            request.session_id,
            agent_type,
            request.tool_name,
            audit_text(&request.arguments),
            revision,
            started_at,
        ],
    )
    .map_err(|e| format!("记录 Tool Call 失败：{e}"))?;

    let selection: SelectionSnapshot =
        serde_json::from_str(&selection_json).map_err(|e| format!("读取任务选区失败：{e}"))?;
    let result = argument_object(&request.arguments)
    .and_then(|arguments| {
        validate_arguments(&request.tool_name, arguments)?;
        execute_inner(
            &tx,
            app_data_dir,
            &project_id,
            &selection,
            &write_scope_json,
            &request.tool_name,
            arguments,
        )
    })
    .and_then(|data| {
        let wrapped = json!({ "projectRevision": revision, "data": data });
        let bytes = serde_json::to_vec(&wrapped).map_err(|e| e.to_string())?.len();
        if bytes > MAX_TOOL_RESULT_BYTES {
            return Err(format!(
                "TOOL_RESULT_TOO_LARGE: 工具结果 {bytes} bytes，最大允许 {MAX_TOOL_RESULT_BYTES} bytes"
            ));
        }
        Ok((wrapped, bytes))
    });

    let completed_at = now();
    let (status, summary) = match &result {
        Ok((value, bytes)) => (
            "completed",
            json!({ "bytes": bytes, "topLevelKeys": top_level_keys(value) }).to_string(),
        ),
        Err(error) => ("failed", audit_text(&json!({ "error": error }))),
    };
    tx.execute(
        "UPDATE agent_tool_calls
         SET result_summary_json=?1, status=?2, completed_at=?3 WHERE id=?4",
        params![summary, status, completed_at, request.tool_call_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_tasks SET tool_call_count=tool_call_count+1 WHERE id=?1",
        [&request.task_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    result.map(|(value, _)| value)
}

fn execute_inner(
    conn: &Connection,
    app_data_dir: Option<&Path>,
    project_id: &str,
    selection: &SelectionSnapshot,
    write_scope_json: &str,
    tool_name: &str,
    arguments: &Map<String, Value>,
) -> AppResult<Value> {
    match tool_name {
        "get_selection" => Ok(json!({
            "selection": selection,
            "writeScope": serde_json::from_str::<Value>(write_scope_json).map_err(|e| e.to_string())?,
        })),
        "read_object" => object_value(conn, &object_ref(project_id, arguments)?, false),
        "read_parent" => {
            let reference = object_ref(project_id, arguments)?;
            parent_ref(conn, &reference)?
                .map(|parent| object_item(conn, &parent, false))
                .transpose()
                .map(|value| value.unwrap_or(Value::Null))
        }
        "read_children" => read_children(
            conn,
            project_id,
            &string_arg(arguments, "objectType")?,
            &string_arg(arguments, "objectId")?,
            usize_arg(arguments, "limit", 40, 100)?,
        ),
        "read_neighbors" => {
            let reference = object_ref(project_id, arguments)?;
            let count = usize_arg(arguments, "count", 1, 5)?;
            neighbor_refs(conn, &reference, count)?
                .iter()
                .map(|item| object_item(conn, item, false))
                .collect::<AppResult<Vec<_>>>()
                .map(Value::Array)
        }
        "read_scene" => read_scene(conn, project_id, &string_arg(arguments, "sceneId")?),
        "read_shot_context" => {
            read_shot_context(conn, project_id, &string_arg(arguments, "shotId")?)
        }
        "read_asset" => read_asset(conn, project_id, &string_arg(arguments, "assetId")?),
        "read_generation_task" => read_generation_task(
            conn,
            project_id,
            &string_arg(arguments, "generationTaskId")?,
        ),
        "read_story_structure" => read_story_structure(
            conn,
            project_id,
            optional_string_arg(arguments, "scopeType")?
                .as_deref()
                .unwrap_or("project"),
            optional_string_arg(arguments, "scopeId")?.as_deref(),
            usize_arg(arguments, "limit", 120, 160)?,
        ),
        "search_project" => serde_json::to_value(search_project(
            conn,
            &string_arg(arguments, "query")?,
            usize_arg(arguments, "limit", 20, 50)?,
        )?)
        .map_err(|e| e.to_string()),
        "read_active_memories" => {
            read_active_memories(conn, app_data_dir, project_id, selection, arguments)
        }
        "read_change_set" => object_value(
            conn,
            &ObjectRef {
                project_id: project_id.into(),
                object_type: "changeSet".into(),
                object_id: string_arg(arguments, "changeSetId")?,
                field: None,
            },
            false,
        ),
        _ => Err(format!("TOOL_NOT_ALLOWED: 不支持的工作台工具：{tool_name}")),
    }
}

fn read_children(
    conn: &Connection,
    project_id: &str,
    object_type: &str,
    object_id: &str,
    limit: usize,
) -> AppResult<Value> {
    let parent = ObjectRef {
        project_id: project_id.into(),
        object_type: object_type.into(),
        object_id: object_id.into(),
        field: None,
    };
    object_value(conn, &parent, true)?;
    let mut references = Vec::new();
    match object_type {
        "project" => references.extend(query_refs(
            conn,
            "SELECT id FROM content_units WHERE project_id=?1 AND parent_id IS NULL ORDER BY sort_order, id LIMIT ?2",
            object_id,
            "contentUnit",
            project_id,
            limit,
        )?),
        "contentUnit" => {
            references.extend(query_refs(
                conn,
                "SELECT id FROM content_units WHERE parent_id=?1 ORDER BY sort_order, id LIMIT ?2",
                object_id,
                "contentUnit",
                project_id,
                limit,
            )?);
            if references.len() < limit {
                references.extend(query_refs(
                    conn,
                    "SELECT id FROM scripts WHERE content_unit_id=?1 ORDER BY id LIMIT ?2",
                    object_id,
                    "script",
                    project_id,
                    limit - references.len(),
                )?);
            }
        }
        "script" => references.extend(query_refs(conn, "SELECT id FROM scenes WHERE script_id=?1 ORDER BY sort_order, id LIMIT ?2", object_id, "scene", project_id, limit)?),
        "scene" => references.extend(query_refs(conn, "SELECT id FROM shots WHERE scene_id=?1 ORDER BY sort_order, id LIMIT ?2", object_id, "shot", project_id, limit)?),
        "shot" => references.extend(query_refs(conn, "SELECT id FROM keyframes WHERE shot_id=?1 ORDER BY sort_order, id LIMIT ?2", object_id, "keyframe", project_id, limit)?),
        "asset" => references.extend(query_refs(conn, "SELECT id FROM asset_requirements WHERE asset_id=?1 ORDER BY created_at, id LIMIT ?2", object_id, "assetRequirement", project_id, limit)?),
        "storyElement" => references.extend(query_refs(conn, "SELECT id FROM story_element_occurrences WHERE story_element_id=?1 ORDER BY sort_order, id LIMIT ?2", object_id, "storyElementOccurrence", project_id, limit)?),
        "generationTask" => references.extend(query_refs(conn, "SELECT shot_id FROM generation_task_shots WHERE generation_task_id=?1 ORDER BY sort_order, shot_id LIMIT ?2", object_id, "shot", project_id, limit)?),
        _ => return Err(format!("TOOL_ARGUMENT_INVALID: {object_type} 不支持 read_children")),
    }
    references
        .iter()
        .map(|reference| object_item(conn, reference, false))
        .collect::<AppResult<Vec<_>>>()
        .map(Value::Array)
}

fn read_scene(conn: &Connection, project_id: &str, scene_id: &str) -> AppResult<Value> {
    let scene = reference(project_id, "scene", scene_id);
    let shots = query_refs(
        conn,
        "SELECT id FROM shots WHERE scene_id=?1 ORDER BY sort_order, id LIMIT ?2",
        scene_id,
        "shot",
        project_id,
        100,
    )?
    .iter()
    .map(|item| object_value(conn, item, false))
    .collect::<AppResult<Vec<_>>>()?;
    Ok(json!({ "scene": object_value(conn, &scene, false)?, "shots": shots }))
}

fn read_shot_context(conn: &Connection, project_id: &str, shot_id: &str) -> AppResult<Value> {
    let shot = reference(project_id, "shot", shot_id);
    let scene =
        parent_ref(conn, &shot)?.ok_or_else(|| "OBJECT_NOT_FOUND: 镜头缺少所属场".to_string())?;
    let neighbors = neighbor_refs(conn, &shot, 2)?
        .iter()
        .map(|item| object_value(conn, item, false))
        .collect::<AppResult<Vec<_>>>()?;
    let asset_ids = query_ids(
        conn,
        "SELECT asset_id FROM shot_assets WHERE shot_id=?1 ORDER BY created_at, asset_id LIMIT ?2",
        shot_id,
        24,
    )?;
    let assets = asset_ids
        .iter()
        .map(|asset_id| read_asset(conn, project_id, asset_id))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(json!({
        "shot": object_value(conn, &shot, false)?,
        "scene": object_value(conn, &scene, false)?,
        "neighbors": neighbors,
        "assets": assets,
    }))
}

fn read_asset(conn: &Connection, project_id: &str, asset_id: &str) -> AppResult<Value> {
    let asset = reference(project_id, "asset", asset_id);
    let media = {
        let mut statement = conn.prepare(
            "SELECT id, media_type, label, description, sort_order, is_primary, source_type, created_at
             FROM asset_media WHERE asset_id=?1 ORDER BY is_primary DESC, sort_order, id LIMIT 40",
        ).map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([asset_id], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?, "mediaType": row.get::<_, String>(1)?,
                    "label": row.get::<_, String>(2)?, "description": row.get::<_, String>(3)?,
                    "sortOrder": row.get::<_, i64>(4)?, "isPrimary": row.get::<_, i64>(5)? != 0,
                    "sourceType": row.get::<_, String>(6)?, "createdAt": row.get::<_, String>(7)?,
                }))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    let requirements = query_refs(
        conn,
        "SELECT id FROM asset_requirements WHERE asset_id=?1 ORDER BY created_at, id LIMIT ?2",
        asset_id,
        "assetRequirement",
        project_id,
        40,
    )?
    .iter()
    .map(|item| object_value(conn, item, false))
    .collect::<AppResult<Vec<_>>>()?;
    let shots = query_refs(
        conn,
        "SELECT shot_id FROM shot_assets WHERE asset_id=?1 ORDER BY created_at, shot_id LIMIT ?2",
        asset_id,
        "shot",
        project_id,
        40,
    )?
    .iter()
    .map(|item| object_value(conn, item, true))
    .collect::<AppResult<Vec<_>>>()?;
    Ok(
        json!({ "asset": object_value(conn, &asset, false)?, "media": media, "requirements": requirements, "shots": shots }),
    )
}

fn read_generation_task(conn: &Connection, project_id: &str, task_id: &str) -> AppResult<Value> {
    let task = reference(project_id, "generationTask", task_id);
    let shots = query_refs(conn, "SELECT shot_id FROM generation_task_shots WHERE generation_task_id=?1 ORDER BY sort_order, shot_id LIMIT ?2", task_id, "shot", project_id, 100)?
        .iter().map(|item| object_value(conn, item, false)).collect::<AppResult<Vec<_>>>()?;
    let compilations = {
        let mut statement = conn.prepare(
            "SELECT id, model_profile_key, model_profile_version, template_id, template_version,
                    source_revision, current_prompt, warnings_json, status, created_at
             FROM prompt_compilations WHERE generation_task_id=?1 ORDER BY created_at DESC LIMIT 10",
        ).map_err(|e| e.to_string())?;
        let rows = statement.query_map([task_id], |row| {
            let warnings: String = row.get(7)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?, "modelProfileKey": row.get::<_, String>(1)?,
                "modelProfileVersion": row.get::<_, String>(2)?, "templateId": row.get::<_, String>(3)?,
                "templateVersion": row.get::<_, String>(4)?, "sourceRevision": row.get::<_, i64>(5)?,
                "currentPrompt": row.get::<_, Option<String>>(6)?,
                "warnings": serde_json::from_str::<Value>(&warnings).unwrap_or(Value::Array(Vec::new())),
                "status": row.get::<_, String>(8)?, "createdAt": row.get::<_, String>(9)?,
            }))
        }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        rows
    };
    Ok(
        json!({ "generationTask": object_value(conn, &task, false)?, "shots": shots, "compilations": compilations }),
    )
}

fn read_story_structure(
    conn: &Connection,
    project_id: &str,
    scope_type: &str,
    scope_id: Option<&str>,
    limit: usize,
) -> AppResult<Value> {
    let unit_ids = match scope_type {
        "project" => query_ids(
            conn,
            "SELECT id FROM content_units WHERE project_id=?1 ORDER BY sort_order, id LIMIT ?2",
            project_id,
            limit,
        )?,
        "season" | "episode" | "contentUnit" => {
            let scope_id = scope_id.ok_or("TOOL_ARGUMENT_INVALID: scopeId 不能为空")?;
            object_value(conn, &reference(project_id, "contentUnit", scope_id), true)?;
            let mut statement = conn
                .prepare(
                    "WITH RECURSIVE tree(id, path) AS (
                   SELECT id, printf('%08d', sort_order) FROM content_units WHERE id=?1
                   UNION ALL
                   SELECT child.id, tree.path || '.' || printf('%08d', child.sort_order)
                   FROM content_units child JOIN tree ON child.parent_id=tree.id
                 ) SELECT id FROM tree ORDER BY path LIMIT ?2",
                )
                .map_err(|e| e.to_string())?;
            let ids = statement
                .query_map(params![scope_id, limit as i64], |row| row.get(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<String>, _>>()
                .map_err(|e| e.to_string())?;
            ids
        }
        "storyElement" => Vec::new(),
        _ => return Err("TOOL_ARGUMENT_INVALID: read_story_structure scopeType 无效".into()),
    };
    let units = unit_ids
        .iter()
        .map(|id| object_value(conn, &reference(project_id, "contentUnit", id), false))
        .collect::<AppResult<Vec<_>>>()?;
    let element_ids = if scope_type == "storyElement" {
        let id = scope_id.ok_or("TOOL_ARGUMENT_INVALID: scopeId 不能为空")?;
        object_value(conn, &reference(project_id, "storyElement", id), true)?;
        vec![id.to_string()]
    } else if unit_ids.is_empty() {
        query_ids(
            conn,
            "SELECT id FROM story_elements WHERE project_id=?1 ORDER BY created_at, id LIMIT ?2",
            project_id,
            limit,
        )?
    } else {
        let placeholders = std::iter::repeat_n("?", unit_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT id FROM story_elements WHERE project_id=?1 AND (scope_unit_id IS NULL OR scope_unit_id IN ({placeholders})) ORDER BY created_at, id LIMIT ?{}", unit_ids.len() + 2);
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![&project_id];
        values.extend(unit_ids.iter().map(|id| id as &dyn rusqlite::ToSql));
        let limit_value = limit as i64;
        values.push(&limit_value);
        let mut statement = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let ids = statement
            .query_map(values.as_slice(), |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| e.to_string())?;
        ids
    };
    let elements = element_ids
        .iter()
        .map(|id| object_value(conn, &reference(project_id, "storyElement", id), false))
        .collect::<AppResult<Vec<_>>>()?;
    let mut occurrence_refs = Vec::new();
    for id in &element_ids {
        occurrence_refs.extend(query_refs(
            conn,
            "SELECT id FROM story_element_occurrences WHERE story_element_id=?1 ORDER BY sort_order, id LIMIT ?2",
            id,
            "storyElementOccurrence",
            project_id,
            limit.saturating_sub(occurrence_refs.len()),
        )?);
        if occurrence_refs.len() >= limit {
            break;
        }
    }
    let occurrences = occurrence_refs
        .iter()
        .map(|item| object_value(conn, item, false))
        .collect::<AppResult<Vec<_>>>()?;
    let relations = query_refs(conn, "SELECT id FROM relations WHERE project_id=?1 ORDER BY importance DESC, created_at, id LIMIT ?2", project_id, "relation", project_id, limit)?
        .iter().map(|item| object_value(conn, item, false)).collect::<AppResult<Vec<_>>>()?;
    Ok(
        json!({ "scopeType": scope_type, "scopeId": scope_id, "contentUnits": units, "storyElements": elements, "occurrences": occurrences, "relations": relations }),
    )
}

fn read_active_memories(
    conn: &Connection,
    app_data_dir: Option<&Path>,
    project_id: &str,
    selection: &SelectionSnapshot,
    arguments: &Map<String, Value>,
) -> AppResult<Value> {
    let object_type = optional_string_arg(arguments, "objectType")?;
    let object_id = optional_string_arg(arguments, "objectId")?;
    if object_type.is_some() != object_id.is_some() {
        return Err("TOOL_ARGUMENT_INVALID: objectType 和 objectId 必须同时提供".into());
    }
    let center = match (object_type, object_id) {
        (Some(object_type), Some(object_id)) => reference(project_id, &object_type, &object_id),
        _ => selection
            .center
            .clone()
            .unwrap_or_else(|| reference(project_id, "project", project_id)),
    };
    object_value(conn, &center, true)?;
    let project = active_project_memories(conn, &center)?;
    let global = match app_data_dir {
        Some(path) if load_feature_flags(path)?.get("memory") == Some(&true) => {
            active_global_memories(path)?
        }
        _ => Vec::new(),
    };
    Ok(json!({
        "projectMemories": project.iter().map(memory_value).collect::<Vec<_>>(),
        "globalMemories": global.iter().take(8).map(memory_value).collect::<Vec<_>>(),
    }))
}

fn memory_value(entry: &MemoryContextEntry) -> Value {
    json!({
        "id": entry.id, "storage": entry.storage, "scopeType": entry.scope_type,
        "scopeId": entry.scope_id, "category": entry.category, "content": entry.content,
        "sourceType": entry.source_type, "priority": entry.priority, "updatedAt": entry.updated_at,
    })
}

fn query_refs(
    conn: &Connection,
    sql: &str,
    id: &str,
    object_type: &str,
    project_id: &str,
    limit: usize,
) -> AppResult<Vec<ObjectRef>> {
    query_ids(conn, sql, id, limit).map(|ids| {
        ids.into_iter()
            .map(|id| reference(project_id, object_type, &id))
            .collect()
    })
}

fn query_ids(conn: &Connection, sql: &str, id: &str, limit: usize) -> AppResult<Vec<String>> {
    let mut statement = conn.prepare(sql).map_err(|e| e.to_string())?;
    let ids = statement
        .query_map(params![id, limit as i64], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(ids)
}

fn object_item(conn: &Connection, reference: &ObjectRef, compact: bool) -> AppResult<Value> {
    Ok(
        json!({ "objectType": reference.object_type, "object": object_value(conn, reference, compact)? }),
    )
}

fn object_ref(project_id: &str, arguments: &Map<String, Value>) -> AppResult<ObjectRef> {
    Ok(reference(
        project_id,
        &string_arg(arguments, "objectType")?,
        &string_arg(arguments, "objectId")?,
    ))
}

fn reference(project_id: &str, object_type: &str, object_id: &str) -> ObjectRef {
    ObjectRef {
        project_id: project_id.into(),
        object_type: object_type.into(),
        object_id: object_id.into(),
        field: None,
    }
}

fn argument_object(arguments: &Value) -> AppResult<&Map<String, Value>> {
    arguments
        .as_object()
        .ok_or_else(|| "TOOL_ARGUMENT_INVALID: arguments 必须是对象".into())
}

fn validate_arguments(tool_name: &str, arguments: &Map<String, Value>) -> AppResult<()> {
    let allowed: &[&str] = match tool_name {
        "get_selection" => &[],
        "read_object" | "read_parent" => &["objectType", "objectId"],
        "read_children" => &["objectType", "objectId", "limit"],
        "read_neighbors" => &["objectType", "objectId", "count"],
        "read_scene" => &["sceneId"],
        "read_shot_context" => &["shotId"],
        "read_asset" => &["assetId"],
        "read_generation_task" => &["generationTaskId"],
        "read_story_structure" => &["scopeType", "scopeId", "limit"],
        "search_project" => &["query", "limit"],
        "read_active_memories" => &["objectType", "objectId"],
        "read_change_set" => &["changeSetId"],
        _ => return Err(format!("TOOL_NOT_ALLOWED: 不支持的工作台工具：{tool_name}")),
    };
    if let Some(key) = arguments
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
    {
        return Err(format!("TOOL_ARGUMENT_INVALID: 不允许参数 {key}"));
    }
    Ok(())
}

fn string_arg(arguments: &Map<String, Value>, key: &str) -> AppResult<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .map(str::to_string)
        .ok_or_else(|| format!("TOOL_ARGUMENT_INVALID: {key} 不能为空或过长"))
}

fn optional_string_arg(arguments: &Map<String, Value>, key: &str) -> AppResult<Option<String>> {
    arguments
        .get(key)
        .map(|_| string_arg(arguments, key))
        .transpose()
}

fn usize_arg(
    arguments: &Map<String, Value>,
    key: &str,
    default: usize,
    maximum: usize,
) -> AppResult<usize> {
    match arguments.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0 && *value <= maximum as u64)
            .map(|value| value as usize)
            .ok_or_else(|| format!("TOOL_ARGUMENT_INVALID: {key} 必须在 1..={maximum}")),
    }
}

fn audit_text(value: &Value) -> String {
    let redacted = redact(value);
    truncate_utf8(&redacted.to_string(), MAX_AUDIT_TEXT_BYTES)
}

fn redact(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let value = if lower.contains("key")
                        || lower.contains("token")
                        || lower.contains("secret")
                        || lower.contains("path")
                        || lower.contains("image")
                    {
                        Value::String("[redacted]".into())
                    } else {
                        redact(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().take(32).map(redact).collect()),
        Value::String(value) if value.len() > 512 => Value::String(truncate_utf8(value, 512)),
        value => value.clone(),
    }
}

fn truncate_utf8(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.into();
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn top_level_keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{init_database, now};

    fn setup_project() -> (tempfile::TempDir, String) {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "Tool Gateway", "series").unwrap();
        let conn = open_database(temp.path()).unwrap();
        let timestamp = now();
        conn.execute("UPDATE projects SET revision=7 WHERE id=?1", [&project.id])
            .unwrap();
        conn.execute("INSERT INTO content_units (id, project_id, type, name, summary, sort_order, created_at, updated_at) VALUES ('episode', ?1, 'episode', '第10集', '压迫感测试', 10, ?2, ?2)", params![project.id, timestamp]).unwrap();
        conn.execute("INSERT INTO scripts (id, content_unit_id, title, created_at, updated_at) VALUES ('script', 'episode', '第10集', ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO scenes (id, script_id, title, sort_order, content, created_at, updated_at) VALUES ('scene', 'script', '密室', 0, '角色被围困', ?1, ?1)", [&timestamp]).unwrap();
        for (id, order, title) in [
            ("shot03", 3, "镜头03"),
            ("shot04", 4, "镜头04"),
            ("shot05", 5, "镜头05"),
        ] {
            conn.execute("INSERT INTO shots (id, scene_id, sort_order, title, composition, subjects, environment, created_at, updated_at) VALUES (?1, 'scene', ?2, ?3, '平视中景', '主角', '狭窄密室', ?4, ?4)", params![id, order, title, timestamp]).unwrap();
        }
        conn.execute("INSERT INTO assets (id, project_id, type, name, description, created_at, updated_at) VALUES ('hero', ?1, 'character', '主角', '正式角色资产', ?2, ?2)", params![project.id, timestamp]).unwrap();
        conn.execute("INSERT INTO asset_media (id, asset_id, file_path, label, is_primary, created_at, updated_at) VALUES ('hero-image', 'hero', 'C:/private/hero.png', '正面设定', 1, ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO shot_assets (id, shot_id, asset_id, role, created_at, updated_at) VALUES ('shot-hero', 'shot04', 'hero', 'subject', ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO generation_tasks (id, content_unit_id, name, target_model, prompt, created_at, updated_at) VALUES ('generation', 'episode', '静态关键帧', 'gpt-image', '密室压迫感', ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO generation_task_shots (generation_task_id, shot_id, sort_order) VALUES ('generation', 'shot04', 0)", []).unwrap();
        conn.execute("INSERT INTO story_elements (id, project_id, type, name, description, created_at, updated_at) VALUES ('clue', ?1, 'clue', '钥匙线索', '第10集回收线索', ?2, ?2)", params![project.id, timestamp]).unwrap();
        conn.execute("INSERT INTO story_element_occurrences (id, story_element_id, content_unit_id, occurrence_type, description, sort_order, created_at, updated_at) VALUES ('clue-payoff', 'clue', 'episode', 'payoff', '线索回收', 0, ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO project_memories (id, scope_type, scope_id, category, content, status, created_at, updated_at) VALUES ('memory', 'project', ?1, 'decision', '保持密室压迫感', 'active', ?2, ?2)", params![project.id, timestamp]).unwrap();
        conn.execute("INSERT INTO change_sets (id, project_id, name, source_type, status, created_at) VALUES ('changeset', ?1, '本轮修改', 'user', 'open', ?2)", params![project.id, timestamp]).unwrap();
        conn.execute("INSERT INTO changes (id, change_set_id, object_type, object_id, field_name, old_value, new_value, source_type, created_at) VALUES ('change', 'changeset', 'shot', 'shot04', 'composition', '\"旧构图\"', '\"平视中景\"', 'user', ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO agent_sessions (id, project_id, scope_type, scope_id, title, status, session_kind, session_status, last_active_at, created_at, updated_at) VALUES ('session', ?1, 'project', ?1, '工具测试', 'active', 'main', 'active', ?2, ?2, ?2)", params![project.id, timestamp]).unwrap();
        let selection = json!({
            "projectId": project.id,
            "center": { "projectId": project.id, "objectType": "shot", "objectId": "shot04", "field": "composition" },
            "selected": [{ "projectId": project.id, "objectType": "shot", "objectId": "shot04", "field": "composition" }],
            "projectRevision": 7,
        });
        conn.execute("INSERT INTO agent_tasks (id, session_id, task_type, interaction_mode, agent_type, selection_json, read_scope_json, write_scope_json, context_revision, base_revision, status, created_at) VALUES ('task', 'session', 'discussion', 'discussion', 'main', ?1, '[]', '{\"refs\":[],\"protectedRefs\":[]}', 7, 7, 'running', ?2)", params![selection.to_string(), timestamp]).unwrap();
        (temp, project.id)
    }

    fn call(path: &Path, id: &str, tool_name: &str, arguments: Value) -> AppResult<Value> {
        execute_tool(
            path,
            None,
            ToolGatewayRequest {
                tool_call_id: id.into(),
                task_id: "task".into(),
                session_id: "session".into(),
                tool_name: tool_name.into(),
                arguments,
            },
        )
    }

    #[test]
    fn executes_all_read_tools_with_current_revision_and_audit() {
        let (temp, _) = setup_project();
        let cases = [
            ("get_selection", json!({})),
            (
                "read_object",
                json!({ "objectType": "shot", "objectId": "shot04" }),
            ),
            (
                "read_parent",
                json!({ "objectType": "shot", "objectId": "shot04" }),
            ),
            (
                "read_children",
                json!({ "objectType": "scene", "objectId": "scene" }),
            ),
            (
                "read_neighbors",
                json!({ "objectType": "shot", "objectId": "shot04" }),
            ),
            ("read_scene", json!({ "sceneId": "scene" })),
            ("read_shot_context", json!({ "shotId": "shot04" })),
            ("read_asset", json!({ "assetId": "hero" })),
            (
                "read_generation_task",
                json!({ "generationTaskId": "generation" }),
            ),
            ("read_story_structure", json!({ "scopeType": "project" })),
            ("search_project", json!({ "query": "钥匙线索" })),
            ("read_active_memories", json!({})),
            ("read_change_set", json!({ "changeSetId": "changeset" })),
        ];
        for (index, (name, arguments)) in cases.into_iter().enumerate() {
            let result = call(temp.path(), &format!("tool-{index}"), name, arguments).unwrap();
            assert_eq!(result["projectRevision"], 7, "{name}");
        }
        let asset = call(
            temp.path(),
            "tool-asset-redaction",
            "read_asset",
            json!({ "assetId": "hero" }),
        )
        .unwrap();
        let asset_text = asset.to_string();
        assert!(!asset_text.contains("C:/private"));
        assert!(!asset_text.contains("file_path"));
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM agent_tool_calls WHERE status='completed'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            14
        );
        assert_eq!(
            conn.query_row(
                "SELECT tool_call_count FROM agent_tasks WHERE id='task'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            14
        );
    }

    #[test]
    fn rejects_unsafe_tool_inputs_limits_results_and_reads_fresh_revision() {
        let (temp, _) = setup_project();
        for (id, tool, arguments) in [
            (
                "bad-type",
                "read_object",
                json!({ "objectType": "shots; DROP TABLE projects", "objectId": "shot04" }),
            ),
            (
                "bad-id",
                "read_object",
                json!({ "objectType": "shot", "objectId": "C:/Windows/System32/config/SAM" }),
            ),
            (
                "bad-path",
                "read_object",
                json!({ "objectType": "shot", "objectId": "shot04", "path": "C:/Windows/win.ini" }),
            ),
            (
                "bad-tool",
                "read_file",
                json!({ "path": "C:/Windows/win.ini" }),
            ),
            (
                "sql",
                "search_project",
                json!({ "query": "\" OR 1=1; DROP TABLE projects; --" }),
            ),
        ] {
            if id == "sql" {
                call(temp.path(), id, tool, arguments).unwrap();
            } else {
                assert!(call(temp.path(), id, tool, arguments).is_err(), "{id}");
            }
        }
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM projects", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        conn.execute(
            "UPDATE scenes SET content=?1 WHERE id='scene'",
            ["压".repeat(70_000)],
        )
        .unwrap();
        drop(conn);
        let error = call(
            temp.path(),
            "too-large",
            "read_scene",
            json!({ "sceneId": "scene" }),
        )
        .unwrap_err();
        assert!(error.starts_with("TOOL_RESULT_TOO_LARGE"));
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT status FROM agent_tool_calls WHERE id='too-large'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "failed"
        );
        conn.execute("UPDATE projects SET revision=8", []).unwrap();
        drop(conn);
        assert_eq!(
            call(temp.path(), "fresh", "get_selection", json!({})).unwrap()["projectRevision"],
            8
        );
    }
}
