use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::agent_application::{expert, expert_model_override, visual_attachments};
use crate::agent_models::model_choice_for_role;
use crate::app_database::load_feature_flags;
use crate::context::{
    estimate_tokens, neighbor_refs, object_value, parent_ref, search_project, ContextPolicy,
    ObjectRef, SelectionSnapshot, CONTEXT_POLICY_VERSION,
};
use crate::database::{new_id, now, open_database, AppResult};
use crate::memory::{active_global_memories, active_project_memories, MemoryContextEntry};
use crate::prompt_compiler::compile_prompt_preview;

const MAX_TOOL_RESULT_BYTES: usize = 64 * 1024;
const MAX_TOOL_RESULT_TOKENS: usize = 12_000;
const MAX_EXPERT_RESULT_BYTES: usize = 60 * 1024;
const MAX_EXPERT_RESULT_TOKENS: usize = 11_500;
const MAX_AUDIT_TEXT_BYTES: usize = 4 * 1024;
const MAX_CACHE_ENTRIES: usize = 256;
static TOOL_CACHE: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();

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
    let (
        session_id,
        agent_type,
        task_type,
        selection_json,
        write_scope_json,
        model_provider,
        model_name,
    ): (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT session_id, agent_type, task_type, selection_json, write_scope_json,
                    model_provider, model_name
             FROM agent_tasks WHERE id=?1",
            [&request.task_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .map_err(|_| "OBJECT_NOT_FOUND: AgentTask 不存在".to_string())?;
    if session_id != request.session_id {
        return Err("TOOL_SCOPE_DENIED: Tool Call 与 AgentSession 不匹配".into());
    }
    let (project_id, revision): (String, i64) = conn
        .query_row("SELECT id, revision FROM projects LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| e.to_string())?;
    let started_at = now();
    conn.execute(
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
            audit_arguments(&request.tool_name, &request.arguments),
            revision,
            started_at,
        ],
    )
    .map_err(|e| format!("记录 Tool Call 失败：{e}"))?;

    let selection: SelectionSnapshot =
        serde_json::from_str(&selection_json).map_err(|e| format!("读取任务选区失败：{e}"))?;
    let policy = ContextPolicy::for_intent(&task_type);
    let cacheable = is_read_tool(&request.tool_name);
    let cache_key = format!(
        "{}\n{}\n{}\n{}\n{}",
        project_path.display(),
        request.task_id,
        revision,
        request.tool_name,
        request.arguments
    );
    let cached = cacheable.then(|| cache_get(&cache_key)).flatten();
    let cache_hit = cached.is_some();
    let result = cached
        .map(Ok)
        .unwrap_or_else(|| {
            argument_object(&request.arguments).and_then(|arguments| {
                validate_arguments(&request.tool_name, arguments)?;
                ensure_tool_allowed(&agent_type, &task_type, &request.tool_name)?;
                match request.tool_name.as_str() {
                    "call_expert" => {
                        let tx = conn.transaction().map_err(|e| e.to_string())?;
                        let value = start_expert(
                            &tx,
                            app_data_dir,
                            project_path,
                            &project_id,
                            revision,
                            &request.task_id,
                            &session_id,
                            &agent_type,
                            &selection,
                            model_provider.clone(),
                            model_name.clone(),
                            arguments,
                        )?;
                        tx.commit().map_err(|e| e.to_string())?;
                        Ok(value)
                    }
                    "complete_expert" => {
                        let tx = conn.transaction().map_err(|e| e.to_string())?;
                        let value = complete_expert(
                            &tx,
                            &request.task_id,
                            &session_id,
                            arguments,
                        )?;
                        tx.commit().map_err(|e| e.to_string())?;
                        Ok(value)
                    }
                    "fail_expert" => {
                        let tx = conn.transaction().map_err(|e| e.to_string())?;
                        let value = fail_expert(
                            &tx,
                            &request.task_id,
                            &session_id,
                            arguments,
                        )?;
                        tx.commit().map_err(|e| e.to_string())?;
                        Ok(value)
                    }
                    _ => execute_inner(
                        &conn,
                        app_data_dir,
                        project_path,
                        &project_id,
                        &selection,
                        &write_scope_json,
                        &policy,
                        &request.tool_name,
                        arguments,
                    ),
                }
            })
        })
        .and_then(|data| {
            let token_estimate =
                estimate_tokens(&serde_json::to_string(&data).map_err(|e| e.to_string())?);
            if token_estimate > MAX_TOOL_RESULT_TOKENS {
                return Err(format!(
                    "TOOL_RESULT_TOO_LARGE: 工具结果约 {token_estimate} tokens，最大允许 {MAX_TOOL_RESULT_TOKENS} tokens"
                ));
            }
            let wrapped = json!({
                "projectRevision": revision,
                "contextPolicy": CONTEXT_POLICY_VERSION,
                "cached": cache_hit,
                "tokenEstimate": token_estimate,
                "data": data,
            });
            let bytes = serde_json::to_vec(&wrapped).map_err(|e| e.to_string())?.len();
            if bytes > MAX_TOOL_RESULT_BYTES {
                return Err(format!(
                    "TOOL_RESULT_TOO_LARGE: 工具结果 {bytes} bytes，最大允许 {MAX_TOOL_RESULT_BYTES} bytes"
                ));
            }
            if cacheable && !cache_hit {
                cache_put(cache_key.clone(), data);
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
    let tx = conn.transaction().map_err(|e| e.to_string())?;
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

fn is_read_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "get_selection"
            | "read_object"
            | "read_parent"
            | "read_children"
            | "read_neighbors"
            | "read_scene"
            | "read_shot_context"
            | "read_asset"
            | "read_generation_task"
            | "read_story_structure"
            | "search_project"
            | "read_active_memories"
            | "read_change_set"
    )
}

fn ensure_tool_allowed(agent_type: &str, task_type: &str, tool_name: &str) -> AppResult<()> {
    if task_type == "expert_team_synthesis" {
        return Err(format!(
            "TOOL_SCOPE_DENIED: 专家团综合任务不允许调用工具：{tool_name}"
        ));
    }
    let allowed = if matches!(tool_name, "complete_expert" | "fail_expert") {
        task_type == "professional_consultation" && agent_type != "main"
    } else if agent_type == "main" {
        tool_name == "call_expert"
            || matches!(
                tool_name,
                "get_selection"
                    | "read_object"
                    | "read_parent"
                    | "read_children"
                    | "read_neighbors"
                    | "read_scene"
                    | "read_shot_context"
                    | "read_asset"
                    | "read_generation_task"
                    | "compile_prompt_preview"
                    | "read_story_structure"
                    | "search_project"
                    | "read_active_memories"
                    | "read_change_set"
            )
    } else {
        expert_tool_names(agent_type).contains(&tool_name)
    };
    if allowed {
        Ok(())
    } else {
        Err(format!(
            "TOOL_SCOPE_DENIED: {agent_type}/{task_type} 不允许调用工具：{tool_name}"
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn start_expert(
    conn: &Connection,
    app_data_dir: Option<&Path>,
    project_path: &Path,
    project_id: &str,
    revision: i64,
    parent_task_id: &str,
    parent_session_id: &str,
    parent_agent_type: &str,
    parent_selection: &SelectionSnapshot,
    provider: Option<String>,
    model: Option<String>,
    arguments: &Map<String, Value>,
) -> AppResult<Value> {
    if parent_agent_type != "main" {
        return Err("TOOL_SCOPE_DENIED: 只有主 Agent 可以调用专业 Agent".into());
    }
    let expert_type = string_arg(arguments, "expertType")?;
    let definition = expert(&expert_type)
        .ok_or_else(|| format!("TOOL_ARGUMENT_INVALID: 未知专业 Agent：{expert_type}"))?;
    let task = long_string_arg(arguments, "task", 4_000)?;
    let focus_refs = focus_refs(conn, project_id, arguments.get("focusRefs"))?;
    let mut selection = parent_selection.clone();
    if !focus_refs.is_empty() {
        selection.center = focus_refs.first().cloned();
        selection.selected = focus_refs.clone();
    }
    selection.project_revision = revision;
    let app_choice = app_data_dir
        .map(|path| model_choice_for_role(path, &expert_type))
        .transpose()?
        .unwrap_or_default();
    let thinking_level = app_choice.thinking_level.unwrap_or_else(|| "medium".into());
    let (provider, model) = expert_model_override(
        conn,
        project_id,
        &expert_type,
        app_choice.provider.or(provider),
        app_choice.model.or(model),
    )?;
    let attachments = visual_attachments(project_path, conn, &expert_type, &selection)?;
    let expert_session_id = new_id();
    let expert_task_id = new_id();
    let timestamp = now();
    let scope_id = selection
        .center
        .as_ref()
        .map(|reference| reference.object_id.as_str());
    conn.execute(
        "INSERT INTO agent_sessions
         (id, project_id, scope_type, scope_id, title, status, runtime_session_id,
          session_kind, parent_session_id, expert_type, session_status,
          last_active_at, created_at, updated_at)
         VALUES (?1, ?2, 'selection', ?3, ?4, 'active', ?1, 'expert', ?5, ?6,
                 'active', ?7, ?7, ?7)",
        params![
            expert_session_id,
            project_id,
            scope_id,
            format!("{}：{}", definition.display_name, truncate_utf8(&task, 80)),
            parent_session_id,
            expert_type,
            timestamp,
        ],
    )
    .map_err(|e| format!("创建专业 AgentSession 失败：{e}"))?;
    conn.execute(
        "INSERT INTO agent_tasks
         (id, session_id, task_type, interaction_mode, agent_type, selection_json,
          read_scope_json, write_scope_json, context_revision, base_revision, status,
          model_provider, model_name, pi_session_id, created_at, started_at)
         VALUES (?1, ?2, 'professional_consultation', 'suggestion', ?3, ?4, ?5,
                 '{\"refs\":[],\"protectedRefs\":[]}', ?6, ?6, 'running', ?7, ?8, ?2, ?9, ?9)",
        params![
            expert_task_id,
            expert_session_id,
            expert_type,
            serde_json::to_string(&selection).map_err(|e| e.to_string())?,
            serde_json::to_string(&focus_refs).map_err(|e| e.to_string())?,
            revision,
            provider,
            model,
            timestamp,
        ],
    )
    .map_err(|e| format!("创建专业 AgentTask 失败：{e}"))?;
    conn.execute(
        "INSERT INTO agent_messages
         (id, session_id, role, agent_type, content, created_at)
         VALUES (?1, ?2, 'user', ?3, ?4, ?5)",
        params![new_id(), expert_session_id, expert_type, task, timestamp],
    )
    .map_err(|e| e.to_string())?;
    let allowed_tools = expert_tool_names(&expert_type);
    let system_prompt = professional_system_prompt(&expert_type)?;
    Ok(json!({
        "expertType": expert_type,
        "expertSessionId": expert_session_id,
        "expertTaskId": expert_task_id,
        "runtimeSessionId": expert_session_id,
        "systemPrompt": system_prompt,
        "allowedTools": allowed_tools,
        "provider": provider,
        "model": model,
        "thinkingLevel": thinking_level,
        "images": attachments,
        "parentTaskId": parent_task_id,
    }))
}

fn complete_expert(
    conn: &Connection,
    task_id: &str,
    session_id: &str,
    arguments: &Map<String, Value>,
) -> AppResult<Value> {
    let runtime_session_id = string_arg(arguments, "runtimeSessionId")?;
    let output = long_string_arg(arguments, "result", MAX_EXPERT_RESULT_BYTES)?;
    let token_estimate = estimate_tokens(&output);
    if token_estimate > MAX_EXPERT_RESULT_TOKENS {
        return Err(format!(
            "TOOL_RESULT_TOO_LARGE: 专业 Agent 结果约 {token_estimate} tokens，最大允许 {MAX_EXPERT_RESULT_TOKENS} tokens"
        ));
    }
    let mut result =
        serde_json::from_str::<Value>(&output).unwrap_or_else(|_| json!({ "summary": output }));
    let context_revision: i64 = conn
        .query_row(
            "SELECT context_revision FROM agent_tasks WHERE id=?1 AND session_id=?2",
            params![task_id, session_id],
            |row| row.get(0),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: 专业 AgentTask 不存在".to_string())?;
    let current_revision: i64 = conn
        .query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(|error| error.to_string())?;
    let stale = context_revision != current_revision;
    if let Some(result) = result.as_object_mut() {
        result.insert("baseRevision".into(), json!(context_revision));
        result.insert("currentRevision".into(), json!(current_revision));
        result.insert("stale".into(), json!(stale));
    }
    let status = if stale { "stale" } else { "completed" };
    let timestamp = now();
    let changed = conn
        .execute(
            "UPDATE agent_tasks
             SET status=?1, result_json=?2, completed_at=?3
             WHERE id=?4 AND session_id=?5 AND task_type='professional_consultation'
               AND status IN ('queued','running')",
            params![status, result.to_string(), timestamp, task_id, session_id],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("SESSION_BUSY: 专业 AgentTask 已结束或不匹配".into());
    }
    conn.execute(
        "UPDATE agent_sessions
         SET status='closed', session_status='closed', runtime_session_id=?1,
             last_active_at=?2, updated_at=?2 WHERE id=?3 AND session_kind='expert'",
        params![runtime_session_id, timestamp, session_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO agent_messages
         (id, session_id, role, agent_type, content, structured_json, created_at)
         SELECT ?1, ?2, 'assistant', expert_type, ?3, ?4, ?5
         FROM agent_sessions WHERE id=?2",
        params![new_id(), session_id, output, result.to_string(), timestamp],
    )
    .map_err(|e| e.to_string())?;
    Ok(json!({
        "completed": true,
        "expertSessionId": session_id,
        "result": result,
    }))
}

fn fail_expert(
    conn: &Connection,
    task_id: &str,
    session_id: &str,
    arguments: &Map<String, Value>,
) -> AppResult<Value> {
    let error = long_string_arg(arguments, "error", 4_000)?;
    let timestamp = now();
    conn.execute(
        "UPDATE agent_tasks SET status='failed', error_json=?1, completed_at=?2
         WHERE id=?3 AND session_id=?4 AND task_type='professional_consultation'
           AND status IN ('queued','running')",
        params![
            json!({ "message": error }).to_string(),
            timestamp,
            task_id,
            session_id
        ],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agent_sessions SET status='closed', session_status='closed',
         last_active_at=?1, updated_at=?1 WHERE id=?2 AND session_kind='expert'",
        params![timestamp, session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(json!({ "failed": true, "expertSessionId": session_id }))
}

fn focus_refs(
    conn: &Connection,
    project_id: &str,
    value: Option<&Value>,
) -> AppResult<Vec<ObjectRef>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .filter(|items| items.len() <= 8)
        .ok_or("TOOL_ARGUMENT_INVALID: focusRefs 必须是最多 8 项的数组")?;
    items
        .iter()
        .map(|item| {
            let item = item
                .as_object()
                .ok_or("TOOL_ARGUMENT_INVALID: focusRefs 项必须是对象")?;
            if item
                .keys()
                .any(|key| !matches!(key.as_str(), "objectType" | "objectId"))
            {
                return Err("TOOL_ARGUMENT_INVALID: focusRefs 含未知字段".into());
            }
            let reference = object_ref(project_id, item)?;
            object_value(conn, &reference, true)?;
            Ok(reference)
        })
        .collect()
}

pub(crate) fn expert_tool_names(expert_type: &str) -> &'static [&'static str] {
    match expert_type {
        "writer" => &[
            "get_selection",
            "read_object",
            "read_parent",
            "read_children",
            "read_neighbors",
            "read_story_structure",
            "search_project",
            "read_active_memories",
        ],
        "director" => &[
            "get_selection",
            "read_scene",
            "read_shot_context",
            "read_neighbors",
            "read_asset",
            "search_project",
        ],
        "cinematography" => &[
            "get_selection",
            "read_shot_context",
            "read_asset",
            "read_neighbors",
        ],
        "art" => &[
            "get_selection",
            "read_asset",
            "read_shot_context",
            "search_project",
            "read_active_memories",
        ],
        "keyframe" => &["get_selection", "read_shot_context", "read_asset"],
        "prompt" => &[
            "get_selection",
            "read_generation_task",
            "compile_prompt_preview",
            "read_shot_context",
            "read_asset",
        ],
        _ => &[],
    }
}

pub(crate) fn professional_system_prompt(expert_type: &str) -> AppResult<String> {
    let definition = expert(expert_type)
        .ok_or_else(|| format!("TOOL_ARGUMENT_INVALID: 未知专业 Agent：{expert_type}"))?;
    Ok(format!(
        "你是创作工作台的{}。{} 你拥有独立 Pi AgentSession，只能使用当前开放的只读工作台工具核对项目事实；不得猜测，不得调用其他专业 Agent，不得访问文件、Shell、PowerShell 或数据库。你只返回结构化专业意见，修改建议交给主 Agent 综合，不能直接写入项目。",
        definition.display_name, definition.system_instruction
    ))
}

#[allow(clippy::too_many_arguments)]
fn execute_inner(
    conn: &Connection,
    app_data_dir: Option<&Path>,
    project_path: &Path,
    project_id: &str,
    selection: &SelectionSnapshot,
    write_scope_json: &str,
    policy: &ContextPolicy,
    tool_name: &str,
    arguments: &Map<String, Value>,
) -> AppResult<Value> {
    match tool_name {
        "get_selection" => Ok(json!({
            "selection": selection,
            "creativePreferences": crate::creative_settings::selection_prompt(conn, selection)?,
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
            read_shot_context(conn, project_id, &string_arg(arguments, "shotId")?, policy)
        }
        "read_asset" => read_asset(conn, project_id, &string_arg(arguments, "assetId")?),
        "read_generation_task" => read_generation_task(
            conn,
            project_id,
            &string_arg(arguments, "generationTaskId")?,
        ),
        "compile_prompt_preview" => {
            let preview = compile_prompt_preview(
                app_data_dir.ok_or("TOOL_NOT_AVAILABLE: 缺少应用数据目录")?,
                project_path,
                string_arg(arguments, "generationTaskId")?,
                optional_string_arg(arguments, "modelProfileKey")?,
                optional_string_arg(arguments, "templateId")?,
            )?;
            Ok(json!({
                "generationTaskId": preview.generation_task_id,
                "modelProfileKey": preview.model_profile_key,
                "modelProfileVersion": preview.model_profile_version,
                "templateId": preview.template_id,
                "templateVersion": preview.template_version,
                "sourceRevision": preview.source_revision,
                "compiledPrompt": preview.compiled_prompt,
                "sourceMap": preview.source_map,
                "warnings": preview.warnings,
                "referenceImages": preview.reference_images.into_iter().map(|image| json!({
                    "sourceType": image.source_type,
                    "sourceId": image.source_id,
                    "label": image.label,
                })).collect::<Vec<_>>(),
                "status": "preview",
                "persisted": false,
                "videoGenerationCalled": false,
            }))
        }
        "read_story_structure" => read_story_structure(
            conn,
            project_id,
            optional_string_arg(arguments, "scopeType")?
                .as_deref()
                .unwrap_or("project"),
            optional_string_arg(arguments, "scopeId")?.as_deref(),
            usize_arg(arguments, "limit", 120, 160)?,
            policy,
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

fn read_shot_context(
    conn: &Connection,
    project_id: &str,
    shot_id: &str,
    policy: &ContextPolicy,
) -> AppResult<Value> {
    let shot = reference(project_id, "shot", shot_id);
    let scene =
        parent_ref(conn, &shot)?.ok_or_else(|| "OBJECT_NOT_FOUND: 镜头缺少所属场".to_string())?;
    let neighbors = neighbor_refs(conn, &shot, policy.neighbor_count)?
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
    policy: &ContextPolicy,
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
    let relations = query_refs(conn, "SELECT id FROM relations WHERE project_id=?1 ORDER BY importance DESC, created_at, id LIMIT ?2", project_id, "relation", project_id, limit.min(policy.relation_limit))?
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
        "compile_prompt_preview" => &["generationTaskId", "modelProfileKey", "templateId"],
        "read_story_structure" => &["scopeType", "scopeId", "limit"],
        "search_project" => &["query", "limit"],
        "read_active_memories" => &["objectType", "objectId"],
        "read_change_set" => &["changeSetId"],
        "call_expert" => &["expertType", "task", "focusRefs"],
        "complete_expert" => &["runtimeSessionId", "result"],
        "fail_expert" => &["error"],
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

fn long_string_arg(
    arguments: &Map<String, Value>,
    key: &str,
    maximum_bytes: usize,
) -> AppResult<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= maximum_bytes)
        .map(str::to_string)
        .ok_or_else(|| format!("TOOL_ARGUMENT_INVALID: {key} 不能为空或超过 {maximum_bytes} bytes"))
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

fn cache_get(key: &str) -> Option<Value> {
    TOOL_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|cache| cache.get(key).cloned())
}

fn cache_put(key: String, value: Value) {
    let Ok(mut cache) = TOOL_CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock() else {
        return;
    };
    if cache.len() >= MAX_CACHE_ENTRIES {
        if let Some(key_to_remove) = cache.keys().next().cloned() {
            cache.remove(&key_to_remove);
        }
    }
    cache.insert(key, value);
}

fn audit_text(value: &Value) -> String {
    let redacted = redact(value);
    truncate_utf8(&redacted.to_string(), MAX_AUDIT_TEXT_BYTES)
}

fn audit_arguments(tool_name: &str, arguments: &Value) -> String {
    if tool_name == "complete_expert" {
        return audit_text(&json!({
            "runtimeSessionId": arguments.get("runtimeSessionId"),
            "result": "[stored in professional AgentTask]",
        }));
    }
    audit_text(arguments)
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
    use crate::app_database::open_app_database;
    use crate::database::{init_database, now};

    fn setup_project() -> (tempfile::TempDir, String) {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("media")).unwrap();
        std::fs::write(temp.path().join("media/hero.png"), b"\x89PNG\r\n\x1a\n").unwrap();
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
        conn.execute("INSERT INTO asset_media (id, asset_id, file_path, label, is_primary, created_at, updated_at) VALUES ('hero-image', 'hero', 'media/hero.png', '正面设定', 1, ?1, ?1)", [&timestamp]).unwrap();
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
        call_as(path, id, "task", "session", tool_name, arguments)
    }

    fn call_as(
        path: &Path,
        id: &str,
        task_id: &str,
        session_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> AppResult<Value> {
        execute_tool(
            path,
            None,
            ToolGatewayRequest {
                tool_call_id: id.into(),
                task_id: task_id.into(),
                session_id: session_id.into(),
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
        assert_eq!(asset["cached"], true);
        assert_eq!(asset["contextPolicy"], CONTEXT_POLICY_VERSION);
        assert!(asset["tokenEstimate"]
            .as_u64()
            .is_some_and(|value| value > 0));
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
    fn prompt_agent_compiles_readonly_preview_without_persisting_or_exposing_paths() {
        let (temp, _) = setup_project();
        let app_data_dir = temp.path().join("app-data");
        let app_conn = open_app_database(&app_data_dir).unwrap();
        let timestamp = now();
        app_conn.execute("INSERT INTO model_profiles (key,display_name,provider,prompt_format,image_reference_rules,supports_start_end_frame,recommended_constraints_json,prohibited_patterns_json,version,created_at,updated_at) VALUES ('preview-model','预览模型','test','plain_text','',0,'[]','[]','1.0',?1,?1)", [&timestamp]).unwrap();
        app_conn.execute("INSERT INTO prompt_templates (id,scope,model_profile_key,name,version,template_body,conditional_rules_json,active,created_at,updated_at) VALUES ('preview-template','global','preview-model','预览模板','1.0','{{header}}\n{{shots}}','{}',1,?1,?1)", [&timestamp]).unwrap();
        drop(app_conn);

        let result = execute_tool(
            temp.path(),
            Some(&app_data_dir),
            ToolGatewayRequest {
                tool_call_id: "tool-compile-preview".into(),
                task_id: "task".into(),
                session_id: "session".into(),
                tool_name: "compile_prompt_preview".into(),
                arguments: json!({ "generationTaskId": "generation" }),
            },
        )
        .unwrap();
        assert_eq!(result["data"]["status"], "preview");
        assert_eq!(result["data"]["persisted"], false);
        assert_eq!(result["data"]["videoGenerationCalled"], false);
        assert!(result["data"]["compiledPrompt"]
            .as_str()
            .is_some_and(|value| value.contains("镜头04")));
        assert!(!result.to_string().contains("C:/private"));
        assert!(!result.to_string().contains("filePath"));
        let project_conn = open_database(temp.path()).unwrap();
        assert_eq!(
            project_conn
                .query_row("SELECT COUNT(*) FROM prompt_compilations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn creates_all_six_professional_sessions_with_bounded_tools_and_results() {
        let (temp, _) = setup_project();
        for expert_type in [
            "writer",
            "director",
            "cinematography",
            "art",
            "keyframe",
            "prompt",
        ] {
            let launch = call(
                temp.path(),
                &format!("call-{expert_type}"),
                "call_expert",
                json!({
                    "expertType": expert_type,
                    "task": "判断镜头04的专业问题",
                    "focusRefs": [{ "objectType": "shot", "objectId": "shot04" }],
                }),
            )
            .unwrap();
            assert_eq!(launch["cached"], false);
            assert_eq!(launch["data"]["expertType"], expert_type);
            assert!(launch["data"]["systemPrompt"]
                .as_str()
                .is_some_and(|value| value.contains("独立 Pi AgentSession")));
            let tools = launch["data"]["allowedTools"].as_array().unwrap();
            assert!(!tools.is_empty());
            assert!(!tools.iter().any(|tool| tool == "call_expert"));
            let session_id = launch["data"]["expertSessionId"].as_str().unwrap();
            let task_id = launch["data"]["expertTaskId"].as_str().unwrap();
            if expert_type == "cinematography" {
                let context = call_as(
                    temp.path(),
                    "cinema-read",
                    task_id,
                    session_id,
                    "read_shot_context",
                    json!({ "shotId": "shot04" }),
                )
                .unwrap();
                assert_eq!(context["data"]["shot"]["id"], "shot04");
                assert!(call_as(
                    temp.path(),
                    "recursive-expert",
                    task_id,
                    session_id,
                    "call_expert",
                    json!({
                        "expertType": "director",
                        "task": "继续调用导演",
                        "focusRefs": [],
                    }),
                )
                .unwrap_err()
                .starts_with("TOOL_SCOPE_DENIED"));
            }
            call_as(
                temp.path(),
                &format!("complete-{expert_type}"),
                task_id,
                session_id,
                "complete_expert",
                json!({
                    "runtimeSessionId": session_id,
                    "result": format!(r#"{{"summary":"{expert_type} 专业意见"}}"#),
                }),
            )
            .unwrap();
        }
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM agent_sessions
                 WHERE session_kind='expert' AND session_status='closed'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            6
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM agent_tasks
                 WHERE task_type='professional_consultation' AND status='completed'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            6
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM agent_tool_calls
                 WHERE task_id IN (SELECT id FROM agent_tasks WHERE agent_type='cinematography')
                   AND tool_name='read_shot_context' AND agent_type='cinematography'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn single_expert_uses_role_model_thinking_and_focus_specific_images() {
        let (temp, project_id) = setup_project();
        let app_data_dir = temp.path().join("app-data");
        let app_conn = open_app_database(&app_data_dir).unwrap();
        app_conn.execute(
            "INSERT INTO app_settings (key,value_json,updated_at) VALUES ('agent_model_settings',?1,?2)",
            params![json!({
                "defaultModel": { "provider": "main-provider", "model": "main-model", "thinkingLevel": "low" },
                "professionalOverrides": {
                    "cinematography": { "provider": "vision-provider", "model": "vision-model", "thinkingLevel": "high" }
                }
            }).to_string(), now()],
        ).unwrap();
        open_database(temp.path()).unwrap().execute(
            "INSERT INTO project_expert_overrides (id,project_id,expert_type,enabled,model_provider,model_name,created_at,updated_at) VALUES ('cinema-override',?1,'cinematography',1,'project-provider','project-model',?2,?2)",
            params![project_id, now()],
        ).unwrap();
        let launch = execute_tool(
            temp.path(),
            Some(&app_data_dir),
            ToolGatewayRequest {
                tool_call_id: "role-model".into(),
                task_id: "task".into(),
                session_id: "session".into(),
                tool_name: "call_expert".into(),
                arguments: json!({
                    "expertType": "cinematography",
                    "task": "检查主角构图",
                    "focusRefs": [{ "objectType": "shot", "objectId": "shot04" }],
                }),
            },
        )
        .unwrap();
        assert_eq!(launch["data"]["provider"], "project-provider");
        assert_eq!(launch["data"]["model"], "project-model");
        assert_eq!(launch["data"]["thinkingLevel"], "high");
        assert_eq!(launch["data"]["images"].as_array().unwrap().len(), 1);
        assert_eq!(launch["data"]["images"][0]["name"], "正面设定");
    }

    #[test]
    fn synthesis_task_is_technically_denied_every_gateway_tool() {
        let (temp, _) = setup_project();
        open_database(temp.path())
            .unwrap()
            .execute(
                "UPDATE agent_tasks SET task_type='expert_team_synthesis' WHERE id='task'",
                [],
            )
            .unwrap();
        let error = call(temp.path(), "synthesis-read", "get_selection", json!({})).unwrap_err();
        assert!(error.starts_with("TOOL_SCOPE_DENIED"));
        let error = call(
            temp.path(),
            "synthesis-expert",
            "call_expert",
            json!({ "expertType": "writer", "task": "绕过确认", "focusRefs": [] }),
        )
        .unwrap_err();
        assert!(error.starts_with("TOOL_SCOPE_DENIED"));
        assert_eq!(
            open_database(temp.path())
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM agent_sessions WHERE session_kind='expert'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
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
        drop(conn);
        let first = call(temp.path(), "cache-first", "get_selection", json!({})).unwrap();
        let second = call(temp.path(), "cache-second", "get_selection", json!({})).unwrap();
        assert_eq!(first["cached"], false);
        assert_eq!(second["cached"], true);
        let conn = open_database(temp.path()).unwrap();
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
        let fresh = call(temp.path(), "fresh", "get_selection", json!({})).unwrap();
        assert_eq!(fresh["projectRevision"], 8);
        assert_eq!(fresh["cached"], false);
    }
}
