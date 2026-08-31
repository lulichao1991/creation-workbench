use crate::app_database::{load_feature_flags, open_app_database};
use crate::context::ObjectRef;
use crate::database::{now, open_database, AppResult};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::path::Path;
use tauri::Manager;

const MEMORY_STATUSES: &[&str] = &["candidate", "active", "superseded", "invalidated"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySource {
    id: String,
    source_type: String,
    source_id: Option<String>,
    excerpt: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecord {
    id: String,
    storage: String,
    scope_type: String,
    scope_id: Option<String>,
    category: String,
    content: String,
    status: String,
    confidence: f64,
    priority: i64,
    source_type: String,
    source_id: Option<String>,
    supersedes_id: Option<String>,
    created_at: String,
    updated_at: String,
    sources: Vec<MemorySource>,
    used_by_task_ids: Vec<String>,
    conflict_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryContextEntry {
    pub id: String,
    pub storage: String,
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub category: String,
    pub content: String,
    pub source_type: String,
    pub priority: i64,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMemoryInput {
    request_id: String,
    storage: String,
    scope_type: String,
    scope_id: Option<String>,
    category: String,
    content: String,
    status: String,
    confidence: Option<f64>,
    priority: Option<i64>,
    source_type: Option<String>,
    source_id: Option<String>,
    excerpt: Option<String>,
    supersedes_id: Option<String>,
    confirmed: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemoryInput {
    storage: String,
    memory_id: String,
    content: Option<String>,
    category: Option<String>,
    scope_type: Option<String>,
    scope_id: Option<String>,
    status: Option<String>,
    confidence: Option<f64>,
    priority: Option<i64>,
    supersedes_id: Option<String>,
    confirmed: Option<bool>,
}

#[derive(Clone, Copy)]
struct StoreSpec {
    storage: &'static str,
    table: &'static str,
    source_table: &'static str,
}

fn store_spec(storage: &str) -> AppResult<StoreSpec> {
    match storage {
        "project" => Ok(StoreSpec {
            storage: "project",
            table: "project_memories",
            source_table: "memory_sources",
        }),
        "global" => Ok(StoreSpec {
            storage: "global",
            table: "long_term_memories",
            source_table: "long_term_memory_sources",
        }),
        _ => Err("TOOL_ARGUMENT_INVALID: memory storage 必须是 project 或 global".into()),
    }
}

fn validate_status(status: &str) -> AppResult<()> {
    if MEMORY_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(format!("TOOL_ARGUMENT_INVALID: 未知记忆状态 {status}"))
    }
}

fn validate_content(content: &str) -> AppResult<()> {
    let length = content.trim().chars().count();
    if (1..=4000).contains(&length) {
        Ok(())
    } else {
        Err("TOOL_ARGUMENT_INVALID: 记忆内容必须为 1–4000 个字符".into())
    }
}

fn validate_scope(
    conn: &Connection,
    storage: &str,
    scope_type: &str,
    scope_id: Option<&str>,
) -> AppResult<()> {
    if storage == "global" {
        return if scope_type == "global" && scope_id.is_none() {
            Ok(())
        } else {
            Err("TOOL_ARGUMENT_INVALID: 长期记忆只能使用 global 作用域".into())
        };
    }
    match scope_type {
        "project" => {
            let project_id: String = conn
                .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            if scope_id == Some(project_id.as_str()) {
                Ok(())
            } else {
                Err("TOOL_ARGUMENT_INVALID: 项目记忆的 project scopeId 不匹配".into())
            }
        }
        "contentUnit" => {
            let Some(scope_id) = scope_id else {
                return Err("TOOL_ARGUMENT_INVALID: contentUnit 作用域缺少 scopeId".into());
            };
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM content_units WHERE id=?1)",
                    [scope_id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if exists {
                Ok(())
            } else {
                Err("OBJECT_NOT_FOUND: 记忆作用域内容单元不存在".into())
            }
        }
        _ => Err("TOOL_ARGUMENT_INVALID: 项目记忆作用域必须是 project 或 contentUnit".into()),
    }
}

fn ensure_long_term_confirmation(storage: &str, status: &str, confirmed: bool) -> AppResult<()> {
    if storage == "global" && status == "active" && !confirmed {
        Err("CONFIRMATION_REQUIRED: 长期记忆必须由用户明确确认".into())
    } else {
        Ok(())
    }
}

fn conflicting_ids(
    conn: &Connection,
    spec: StoreSpec,
    memory_id: &str,
    scope_type: &str,
    scope_id: Option<&str>,
    category: &str,
) -> AppResult<Vec<String>> {
    let sql = format!(
        "SELECT id FROM {} WHERE id<>?1 AND status='active' AND scope_type=?2 AND scope_id IS ?3 AND category=?4 ORDER BY updated_at DESC, id",
        spec.table
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![memory_id, scope_type, scope_id, category], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn activate_with_replacement(
    tx: &Transaction<'_>,
    spec: StoreSpec,
    memory_id: &str,
    scope_type: &str,
    scope_id: Option<&str>,
    category: &str,
    supersedes_id: Option<&str>,
) -> AppResult<()> {
    let conflicts = conflicting_ids(tx, spec, memory_id, scope_type, scope_id, category)?;
    if conflicts.is_empty() {
        return Ok(());
    }
    let Some(supersedes_id) = supersedes_id else {
        return Err(format!(
            "MEMORY_CONFLICT: 需明确替代记忆 {}",
            conflicts.join(",")
        ));
    };
    if !conflicts.iter().any(|id| id == supersedes_id) {
        return Err("MEMORY_CONFLICT: supersedesId 不是同范围同分类的生效记忆".into());
    }
    let sql = format!(
        "UPDATE {} SET status='superseded', updated_at=?1 WHERE id=?2",
        spec.table
    );
    tx.execute(&sql, params![now(), supersedes_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn create_in_conn(conn: &mut Connection, input: &CreateMemoryInput) -> AppResult<MemoryRecord> {
    let spec = store_spec(&input.storage)?;
    if let Ok(existing) = load_memory(conn, spec, &input.request_id, &[]) {
        return Ok(existing);
    }
    validate_status(&input.status)?;
    validate_content(&input.content)?;
    validate_scope(
        conn,
        &input.storage,
        &input.scope_type,
        input.scope_id.as_deref(),
    )?;
    ensure_long_term_confirmation(
        &input.storage,
        &input.status,
        input.confirmed.unwrap_or(false),
    )?;
    if input.category.trim().is_empty() {
        return Err("TOOL_ARGUMENT_INVALID: 记忆分类不能为空".into());
    }
    let confidence = input.confidence.unwrap_or(1.0).clamp(0.0, 1.0);
    let priority = input.priority.unwrap_or(0).clamp(-100, 100);
    let source_type = input.source_type.as_deref().unwrap_or("user");
    let timestamp = now();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    if input.status == "active" {
        activate_with_replacement(
            &tx,
            spec,
            &input.request_id,
            &input.scope_type,
            input.scope_id.as_deref(),
            input.category.trim(),
            input.supersedes_id.as_deref(),
        )?;
    }
    let sql = format!(
        "INSERT INTO {} (id, scope_type, scope_id, category, content, status, confidence, priority, source_type, source_id, supersedes_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        spec.table
    );
    tx.execute(
        &sql,
        params![
            input.request_id,
            input.scope_type,
            input.scope_id,
            input.category.trim(),
            input.content.trim(),
            input.status,
            confidence,
            priority,
            source_type,
            input.source_id,
            input.supersedes_id,
            timestamp
        ],
    )
    .map_err(|e| format!("创建记忆失败：{e}"))?;
    let source_sql = format!(
        "INSERT INTO {} (id, memory_id, source_type, source_id, excerpt, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        spec.source_table
    );
    tx.execute(
        &source_sql,
        params![
            format!("{}:source", input.request_id),
            input.request_id,
            source_type,
            input.source_id,
            input.excerpt.as_deref().unwrap_or(""),
            timestamp
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    load_memory(conn, spec, &input.request_id, &[])
}

fn update_in_conn(conn: &mut Connection, input: &UpdateMemoryInput) -> AppResult<MemoryRecord> {
    let spec = store_spec(&input.storage)?;
    let existing = load_memory(conn, spec, &input.memory_id, &[])?;
    let content = input
        .content
        .as_deref()
        .unwrap_or(&existing.content)
        .trim()
        .to_string();
    let category = input
        .category
        .as_deref()
        .unwrap_or(&existing.category)
        .trim()
        .to_string();
    let scope_type = input
        .scope_type
        .as_deref()
        .unwrap_or(&existing.scope_type)
        .to_string();
    let scope_id = input.scope_id.clone().or(existing.scope_id.clone());
    let status = input
        .status
        .as_deref()
        .unwrap_or(&existing.status)
        .to_string();
    validate_content(&content)?;
    validate_status(&status)?;
    validate_scope(conn, &input.storage, &scope_type, scope_id.as_deref())?;
    ensure_long_term_confirmation(&input.storage, &status, input.confirmed.unwrap_or(false))?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let activation_target_changed = existing.status != "active"
        || existing.scope_type != scope_type
        || existing.scope_id != scope_id
        || existing.category != category;
    if status == "active" && activation_target_changed {
        activate_with_replacement(
            &tx,
            spec,
            &input.memory_id,
            &scope_type,
            scope_id.as_deref(),
            &category,
            input.supersedes_id.as_deref(),
        )?;
    }
    let sql = format!(
        "UPDATE {} SET scope_type=?1, scope_id=?2, category=?3, content=?4, status=?5, confidence=?6, priority=?7, supersedes_id=COALESCE(?8, supersedes_id), updated_at=?9 WHERE id=?10",
        spec.table
    );
    tx.execute(
        &sql,
        params![
            scope_type,
            scope_id,
            category,
            content,
            status,
            input
                .confidence
                .unwrap_or(existing.confidence)
                .clamp(0.0, 1.0),
            input.priority.unwrap_or(existing.priority).clamp(-100, 100),
            input.supersedes_id,
            now(),
            input.memory_id
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    load_memory(conn, spec, &input.memory_id, &[])
}

fn source_rows(
    conn: &Connection,
    spec: StoreSpec,
    memory_id: &str,
) -> AppResult<Vec<MemorySource>> {
    let sql = format!(
        "SELECT id, source_type, source_id, excerpt, created_at FROM {} WHERE memory_id=?1 ORDER BY created_at, id",
        spec.source_table
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([memory_id], |row| {
            Ok(MemorySource {
                id: row.get(0)?,
                source_type: row.get(1)?,
                source_id: row.get(2)?,
                excerpt: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn load_memory(
    conn: &Connection,
    spec: StoreSpec,
    memory_id: &str,
    used_by_task_ids: &[String],
) -> AppResult<MemoryRecord> {
    let sql = format!(
        "SELECT id, scope_type, scope_id, category, content, status, confidence, priority, source_type, source_id, supersedes_id, created_at, updated_at FROM {} WHERE id=?1",
        spec.table
    );
    let row = conn
        .query_row(&sql, [memory_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
            ))
        })
        .map_err(|_| "OBJECT_NOT_FOUND: 记忆不存在".to_string())?;
    let conflict_ids = if row.5 == "candidate" {
        conflicting_ids(conn, spec, &row.0, &row.1, row.2.as_deref(), &row.3)?
    } else {
        Vec::new()
    };
    Ok(MemoryRecord {
        id: row.0.clone(),
        storage: spec.storage.into(),
        scope_type: row.1,
        scope_id: row.2,
        category: row.3,
        content: row.4,
        status: row.5,
        confidence: row.6,
        priority: row.7,
        source_type: row.8,
        source_id: row.9,
        supersedes_id: row.10,
        created_at: row.11,
        updated_at: row.12,
        sources: source_rows(conn, spec, &row.0)?,
        used_by_task_ids: used_by_task_ids.to_vec(),
        conflict_ids,
    })
}

fn used_memory_tasks(conn: &Connection) -> AppResult<Vec<(String, String)>> {
    let mut stmt = conn
        .prepare("SELECT task_id, memory_ids_json FROM context_packages ORDER BY created_at DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        let (task_id, raw) = row.map_err(|e| e.to_string())?;
        for memory_id in serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default() {
            result.push((memory_id, task_id.clone()));
        }
    }
    Ok(result)
}

fn list_ids(conn: &Connection, spec: StoreSpec, query: Option<&str>) -> AppResult<Vec<String>> {
    if let Some(query) = query.filter(|value| !value.trim().is_empty()) {
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS temp.memory_search_fts USING fts5(id UNINDEXED, content, category, tokenize='unicode61'); DELETE FROM temp.memory_search_fts;",
        )
        .map_err(|e| format!("建立记忆 FTS 失败：{e}"))?;
        conn.execute(
            &format!(
                "INSERT INTO temp.memory_search_fts SELECT id, content, category FROM {}",
                spec.table
            ),
            [],
        )
        .map_err(|e| e.to_string())?;
        let phrase = format!("\"{}\"", query.trim().replace('"', "\"\""));
        let mut stmt = conn
            .prepare("SELECT id FROM temp.memory_search_fts WHERE memory_search_fts MATCH ?1 ORDER BY bm25(memory_search_fts) LIMIT 100")
            .map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map([phrase], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if !ids.is_empty() {
            return Ok(ids);
        }
        let sql = format!(
            "SELECT id FROM {} WHERE content LIKE '%' || ?1 || '%' OR category LIKE '%' || ?1 || '%' ORDER BY updated_at DESC, id LIMIT 100",
            spec.table
        );
        let mut fallback = conn.prepare(&sql).map_err(|e| e.to_string())?;
        return fallback
            .query_map([query.trim()], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string());
    }
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id FROM {} ORDER BY updated_at DESC, id",
            spec.table
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn list_memories(
    project_conn: &Connection,
    global_conn: &Connection,
    query: Option<&str>,
) -> AppResult<Vec<MemoryRecord>> {
    let usage = used_memory_tasks(project_conn)?;
    let mut records = Vec::new();
    for (conn, spec) in [
        (project_conn, store_spec("project")?),
        (global_conn, store_spec("global")?),
    ] {
        for id in list_ids(conn, spec, query)? {
            let tasks = usage
                .iter()
                .filter(|(memory_id, _)| memory_id == &id)
                .map(|(_, task_id)| task_id.clone())
                .collect::<Vec<_>>();
            records.push(load_memory(conn, spec, &id, &tasks)?);
        }
    }
    records.sort_by_key(|memory| {
        (
            memory.status != "active",
            Reverse(memory.updated_at.clone()),
        )
    });
    Ok(records)
}

pub fn active_global_memories(app_data_dir: &Path) -> AppResult<Vec<MemoryContextEntry>> {
    let conn = open_app_database(app_data_dir)?;
    context_entries(&conn, store_spec("global")?)
}

pub fn active_project_memories(
    conn: &Connection,
    center: &ObjectRef,
) -> AppResult<Vec<MemoryContextEntry>> {
    let project_id: String = conn
        .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let mut scope_ids = Vec::new();
    if let Some(mut unit_id) = center_content_unit(conn, center)? {
        loop {
            scope_ids.push(unit_id.clone());
            let parent = conn
                .query_row(
                    "SELECT parent_id FROM content_units WHERE id=?1",
                    [&unit_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .flatten();
            let Some(next) = parent else { break };
            unit_id = next;
        }
    }
    scope_ids.push(project_id);
    let mut entries = context_entries(conn, store_spec("project")?)?
        .into_iter()
        .filter(|entry| {
            entry
                .scope_id
                .as_ref()
                .is_some_and(|id| scope_ids.contains(id))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| {
        let distance = entry
            .scope_id
            .as_ref()
            .and_then(|id| scope_ids.iter().position(|candidate| candidate == id))
            .unwrap_or(usize::MAX);
        (
            distance,
            Reverse(entry.priority),
            Reverse(entry.updated_at.clone()),
        )
    });
    entries.truncate(8);
    Ok(entries)
}

fn context_entries(conn: &Connection, spec: StoreSpec) -> AppResult<Vec<MemoryContextEntry>> {
    let sql = format!(
        "SELECT id, scope_type, scope_id, category, content, source_type, priority, updated_at FROM {} WHERE status='active' ORDER BY priority DESC, updated_at DESC LIMIT 500",
        spec.table
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(MemoryContextEntry {
                id: row.get(0)?,
                storage: spec.storage.into(),
                scope_type: row.get(1)?,
                scope_id: row.get(2)?,
                category: row.get(3)?,
                content: row.get(4)?,
                source_type: row.get(5)?,
                priority: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn center_content_unit(conn: &Connection, center: &ObjectRef) -> AppResult<Option<String>> {
    let result = match center.object_type.as_str() {
        "contentUnit" => Some(center.object_id.clone()),
        "script" => conn.query_row("SELECT content_unit_id FROM scripts WHERE id=?1", [&center.object_id], |row| row.get(0)).optional().map_err(|e| e.to_string())?,
        "scene" => conn.query_row("SELECT script.content_unit_id FROM scenes scene JOIN scripts script ON script.id=scene.script_id WHERE scene.id=?1", [&center.object_id], |row| row.get(0)).optional().map_err(|e| e.to_string())?,
        "shot" => conn.query_row("SELECT script.content_unit_id FROM shots shot JOIN scenes scene ON scene.id=shot.scene_id JOIN scripts script ON script.id=scene.script_id WHERE shot.id=?1", [&center.object_id], |row| row.get(0)).optional().map_err(|e| e.to_string())?,
        "storyElementOccurrence" => conn.query_row("SELECT content_unit_id FROM story_element_occurrences WHERE id=?1", [&center.object_id], |row| row.get(0)).optional().map_err(|e| e.to_string())?,
        "storyElement" => conn.query_row("SELECT scope_unit_id FROM story_elements WHERE id=?1", [&center.object_id], |row| row.get::<_, Option<String>>(0)).optional().map_err(|e| e.to_string())?.flatten(),
        "asset" => conn.query_row("SELECT scope_unit_id FROM assets WHERE id=?1", [&center.object_id], |row| row.get::<_, Option<String>>(0)).optional().map_err(|e| e.to_string())?.flatten(),
        _ => None,
    };
    Ok(result)
}

fn ensure_enabled(app: &tauri::AppHandle) -> AppResult<std::path::PathBuf> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    if load_feature_flags(&app_data_dir)?.get("memory") == Some(&true) {
        Ok(app_data_dir)
    } else {
        Err("记忆系统尚未启用".into())
    }
}

#[tauri::command]
pub fn memory_list(
    app: tauri::AppHandle,
    project_path: String,
    query: Option<String>,
) -> AppResult<Vec<MemoryRecord>> {
    let app_data_dir = ensure_enabled(&app)?;
    let project_conn = open_database(Path::new(&project_path))?;
    let global_conn = open_app_database(&app_data_dir)?;
    list_memories(&project_conn, &global_conn, query.as_deref())
}

#[tauri::command]
pub fn memory_create(
    app: tauri::AppHandle,
    project_path: String,
    input: CreateMemoryInput,
) -> AppResult<MemoryRecord> {
    let app_data_dir = ensure_enabled(&app)?;
    if input.storage == "global" {
        create_in_conn(&mut open_app_database(&app_data_dir)?, &input)
    } else {
        create_in_conn(&mut open_database(Path::new(&project_path))?, &input)
    }
}

#[tauri::command]
pub fn memory_update(
    app: tauri::AppHandle,
    project_path: String,
    input: UpdateMemoryInput,
) -> AppResult<MemoryRecord> {
    let app_data_dir = ensure_enabled(&app)?;
    if input.storage == "global" {
        update_in_conn(&mut open_app_database(&app_data_dir)?, &input)
    } else {
        update_in_conn(&mut open_database(Path::new(&project_path))?, &input)
    }
}

#[tauri::command]
pub fn memory_invalidate(
    app: tauri::AppHandle,
    project_path: String,
    storage: String,
    memory_id: String,
) -> AppResult<MemoryRecord> {
    memory_update(
        app,
        project_path,
        UpdateMemoryInput {
            storage,
            memory_id,
            content: None,
            category: None,
            scope_type: None,
            scope_id: None,
            status: Some("invalidated".into()),
            confidence: None,
            priority: None,
            supersedes_id: None,
            confirmed: Some(true),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::init_database;

    fn input(id: &str, status: &str) -> CreateMemoryInput {
        CreateMemoryInput {
            request_id: id.into(),
            storage: "project".into(),
            scope_type: "project".into(),
            scope_id: None,
            category: "style".into(),
            content: format!("记忆 {id}"),
            status: status.into(),
            confidence: None,
            priority: None,
            source_type: Some("user".into()),
            source_id: None,
            excerpt: Some("用户明确要求记住".into()),
            supersedes_id: None,
            confirmed: Some(true),
        }
    }

    #[test]
    fn project_memory_is_idempotent_and_conflict_requires_explicit_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "记忆测试", "short").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        let mut first = input("first", "active");
        first.scope_id = Some(project.id.clone());
        assert_eq!(create_in_conn(&mut conn, &first).unwrap().id, "first");
        assert_eq!(create_in_conn(&mut conn, &first).unwrap().id, "first");
        let mut second = input("second", "active");
        second.scope_id = Some(project.id);
        assert!(create_in_conn(&mut conn, &second)
            .unwrap_err()
            .contains("MEMORY_CONFLICT"));
        second.supersedes_id = Some("first".into());
        assert_eq!(create_in_conn(&mut conn, &second).unwrap().status, "active");
        assert_eq!(
            load_memory(&conn, store_spec("project").unwrap(), "first", &[])
                .unwrap()
                .status,
            "superseded"
        );
    }

    #[test]
    fn long_term_active_memory_requires_confirmation() {
        let temp = tempfile::tempdir().unwrap();
        let mut conn = open_app_database(temp.path()).unwrap();
        let mut global = input("global", "active");
        global.storage = "global".into();
        global.scope_type = "global".into();
        global.confirmed = Some(false);
        assert!(create_in_conn(&mut conn, &global)
            .unwrap_err()
            .contains("CONFIRMATION_REQUIRED"));
    }

    #[test]
    fn lists_five_hundred_project_memories() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "容量测试", "short").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        for index in 0..500 {
            let mut memory = input(&format!("memory-{index:03}"), "candidate");
            memory.scope_id = Some(project.id.clone());
            create_in_conn(&mut conn, &memory).unwrap();
        }
        let ids = list_ids(&conn, store_spec("project").unwrap(), None).unwrap();
        assert_eq!(ids.len(), 500);
    }

    #[test]
    fn searches_chinese_memory_by_substring_when_fts_has_no_token_match() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "搜索测试", "short").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        let mut memory = input("dialogue", "candidate");
        memory.scope_id = Some(project.id);
        memory.content = "对白应保持克制，避免解释性台词".into();
        create_in_conn(&mut conn, &memory).unwrap();

        let ids = list_ids(&conn, store_spec("project").unwrap(), Some("解释性")).unwrap();
        assert_eq!(ids, vec!["dialogue"]);
    }

    #[test]
    fn current_unit_memory_precedes_project_memory() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "范围测试", "short").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        let unit_id = "unit".to_string();
        conn.execute(
            "INSERT INTO content_units (id, project_id, type, name, sort_order, created_at, updated_at) VALUES (?1, ?2, 'short', '正片', 0, ?3, ?3)",
            params![unit_id, project.id, now()],
        )
        .unwrap();

        let mut project_memory = input("project-memory", "active");
        project_memory.scope_id = Some(project.id.clone());
        create_in_conn(&mut conn, &project_memory).unwrap();

        let mut unit_memory = input("unit-memory", "active");
        unit_memory.scope_type = "contentUnit".into();
        unit_memory.scope_id = Some(unit_id.clone());
        unit_memory.category = "dialogue".into();
        create_in_conn(&mut conn, &unit_memory).unwrap();

        let memories = active_project_memories(
            &conn,
            &ObjectRef {
                project_id: project.id,
                object_type: "contentUnit".into(),
                object_id: unit_id,
                field: None,
            },
        )
        .unwrap();
        assert_eq!(memories[0].id, "unit-memory");
        assert_eq!(memories[1].id, "project-memory");
    }
}
