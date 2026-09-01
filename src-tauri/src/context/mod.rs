use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::Path;
use tauri::Manager;

use crate::agent_runtime::ensure_agent_core_enabled;
use crate::app_database::load_feature_flags;
use crate::database::{new_id, now, open_database, row_to_json, AppResult};
use crate::memory::{active_global_memories, active_project_memories, MemoryContextEntry};

pub(crate) const CONTEXT_POLICY_VERSION: &str = "context-v3";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectRef {
    pub project_id: String,
    pub object_type: String,
    pub object_id: String,
    pub field: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSnapshot {
    pub project_id: String,
    pub center: Option<ObjectRef>,
    pub selected: Vec<ObjectRef>,
    pub project_revision: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildContextInput {
    pub task_id: String,
    pub selection: SelectionSnapshot,
    pub task_intent: String,
    pub expert_type: String,
    pub token_budget: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextItem {
    pub reference: ObjectRef,
    pub source: String,
    pub data: Value,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackage {
    pub id: String,
    pub task_id: String,
    pub project_revision: i64,
    pub policy_version: String,
    pub center_ref: ObjectRef,
    pub included_items: Vec<ContextItem>,
    pub included_memory_ids: Vec<String>,
    pub omitted_summary: Vec<String>,
    pub token_estimate: usize,
    pub checksum: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub reference: ObjectRef,
    pub title: String,
    pub snippet: String,
    pub rank: f64,
}

#[derive(Debug, Clone)]
pub struct ContextPolicy {
    pub parent_depth: usize,
    pub neighbor_count: usize,
    pub relation_limit: usize,
}

impl ContextPolicy {
    pub fn for_intent(task_intent: &str) -> Self {
        if task_intent == "project_planning" {
            Self {
                parent_depth: 8,
                neighbor_count: 0,
                relation_limit: 24,
            }
        } else {
            Self {
                parent_depth: 6,
                neighbor_count: 1,
                relation_limit: 12,
            }
        }
    }
}

#[derive(Debug)]
struct Candidate {
    reference: ObjectRef,
    source: &'static str,
    data: Value,
    memory_id: Option<String>,
}

pub fn build_context_with_memories(
    conn: &mut Connection,
    input: BuildContextInput,
    global_memories: Option<&[MemoryContextEntry]>,
) -> AppResult<ContextPackage> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("开始构建 ContextPackage 失败：{e}"))?;
    let package = build_context_in_transaction(&tx, input, global_memories)?;
    tx.commit()
        .map_err(|e| format!("提交 ContextPackage 失败：{e}"))?;
    Ok(package)
}

fn build_context_in_transaction(
    conn: &Connection,
    input: BuildContextInput,
    global_memories: Option<&[MemoryContextEntry]>,
) -> AppResult<ContextPackage> {
    if !(32..=100_000).contains(&input.token_budget) {
        return Err("Context token budget 必须在 32–100000 之间".into());
    }
    let (project_id, project_revision): (String, i64) = conn
        .query_row("SELECT id, revision FROM projects LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| format!("读取项目 revision 失败：{e}"))?;
    if input.selection.project_id != project_id {
        return Err("SelectionSnapshot 不属于当前项目".into());
    }
    if input.selection.project_revision != project_revision {
        return Err(format!(
            "SelectionSnapshot revision 已过期：{} != {project_revision}",
            input.selection.project_revision
        ));
    }
    let center = input
        .selection
        .center
        .clone()
        .ok_or_else(|| "SelectionSnapshot 缺少中心对象".to_string())?;
    validate_ref_project(&center, &project_id)?;
    for reference in &input.selection.selected {
        validate_ref_project(reference, &project_id)?;
    }

    let policy = ContextPolicy::for_intent(&input.task_intent);
    let mut candidates = vec![Candidate {
        data: object_value(conn, &center, false)?,
        reference: center.clone(),
        source: "center",
        memory_id: None,
    }];
    for reference in &input.selection.selected {
        if reference != &center {
            candidates.push(Candidate {
                data: object_value(conn, reference, true)?,
                reference: reference.clone(),
                source: "selection",
                memory_id: None,
            });
        }
    }

    let mut parent = center.clone();
    for _ in 0..policy.parent_depth {
        let Some(next) = parent_ref(conn, &parent)? else {
            break;
        };
        candidates.push(Candidate {
            data: object_value(conn, &next, true)?,
            reference: next.clone(),
            source: "parent",
            memory_id: None,
        });
        parent = next;
    }
    for reference in neighbor_refs(conn, &center, policy.neighbor_count)? {
        candidates.push(Candidate {
            data: object_value(conn, &reference, true)?,
            reference,
            source: "neighbor",
            memory_id: None,
        });
    }
    for (reference, data) in relation_items(conn, &center, policy.relation_limit)? {
        candidates.push(Candidate {
            reference,
            source: "relation",
            data,
            memory_id: None,
        });
    }
    for (reference, source, data) in intent_items(conn, &center, &input.task_intent)? {
        candidates.push(Candidate {
            reference,
            source,
            data,
            memory_id: None,
        });
    }
    if center.object_type == "changeSet" {
        for affected in affected_refs(conn, &center)? {
            let Ok(data) = object_value(conn, &affected, true) else {
                continue;
            };
            candidates.push(Candidate {
                reference: affected.clone(),
                source: "affected",
                memory_id: None,
                data,
            });
            let mut parent = affected.clone();
            for _ in 0..policy.parent_depth {
                let Some(next) = parent_ref(conn, &parent)? else {
                    break;
                };
                candidates.push(Candidate {
                    data: object_value(conn, &next, true)?,
                    reference: next.clone(),
                    source: "parent",
                    memory_id: None,
                });
                parent = next;
            }
            for reference in neighbor_refs(conn, &affected, policy.neighbor_count)? {
                candidates.push(Candidate {
                    data: object_value(conn, &reference, true)?,
                    reference,
                    source: "neighbor",
                    memory_id: None,
                });
            }
            for (reference, data) in relation_items(conn, &affected, policy.relation_limit)? {
                candidates.push(Candidate {
                    reference,
                    source: "relation",
                    data,
                    memory_id: None,
                });
            }
        }
    }

    if let Some(global_memories) = global_memories {
        let mut memories = active_project_memories(conn, &center)?;
        memories.extend(global_memories.iter().take(4).cloned());
        for memory in memories {
            candidates.push(Candidate {
                reference: ObjectRef {
                    project_id: project_id.clone(),
                    object_type: if memory.storage == "project" {
                        "projectMemory".into()
                    } else {
                        "longTermMemory".into()
                    },
                    object_id: memory.id.clone(),
                    field: None,
                },
                source: "memory",
                data: json!({
                    "category": memory.category,
                    "content": memory.content,
                    "scopeType": memory.scope_type,
                    "scopeId": memory.scope_id,
                    "sourceType": memory.source_type,
                    "priority": memory.priority,
                }),
                memory_id: Some(memory.id),
            });
        }
    }

    let mut seen = HashSet::new();
    let mut included_items = Vec::new();
    let mut omitted_summary = Vec::new();
    let mut included_memory_ids = Vec::new();
    let mut token_estimate = 0;
    for candidate in candidates {
        let key = (
            candidate.reference.object_type.clone(),
            candidate.reference.object_id.clone(),
            candidate.reference.field.clone(),
        );
        if !seen.insert(key) {
            continue;
        }
        let remaining = input.token_budget.saturating_sub(token_estimate);
        let estimate = estimate_tokens(&candidate.data.to_string());
        let fitted = if estimate <= remaining {
            Some((candidate.data, estimate))
        } else if candidate.source == "center" {
            fit_value(candidate.data, remaining)
        } else {
            None
        };
        if let Some((data, estimate)) = fitted {
            token_estimate += estimate;
            if let Some(memory_id) = candidate.memory_id {
                included_memory_ids.push(memory_id);
            }
            included_items.push(ContextItem {
                reference: candidate.reference,
                source: candidate.source.into(),
                data,
                token_estimate: estimate,
            });
        } else {
            omitted_summary.push(format!(
                "{}:{} ({})",
                candidate.reference.object_type, candidate.reference.object_id, candidate.source
            ));
        }
    }

    let checksum_input = json!({
        "projectRevision": project_revision,
        "policyVersion": CONTEXT_POLICY_VERSION,
        "taskIntent": input.task_intent,
        "expertType": input.expert_type,
        "centerRef": center,
        "items": included_items,
        "memoryIds": included_memory_ids,
        "omitted": omitted_summary,
    });
    let checksum = stable_checksum(checksum_input.to_string().as_bytes());
    let package = ContextPackage {
        id: new_id(),
        task_id: input.task_id,
        project_revision,
        policy_version: CONTEXT_POLICY_VERSION.into(),
        center_ref: center,
        included_items,
        included_memory_ids,
        omitted_summary,
        token_estimate,
        checksum,
        created_at: now(),
    };
    store_context_package(conn, &package)?;
    Ok(package)
}

pub fn search_project(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> AppResult<Vec<SearchResult>> {
    let query = query.trim();
    if query.is_empty() {
        return Err("搜索内容不能为空".into());
    }
    let limit = limit.clamp(1, 50);
    // ponytail: Goal15 uses a temporary FTS index; persist it only when search profiling justifies migration/trigger complexity.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS temp.context_search_fts USING fts5(object_type UNINDEXED, object_id UNINDEXED, title, body, tokenize='unicode61');
         DELETE FROM temp.context_search_fts;
         INSERT INTO temp.context_search_fts SELECT 'contentUnit', id, name, summary FROM content_units;
         INSERT INTO temp.context_search_fts SELECT 'script', id, title, summary FROM scripts;
         INSERT INTO temp.context_search_fts SELECT 'scene', id, title, location_text || ' ' || time_text || ' ' || summary || ' ' || content FROM scenes;
         INSERT INTO temp.context_search_fts SELECT 'shot', id, title, narrative_purpose || ' ' || new_information || ' ' || subjects || ' ' || action || ' ' || dialogue || ' ' || environment FROM shots;
         INSERT INTO temp.context_search_fts SELECT 'asset', id, name, description FROM assets;
         INSERT INTO temp.context_search_fts SELECT 'relation', id, relation_type, description FROM relations;
         INSERT INTO temp.context_search_fts SELECT 'storyElement', id, name, description FROM story_elements;
         INSERT INTO temp.context_search_fts SELECT 'storyElementOccurrence', id, occurrence_type, description FROM story_element_occurrences;
         INSERT INTO temp.context_search_fts SELECT 'projectMemory', id, category, content FROM project_memories WHERE status='active';",
    )
    .map_err(|e| format!("建立 FTS 索引失败：{e}"))?;
    let project_id: String = conn
        .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let phrase = format!("\"{}\"", query.replace('"', "\"\""));
    let mut stmt = conn
        .prepare(
            "SELECT object_type, object_id, title, snippet(context_search_fts, 3, '[', ']', '…', 18), bm25(context_search_fts)
             FROM temp.context_search_fts WHERE context_search_fts MATCH ?1 ORDER BY bm25(context_search_fts) LIMIT ?2",
        )
        .map_err(|e| format!("准备 FTS 搜索失败：{e}"))?;
    let rows = stmt
        .query_map(params![phrase, limit as i64], |row| {
            Ok(SearchResult {
                reference: ObjectRef {
                    project_id: project_id.clone(),
                    object_type: row.get(0)?,
                    object_id: row.get(1)?,
                    field: None,
                },
                title: row.get(2)?,
                snippet: row.get(3)?,
                rank: row.get(4)?,
            })
        })
        .map_err(|e| format!("执行 FTS 搜索失败：{e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("读取 FTS 搜索结果失败：{e}"))
}

#[tauri::command]
pub fn context_build(
    app: tauri::AppHandle,
    project_path: String,
    input: BuildContextInput,
) -> AppResult<ContextPackage> {
    ensure_agent_core_enabled(&app)?;
    let mut conn = open_database(Path::new(&project_path))?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    let globals = if load_feature_flags(&app_data_dir)?.get("memory") == Some(&true) {
        Some(active_global_memories(&app_data_dir)?)
    } else {
        None
    };
    build_context_with_memories(&mut conn, input, globals.as_deref())
}

#[tauri::command]
pub fn context_search(
    app: tauri::AppHandle,
    project_path: String,
    query: String,
    limit: usize,
) -> AppResult<Vec<SearchResult>> {
    ensure_agent_core_enabled(&app)?;
    let conn = open_database(Path::new(&project_path))?;
    search_project(&conn, &query, limit)
}

fn validate_ref_project(reference: &ObjectRef, project_id: &str) -> AppResult<()> {
    if reference.project_id != project_id {
        return Err(format!(
            "对象 {}:{} 不属于当前项目",
            reference.object_type, reference.object_id
        ));
    }
    table_for_type(&reference.object_type)?;
    Ok(())
}

pub(crate) fn object_value(
    conn: &Connection,
    reference: &ObjectRef,
    compact: bool,
) -> AppResult<Value> {
    let table = table_for_type(&reference.object_type)?;
    let sql = format!("SELECT * FROM {table} WHERE id=?1 LIMIT 1");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let column_names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    let value = stmt
        .query_row([&reference.object_id], |row| {
            row_to_json(row, &column_names)
        })
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "对象不存在：{}:{}",
                reference.object_type, reference.object_id
            )
        })?;
    let Value::Object(object) = value else {
        return Err("对象数据格式无效".into());
    };
    let mut filtered = filter_object(
        object,
        &reference.object_type,
        reference.field.as_deref(),
        compact,
    )?;
    if reference.object_type == "changeSet" && !compact {
        filtered.insert(
            "changes".into(),
            change_set_changes_value(conn, &reference.object_id)?,
        );
    }
    Ok(Value::Object(filtered))
}

fn change_set_changes_value(conn: &Connection, change_set_id: &str) -> AppResult<Value> {
    let mut stmt = conn
        .prepare(
            "SELECT id, object_type, object_id, field_name, old_value, new_value, source_type, source_id, created_at
             FROM changes WHERE change_set_id=?1 ORDER BY created_at, id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([change_set_id], |row| {
            let old_raw: Option<String> = row.get(4)?;
            let new_raw: Option<String> = row.get(5)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "objectType": row.get::<_, String>(1)?,
                "objectId": row.get::<_, String>(2)?,
                "fieldName": row.get::<_, String>(3)?,
                "oldValue": old_raw.and_then(|value| serde_json::from_str(&value).ok()).unwrap_or(Value::Null),
                "newValue": new_raw.and_then(|value| serde_json::from_str(&value).ok()).unwrap_or(Value::Null),
                "sourceType": row.get::<_, String>(6)?,
                "sourceId": row.get::<_, Option<String>>(7)?,
                "createdAt": row.get::<_, String>(8)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
        .map_err(|e| e.to_string())
}

fn affected_refs(conn: &Connection, change_set: &ObjectRef) -> AppResult<Vec<ObjectRef>> {
    let mut stmt = conn
        .prepare(
            "SELECT object_type, object_id FROM changes
             WHERE change_set_id=?1 GROUP BY object_type, object_id ORDER BY MIN(created_at), object_type, object_id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([&change_set.object_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut references = Vec::new();
    for row in rows {
        let (object_type, object_id) = row.map_err(|e| e.to_string())?;
        if table_for_type(&object_type).is_ok() {
            references.push(make_ref(change_set, &object_type, object_id));
        }
    }
    Ok(references)
}

fn filter_object(
    object: Map<String, Value>,
    object_type: &str,
    field: Option<&str>,
    compact: bool,
) -> AppResult<Map<String, Value>> {
    let mut result = Map::new();
    let identity_fields = compact_fields(object_type);
    if let Some(field) = field {
        let field = camel_to_snake(field);
        if field == "path" || field.ends_with("_path") || field == "secret_ref" {
            return Err(format!("敏感本地字段不能进入上下文：{object_type}.{field}"));
        }
        if compact {
            for key in identity_fields
                .iter()
                .chain(std::iter::once(&field.as_str()))
            {
                if let Some(value) = object.get(*key) {
                    result.insert((*key).into(), sanitize_value(value.clone()));
                }
            }
        } else {
            for (key, value) in &object {
                if !key.ends_with("_path") && key != "path" && key != "secret_ref" {
                    result.insert(key.clone(), sanitize_value(value.clone()));
                }
            }
        }
        if !object.contains_key(&field) {
            return Err(format!("对象字段不存在：{object_type}.{field}"));
        }
    } else if compact {
        for key in identity_fields {
            if let Some(value) = object.get(*key) {
                result.insert((*key).into(), sanitize_value(value.clone()));
            }
        }
    } else {
        for (key, value) in object {
            if !key.ends_with("_path") && key != "path" {
                result.insert(key, sanitize_value(value));
            }
        }
    }
    Ok(result)
}

fn sanitize_value(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .filter(|(key, _)| !key.ends_with("_path") && key != "path" && key != "secret_ref")
                .map(|(key, value)| (key, sanitize_value(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize_value).collect()),
        value => value,
    }
}

fn compact_fields(object_type: &str) -> &'static [&'static str] {
    match object_type {
        "project" => &["id", "name", "description", "structure_type", "revision"],
        "contentUnit" => &["id", "parent_id", "type", "name", "summary", "sort_order"],
        "script" => &["id", "content_unit_id", "title", "summary"],
        "scene" => &[
            "id",
            "script_id",
            "title",
            "sort_order",
            "location_text",
            "time_text",
            "summary",
            "content",
        ],
        "shot" => &[
            "id",
            "scene_id",
            "title",
            "sort_order",
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
        ],
        "asset" => &[
            "id",
            "project_id",
            "type",
            "name",
            "description",
            "scope_unit_id",
        ],
        "assetRequirement" => &[
            "id",
            "content_unit_id",
            "asset_id",
            "asset_type",
            "requirement_type",
            "description",
            "status",
        ],
        "keyframe" => &[
            "id",
            "shot_id",
            "type",
            "description",
            "status",
            "sort_order",
        ],
        "generationTask" => &[
            "id",
            "content_unit_id",
            "name",
            "target_model",
            "duration",
            "status",
        ],
        "relation" => &[
            "id",
            "source_type",
            "source_id",
            "relation_type",
            "target_type",
            "target_id",
            "description",
            "importance",
            "status",
        ],
        "storyElement" => &[
            "id",
            "project_id",
            "type",
            "name",
            "description",
            "scope_unit_id",
            "maturity",
            "status",
        ],
        "storyElementOccurrence" => &[
            "id",
            "story_element_id",
            "content_unit_id",
            "occurrence_type",
            "description",
            "sort_order",
        ],
        "changeSet" => &[
            "id",
            "project_id",
            "name",
            "source_type",
            "status",
            "created_at",
        ],
        _ => &["id"],
    }
}

fn table_for_type(object_type: &str) -> AppResult<&'static str> {
    match object_type {
        "project" => Ok("projects"),
        "contentUnit" => Ok("content_units"),
        "script" => Ok("scripts"),
        "scene" => Ok("scenes"),
        "shot" => Ok("shots"),
        "asset" => Ok("assets"),
        "assetRequirement" => Ok("asset_requirements"),
        "keyframe" => Ok("keyframes"),
        "generationTask" => Ok("generation_tasks"),
        "relation" => Ok("relations"),
        "storyElement" => Ok("story_elements"),
        "storyElementOccurrence" => Ok("story_element_occurrences"),
        "changeSet" => Ok("change_sets"),
        _ => Err(format!("不支持的上下文对象类型：{object_type}")),
    }
}

pub(crate) fn parent_ref(conn: &Connection, reference: &ObjectRef) -> AppResult<Option<ObjectRef>> {
    let parent = match reference.object_type.as_str() {
        "changeSet" => parent_id(
            conn,
            "SELECT project_id FROM change_sets WHERE id=?1",
            reference,
            "project",
        )?,
        "shot" => parent_id(
            conn,
            "SELECT scene_id FROM shots WHERE id=?1",
            reference,
            "scene",
        )?,
        "scene" => parent_id(
            conn,
            "SELECT script_id FROM scenes WHERE id=?1",
            reference,
            "script",
        )?,
        "script" => parent_id(
            conn,
            "SELECT content_unit_id FROM scripts WHERE id=?1",
            reference,
            "contentUnit",
        )?,
        "keyframe" => parent_id(
            conn,
            "SELECT shot_id FROM keyframes WHERE id=?1",
            reference,
            "shot",
        )?,
        "generationTask" => parent_id(
            conn,
            "SELECT content_unit_id FROM generation_tasks WHERE id=?1",
            reference,
            "contentUnit",
        )?,
        "storyElementOccurrence" => parent_id(
            conn,
            "SELECT story_element_id FROM story_element_occurrences WHERE id=?1",
            reference,
            "storyElement",
        )?,
        "storyElement" => {
            let ids: (Option<String>, String) = conn
                .query_row(
                    "SELECT scope_unit_id, project_id FROM story_elements WHERE id=?1",
                    [&reference.object_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            Some(
                ids.0
                    .map(|id| make_ref(reference, "contentUnit", id))
                    .unwrap_or_else(|| make_ref(reference, "project", ids.1)),
            )
        }
        "assetRequirement" => {
            let ids: (Option<String>, Option<String>) = conn
                .query_row(
                    "SELECT asset_id, content_unit_id FROM asset_requirements WHERE id=?1",
                    [&reference.object_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            ids.0
                .map(|id| make_ref(reference, "asset", id))
                .or_else(|| ids.1.map(|id| make_ref(reference, "contentUnit", id)))
        }
        "asset" => {
            let ids: (Option<String>, String) = conn
                .query_row(
                    "SELECT scope_unit_id, project_id FROM assets WHERE id=?1",
                    [&reference.object_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            Some(
                ids.0
                    .map(|id| make_ref(reference, "contentUnit", id))
                    .unwrap_or_else(|| make_ref(reference, "project", ids.1)),
            )
        }
        "contentUnit" => {
            let ids: (Option<String>, String) = conn
                .query_row(
                    "SELECT parent_id, project_id FROM content_units WHERE id=?1",
                    [&reference.object_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            Some(
                ids.0
                    .map(|id| make_ref(reference, "contentUnit", id))
                    .unwrap_or_else(|| make_ref(reference, "project", ids.1)),
            )
        }
        _ => None,
    };
    Ok(parent)
}

fn parent_id(
    conn: &Connection,
    sql: &str,
    reference: &ObjectRef,
    object_type: &str,
) -> AppResult<Option<ObjectRef>> {
    conn.query_row(sql, [&reference.object_id], |row| row.get::<_, String>(0))
        .optional()
        .map(|id| id.map(|id| make_ref(reference, object_type, id)))
        .map_err(|e| e.to_string())
}

fn make_ref(parent: &ObjectRef, object_type: &str, object_id: String) -> ObjectRef {
    ObjectRef {
        project_id: parent.project_id.clone(),
        object_type: object_type.into(),
        object_id,
        field: None,
    }
}

pub(crate) fn neighbor_refs(
    conn: &Connection,
    reference: &ObjectRef,
    count: usize,
) -> AppResult<Vec<ObjectRef>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let spec = match reference.object_type.as_str() {
        "shot" => Some(("shots", "scene_id", "shot")),
        "scene" => Some(("scenes", "script_id", "scene")),
        "contentUnit" => Some(("content_units", "parent_id", "contentUnit")),
        "storyElementOccurrence" => Some((
            "story_element_occurrences",
            "story_element_id",
            "storyElementOccurrence",
        )),
        _ => None,
    };
    let Some((table, parent_column, object_type)) = spec else {
        return Ok(Vec::new());
    };
    let current_sql = format!("SELECT {parent_column}, sort_order FROM {table} WHERE id=?1");
    let (parent_id, sort_order): (Option<String>, i64) = conn
        .query_row(&current_sql, [&reference.object_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| e.to_string())?;
    let before_sql = format!(
        "SELECT id FROM {table} WHERE {parent_column} IS ?1 AND sort_order < ?2 ORDER BY sort_order DESC LIMIT ?3"
    );
    let after_sql = format!(
        "SELECT id FROM {table} WHERE {parent_column} IS ?1 AND sort_order > ?2 ORDER BY sort_order ASC LIMIT ?3"
    );
    let mut ids = query_ids(conn, &before_sql, parent_id.as_deref(), sort_order, count)?;
    ids.reverse();
    ids.extend(query_ids(
        conn,
        &after_sql,
        parent_id.as_deref(),
        sort_order,
        count,
    )?);
    Ok(ids
        .into_iter()
        .map(|id| make_ref(reference, object_type, id))
        .collect())
}

fn query_ids(
    conn: &Connection,
    sql: &str,
    parent_id: Option<&str>,
    sort_order: i64,
    count: usize,
) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![parent_id, sort_order, count as i64], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn relation_items(
    conn: &Connection,
    reference: &ObjectRef,
    limit: usize,
) -> AppResult<Vec<(ObjectRef, Value)>> {
    let mut stmt = conn
        .prepare(
            "SELECT * FROM relations
             WHERE (source_type=?1 AND source_id=?2) OR (target_type=?1 AND target_id=?2)
             ORDER BY importance DESC, created_at ASC LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let column_names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    let rows = stmt
        .query_map(
            params![reference.object_type, reference.object_id, limit as i64],
            |row| row_to_json(row, &column_names),
        )
        .map_err(|e| e.to_string())?;
    let mut items: Vec<(ObjectRef, Value)> = rows
        .map(|row| {
            let value = row.map_err(|e| e.to_string())?;
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "Relation 缺少 id".to_string())?;
            Ok((
                ObjectRef {
                    project_id: reference.project_id.clone(),
                    object_type: "relation".into(),
                    object_id: id.into(),
                    field: None,
                },
                sanitize_value(value),
            ))
        })
        .collect::<AppResult<_>>()?;
    drop(stmt);

    if reference.object_type == "shot" && items.len() < limit {
        let remaining = limit - items.len();
        let mut asset_stmt = conn
            .prepare(
                "SELECT asset_id FROM shot_assets WHERE shot_id=?1 ORDER BY created_at ASC LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        let asset_ids = asset_stmt
            .query_map(params![reference.object_id, remaining as i64], |row| {
                row.get(0)
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| e.to_string())?;
        for asset_id in asset_ids {
            let asset_ref = make_ref(reference, "asset", asset_id);
            items.push((asset_ref.clone(), object_value(conn, &asset_ref, true)?));
        }
    }
    Ok(items)
}

fn intent_items(
    conn: &Connection,
    center: &ObjectRef,
    task_intent: &str,
) -> AppResult<Vec<(ObjectRef, &'static str, Value)>> {
    let mut items = Vec::new();
    if task_intent == "project_planning" {
        let (anchor_sql, anchor_id) = if center.object_type == "contentUnit" {
            (
                "SELECT id FROM content_units WHERE id=?1",
                center.object_id.as_str(),
            )
        } else {
            (
                "SELECT id FROM content_units WHERE project_id=?1 AND parent_id IS NULL",
                center.project_id.as_str(),
            )
        };
        let sql = format!(
            "WITH RECURSIVE tree(id, path) AS (
               SELECT unit.id, printf('%08d', unit.sort_order) FROM content_units unit
               WHERE unit.id IN ({anchor_sql})
               UNION ALL
               SELECT child.id, tree.path || '.' || printf('%08d', child.sort_order)
               FROM content_units child JOIN tree ON child.parent_id=tree.id
             ) SELECT id FROM tree ORDER BY path LIMIT 120"
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map([anchor_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        for id in ids {
            if center.object_type == "contentUnit" && id == center.object_id {
                continue;
            }
            let reference = make_ref(center, "contentUnit", id);
            items.push((
                reference.clone(),
                "descendant",
                object_value(conn, &reference, true)?,
            ));
        }
        let mut story_stmt = conn
            .prepare(
                "SELECT id FROM story_elements WHERE project_id=?1 AND status='active'
                 ORDER BY type, name, id LIMIT 100",
            )
            .map_err(|e| e.to_string())?;
        let story_ids = story_stmt
            .query_map([&center.project_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        for id in story_ids {
            let reference = make_ref(center, "storyElement", id.clone());
            items.push((
                reference.clone(),
                "story_element",
                object_value(conn, &reference, true)?,
            ));
            let mut occurrence_stmt = conn
                .prepare(
                    "SELECT id FROM story_element_occurrences
                     WHERE story_element_id=?1 ORDER BY sort_order, id LIMIT 200",
                )
                .map_err(|e| e.to_string())?;
            let occurrence_ids = occurrence_stmt
                .query_map([id], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            for occurrence_id in occurrence_ids {
                let occurrence = make_ref(center, "storyElementOccurrence", occurrence_id);
                items.push((
                    occurrence.clone(),
                    "occurrence",
                    object_value(conn, &occurrence, true)?,
                ));
            }
        }
    }

    if task_intent == "story_element_analysis" || center.object_type == "storyElement" {
        let story_element_id = if center.object_type == "storyElementOccurrence" {
            conn.query_row(
                "SELECT story_element_id FROM story_element_occurrences WHERE id=?1",
                [&center.object_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?
        } else {
            center.object_id.clone()
        };
        let mut stmt = conn
            .prepare(
                "SELECT id, content_unit_id FROM story_element_occurrences
                 WHERE story_element_id=?1 ORDER BY sort_order, id LIMIT 200",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([story_element_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        for (occurrence_id, unit_id) in rows {
            let occurrence = make_ref(center, "storyElementOccurrence", occurrence_id);
            items.push((
                occurrence.clone(),
                "occurrence",
                object_value(conn, &occurrence, true)?,
            ));
            let unit = make_ref(center, "contentUnit", unit_id);
            items.push((
                unit.clone(),
                "occurrence_unit",
                object_value(conn, &unit, true)?,
            ));
        }
    }
    Ok(items)
}

fn store_context_package(conn: &Connection, package: &ContextPackage) -> AppResult<()> {
    conn.execute(
        "INSERT INTO context_packages (
           id, task_id, project_revision, center_ref_json, items_json, memory_ids_json,
           token_estimate, checksum, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(task_id) DO UPDATE SET
           id=excluded.id, project_revision=excluded.project_revision,
           center_ref_json=excluded.center_ref_json, items_json=excluded.items_json,
           memory_ids_json=excluded.memory_ids_json, token_estimate=excluded.token_estimate,
           checksum=excluded.checksum, created_at=excluded.created_at",
        params![
            package.id,
            package.task_id,
            package.project_revision,
            serde_json::to_string(&package.center_ref).map_err(|e| e.to_string())?,
            serde_json::to_string(&package.included_items).map_err(|e| e.to_string())?,
            serde_json::to_string(&package.included_memory_ids).map_err(|e| e.to_string())?,
            package.token_estimate as i64,
            package.checksum,
            package.created_at,
        ],
    )
    .map_err(|e| format!("保存 ContextPackage 失败：{e}"))?;
    Ok(())
}

fn estimate_tokens(text: &str) -> usize {
    let mut ascii = 0usize;
    let mut non_ascii = 0usize;
    for character in text.chars() {
        if character.is_ascii() {
            ascii += 1;
        } else {
            non_ascii += 1;
        }
    }
    non_ascii + ascii.div_ceil(4)
}

fn fit_value(value: Value, budget: usize) -> Option<(Value, usize)> {
    if budget < 16 {
        return None;
    }
    let serialized = value.to_string();
    let mut content = String::new();
    for character in serialized.chars() {
        content.push(character);
        let candidate = json!({ "truncated": true, "content": content });
        if estimate_tokens(&candidate.to_string()) > budget {
            content.pop();
            break;
        }
    }
    let fitted = json!({ "truncated": true, "content": content });
    let estimate = estimate_tokens(&fitted.to_string());
    (estimate <= budget).then_some((fitted, estimate))
}

fn stable_checksum(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn camel_to_snake(value: &str) -> String {
    let mut output = String::new();
    for character in value.chars() {
        if character.is_ascii_uppercase() {
            output.push('_');
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{init_database, open_database};

    fn insert_fixture(conn: &Connection, project_id: &str) {
        let timestamp = now();
        conn.execute(
            "INSERT INTO content_units (id, project_id, type, name, summary, sort_order, created_at, updated_at) VALUES ('unit', ?1, 'episode', 'EP01', '测试集', 0, ?2, ?2)",
            params![project_id, timestamp],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scripts (id, content_unit_id, title, summary, created_at, updated_at) VALUES ('script', 'unit', 'EP01 剧本', '局部剧本', ?1, ?1)",
            [timestamp.clone()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scenes (id, script_id, title, sort_order, summary, content, created_at, updated_at) VALUES ('scene', 'script', '大厅', 0, '当前场', '这里只包含当前场事实', ?1, ?1)",
            [timestamp.clone()],
        )
        .unwrap();
        for (id, order, title, composition, dialogue) in [
            ("shot-before", 0, "镜头03", "全景", "前一个镜头"),
            ("shot-center", 1, "镜头04", "中景低机位", "中心镜头"),
            ("shot-after", 2, "镜头05", "近景", "后一个镜头"),
        ] {
            conn.execute(
                "INSERT INTO shots (id, scene_id, sort_order, title, composition, dialogue, created_at, updated_at) VALUES (?1, 'scene', ?2, ?3, ?4, ?5, ?6, ?6)",
                params![id, order, title, composition, dialogue, timestamp],
            )
            .unwrap();
        }
        for order in 3..33 {
            conn.execute(
                "INSERT INTO shots (id, scene_id, sort_order, title, dialogue, created_at, updated_at) VALUES (?1, 'scene', ?2, ?3, ?4, ?5, ?5)",
                params![
                    format!("far-{order}"),
                    order,
                    format!("远处镜头{order}"),
                    if order == 20 { "暗号蓝门 NEVER_LOAD" } else { "无关内容" },
                    timestamp,
                ],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO assets (id, project_id, type, name, description, created_at, updated_at) VALUES ('asset', ?1, 'character', '奶牛猫', '当前镜头正式资产', ?2, ?2)",
            params![project_id, timestamp],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO shot_assets (id, shot_id, asset_id, role, created_at, updated_at) VALUES ('shot-asset', 'shot-center', 'asset', 'subject', ?1, ?1)",
            [timestamp.clone()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO keyframes (id, shot_id, type, file_path, description, created_at, updated_at) VALUES ('keyframe', 'shot-center', 'single', 'C:\\private\\frame.png', '测试关键帧', ?1, ?1)",
            [timestamp.clone()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO relations (id, project_id, source_type, source_id, relation_type, target_type, target_id, description, created_at, updated_at) VALUES ('relation', ?1, 'shot', 'shot-center', 'features', 'asset', 'asset', '镜头正式关系', ?2, ?2)",
            params![project_id, timestamp],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_sessions (id, project_id, title, created_at, updated_at) VALUES ('session', ?1, 'Context Test', ?2, ?2)",
            params![project_id, timestamp],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_tasks (id, session_id, task_type, agent_type, created_at) VALUES ('task', 'session', 'shot_composition', 'cinematography', ?1)",
            [timestamp],
        )
        .unwrap();
    }

    fn build_input(project_id: &str, revision: i64) -> BuildContextInput {
        BuildContextInput {
            task_id: "task".into(),
            selection: SelectionSnapshot {
                project_id: project_id.into(),
                center: Some(ObjectRef {
                    project_id: project_id.into(),
                    object_type: "shot".into(),
                    object_id: "shot-center".into(),
                    field: Some("composition".into()),
                }),
                selected: Vec::new(),
                project_revision: revision,
            },
            task_intent: "shot_composition".into(),
            expert_type: "cinematography".into(),
            token_budget: 600,
        }
    }

    #[test]
    fn field_context_is_bounded_local_stable_and_persisted() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "Context Project", "series").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        insert_fixture(&conn, &project.id);

        let first =
            build_context_with_memories(&mut conn, build_input(&project.id, 0), None).unwrap();
        let second =
            build_context_with_memories(&mut conn, build_input(&project.id, 0), None).unwrap();
        assert_eq!(first.checksum, second.checksum);
        assert!(first.token_estimate <= 600);
        assert!(first
            .included_items
            .iter()
            .any(|item| item.reference.object_id == "shot-before"));
        assert!(first
            .included_items
            .iter()
            .any(|item| item.reference.object_id == "shot-after"));
        assert!(first
            .included_items
            .iter()
            .any(|item| item.reference.object_id == "asset"));
        let serialized = serde_json::to_string(&first).unwrap();
        assert!(!serialized.contains("NEVER_LOAD"));
        assert!(!serialized.contains("far-20"));
        let center = first
            .included_items
            .iter()
            .find(|item| item.source == "center")
            .unwrap();
        assert_eq!(center.data["composition"], "中景低机位");
        assert_eq!(center.data["dialogue"], "中心镜头");
        assert!(first.included_items.iter().any(|item| {
            item.reference.object_id == "shot-before" && item.data["dialogue"] == "前一个镜头"
        }));
        assert!(first.included_items.iter().any(|item| {
            item.reference.object_id == "scene" && item.data["content"] == "这里只包含当前场事实"
        }));

        let stored: (i64, String) = conn
            .query_row(
                "SELECT COUNT(*), checksum FROM context_packages WHERE task_id='task'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, (1, second.checksum));
    }

    #[test]
    fn facts_precede_active_memories_and_inactive_memories_are_excluded() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "Memory Context", "series").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        insert_fixture(&conn, &project.id);
        let timestamp = now();
        for (id, status, content) in [
            ("active-memory", "active", "镜头切换必须出现新信息"),
            ("candidate-memory", "candidate", "尚未确认的倾向"),
            ("invalid-memory", "invalidated", "已经废弃的错误偏好"),
        ] {
            conn.execute(
                "INSERT INTO project_memories (id, scope_type, scope_id, category, content, status, source_type, created_at, updated_at) VALUES (?1, 'contentUnit', 'unit', 'editing', ?2, ?3, 'user', ?4, ?4)",
                params![id, content, status, timestamp],
            )
            .unwrap();
        }
        let globals = vec![MemoryContextEntry {
            id: "global-memory".into(),
            storage: "global".into(),
            scope_type: "global".into(),
            scope_id: None,
            category: "language".into(),
            content: "默认使用中文".into(),
            source_type: "user".into(),
            priority: 0,
            updated_at: timestamp,
        }];
        let mut input = build_input(&project.id, 0);
        input.token_budget = 2_000;
        let package = build_context_with_memories(&mut conn, input, Some(&globals)).unwrap();
        assert_eq!(package.included_items[0].source, "center");
        assert_eq!(
            package.included_memory_ids,
            ["active-memory", "global-memory"]
        );
        let serialized = serde_json::to_string(&package).unwrap();
        assert!(serialized.contains("镜头切换必须出现新信息"));
        assert!(serialized.contains("默认使用中文"));
        assert!(!serialized.contains("尚未确认的倾向"));
        assert!(!serialized.contains("已经废弃的错误偏好"));
    }

    #[test]
    fn rejects_stale_selection_and_searches_project_with_fts() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "Search Project", "series").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        insert_fixture(&conn, &project.id);

        let error =
            build_context_with_memories(&mut conn, build_input(&project.id, 1), None).unwrap_err();
        assert!(error.contains("revision 已过期"));
        let mut sensitive = build_input(&project.id, 0);
        sensitive.selection.center = Some(ObjectRef {
            project_id: project.id.clone(),
            object_type: "keyframe".into(),
            object_id: "keyframe".into(),
            field: Some("filePath".into()),
        });
        let error = build_context_with_memories(&mut conn, sensitive, None).unwrap_err();
        assert!(error.contains("敏感本地字段"));
        let results = search_project(&conn, "暗号蓝门", 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].reference.object_id, "far-20");
        assert!(results[0].snippet.contains("暗号蓝门"));
    }

    #[test]
    fn project_planning_loads_ordered_descendants_and_story_occurrences() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "Planning Context", "series").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        insert_fixture(&conn, &project.id);
        let timestamp = now();
        conn.execute(
            "INSERT INTO content_units (id, project_id, type, name, summary, sort_order, created_at, updated_at) VALUES ('season', ?1, 'season', '第一季', '季摘要', 1, ?2, ?2)",
            params![project.id, timestamp],
        )
        .unwrap();
        for index in 0..30 {
            conn.execute(
                "INSERT INTO content_units (id, project_id, parent_id, type, name, summary, sort_order, created_at, updated_at) VALUES (?1, ?2, 'season', 'episode', ?3, ?4, ?5, ?6, ?6)",
                params![format!("episode-{index}"), project.id, format!("EP{:02}", index + 1), format!("第{}集摘要", index + 1), index, timestamp],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO story_elements (id, project_id, type, name, description, status, created_at, updated_at) VALUES ('clue', ?1, 'foreshadowing', '蓝门伏笔', '贯穿整季', 'active', ?2, ?2)",
            params![project.id, timestamp],
        )
        .unwrap();
        for (id, unit, kind, order) in [
            ("clue-seed", "episode-0", "seed", 0),
            ("clue-payoff", "episode-29", "payoff", 1),
        ] {
            conn.execute(
                "INSERT INTO story_element_occurrences (id, story_element_id, content_unit_id, occurrence_type, description, sort_order, created_at, updated_at) VALUES (?1, 'clue', ?2, ?3, ?3, ?4, ?5, ?5)",
                params![id, unit, kind, order, timestamp],
            )
            .unwrap();
        }

        let package = build_context_with_memories(
            &mut conn,
            BuildContextInput {
                task_id: "task".into(),
                selection: SelectionSnapshot {
                    project_id: project.id.clone(),
                    center: Some(ObjectRef {
                        project_id: project.id.clone(),
                        object_type: "project".into(),
                        object_id: project.id,
                        field: None,
                    }),
                    selected: Vec::new(),
                    project_revision: 0,
                },
                task_intent: "project_planning".into(),
                expert_type: "writer".into(),
                token_budget: 100_000,
            },
            None,
        )
        .unwrap();
        let descendants = package
            .included_items
            .iter()
            .filter(|item| item.source == "descendant")
            .collect::<Vec<_>>();
        assert_eq!(
            descendants
                .iter()
                .filter(|item| item.data["parent_id"] == "season")
                .count(),
            30
        );
        assert_eq!(descendants[1].data["name"], "第一季");
        assert!(package.included_items.iter().any(|item| {
            item.reference.object_id == "clue-seed" && item.source == "occurrence"
        }));
        assert!(package.included_items.iter().any(|item| {
            item.reference.object_id == "clue-payoff" && item.source == "occurrence"
        }));
    }

    #[test]
    fn change_set_context_contains_diffs_and_local_affected_objects() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "Change Analysis", "series").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        insert_fixture(&conn, &project.id);
        let timestamp = now();
        conn.execute(
            "INSERT INTO change_sets (id, project_id, name, source_type, status, created_at, closed_at) VALUES ('changeset', ?1, '本轮修改', 'user', 'closed', ?2, ?2)",
            params![project.id, timestamp],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO changes (id, change_set_id, object_type, object_id, field_name, old_value, new_value, source_type, created_at) VALUES ('change-duration', 'changeset', 'shot', 'shot-center', 'duration', '3.0', '1.0', 'user', ?1)",
            [&timestamp],
        )
        .unwrap();
        conn.execute(
            "UPDATE agent_tasks SET task_type='change_analysis', agent_type='main' WHERE id='task'",
            [],
        )
        .unwrap();

        let package = build_context_with_memories(
            &mut conn,
            BuildContextInput {
                task_id: "task".into(),
                selection: SelectionSnapshot {
                    project_id: project.id.clone(),
                    center: Some(ObjectRef {
                        project_id: project.id,
                        object_type: "changeSet".into(),
                        object_id: "changeset".into(),
                        field: None,
                    }),
                    selected: Vec::new(),
                    project_revision: 0,
                },
                task_intent: "change_analysis".into(),
                expert_type: "main".into(),
                token_budget: 1_200,
            },
            None,
        )
        .unwrap();

        let center = package
            .included_items
            .iter()
            .find(|item| item.source == "center")
            .unwrap();
        assert_eq!(center.data["changes"][0]["oldValue"], 3.0);
        assert_eq!(center.data["changes"][0]["newValue"], 1.0);
        assert!(package.included_items.iter().any(|item| {
            item.source == "affected" && item.reference.object_id == "shot-center"
        }));
        assert!(package
            .included_items
            .iter()
            .any(|item| item.reference.object_id == "shot-before"));
        assert!(package.token_estimate <= 1_200);
    }
}
