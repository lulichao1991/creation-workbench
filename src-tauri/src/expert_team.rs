use crate::agent_application::{
    build_expert_prompt, ensure_expert_agents_enabled, expert, expert_model_override,
};
use crate::agent_runtime::{
    RuntimeEvent, RuntimeEventSink, RuntimeState, RuntimeTaskInput, RUNTIME_EVENT_NAME,
};
use crate::app_database::load_feature_flags;
use crate::context::{build_context_with_memories, BuildContextInput, SelectionSnapshot};
use crate::database::{now, open_database, AppResult};
use crate::memory::{active_global_memories, MemoryContextEntry};
use crate::permission::WriteScope;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

type EventEmitter = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestExpertTeamInput {
    pub request_id: String,
    pub session_id: String,
    pub message: String,
    pub selection: SelectionSnapshot,
    pub members: Vec<String>,
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmExpertTeamInput {
    pub consultation_id: String,
    pub confirmed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertTeamMember {
    pub id: String,
    pub expert_type: String,
    pub task_id: Option<String>,
    pub status: String,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertTeamConsultation {
    pub id: String,
    pub request_task_id: String,
    pub session_id: String,
    pub user_request: String,
    pub selection: SelectionSnapshot,
    pub members: Vec<ExpertTeamMember>,
    pub cost_level: String,
    pub read_only: bool,
    pub token_budget: usize,
    pub base_revision: i64,
    pub status: String,
    pub synthesis_task_id: Option<String>,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub application_card_id: String,
    pub cost_card_id: String,
    pub created_at: String,
    pub confirmed_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemberDraft {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    findings: Vec<Value>,
    patch_proposal: Option<Value>,
    #[serde(default)]
    related_impacts: Vec<Value>,
    #[serde(default)]
    permission_requests: Vec<Value>,
    #[serde(default)]
    questions: Vec<String>,
    #[serde(default)]
    risks: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SynthesisDraft {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    consensus: Vec<Value>,
    #[serde(default)]
    disagreements: Vec<Value>,
    #[serde(default)]
    recommendations: Vec<Value>,
    #[serde(default)]
    questions: Vec<String>,
    #[serde(default)]
    risks: Vec<String>,
}

struct PreparedRuntimeTask {
    input: RuntimeTaskInput,
}

fn default_token_budget() -> usize {
    8_000
}

#[tauri::command]
pub fn expert_team_request(
    app: tauri::AppHandle,
    project_path: String,
    input: RequestExpertTeamInput,
) -> AppResult<ExpertTeamConsultation> {
    ensure_expert_team_enabled(&app)?;
    request_consultation(Path::new(&project_path), input)
}

#[tauri::command]
pub fn expert_team_confirm(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeState>,
    project_path: String,
    input: ConfirmExpertTeamInput,
) -> AppResult<ExpertTeamConsultation> {
    ensure_expert_team_enabled(&app)?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    let memories = if load_feature_flags(&app_data_dir)?.get("memory") == Some(&true) {
        Some(active_global_memories(&app_data_dir)?)
    } else {
        None
    };
    let event_app = app.clone();
    let emitter: EventEmitter = Arc::new(move |event| {
        let _ = event_app.emit(RUNTIME_EVENT_NAME, event);
    });
    confirm_consultation(
        Path::new(&project_path),
        input,
        memories.as_deref(),
        runtime.inner().clone(),
        Some(emitter),
    )
}

#[tauri::command]
pub fn expert_team_get(
    app: tauri::AppHandle,
    project_path: String,
    consultation_id: String,
) -> AppResult<ExpertTeamConsultation> {
    ensure_expert_team_enabled(&app)?;
    let conn = open_database(Path::new(&project_path))?;
    load_consultation(&conn, &consultation_id)
}

#[tauri::command]
pub fn expert_team_list(
    app: tauri::AppHandle,
    project_path: String,
    session_id: String,
) -> AppResult<Vec<ExpertTeamConsultation>> {
    ensure_expert_team_enabled(&app)?;
    list_consultations(Path::new(&project_path), &session_id)
}

#[tauri::command]
pub fn expert_team_cancel(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeState>,
    project_path: String,
    consultation_id: String,
) -> AppResult<ExpertTeamConsultation> {
    ensure_expert_team_enabled(&app)?;
    cancel_consultation(Path::new(&project_path), &consultation_id, runtime.inner())
}

fn ensure_expert_team_enabled(app: &tauri::AppHandle) -> AppResult<()> {
    ensure_expert_agents_enabled(app)?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    if load_feature_flags(&app_data_dir)?.get("expert_team") != Some(&true) {
        return Err("FEATURE_DISABLED: 专家团特性尚未启用".into());
    }
    Ok(())
}

fn request_consultation(
    project_path: &Path,
    input: RequestExpertTeamInput,
) -> AppResult<ExpertTeamConsultation> {
    if input.request_id.trim().is_empty()
        || input.session_id.trim().is_empty()
        || input.message.trim().is_empty()
    {
        return Err("TOOL_ARGUMENT_INVALID: requestId、sessionId 和 message 不能为空".into());
    }
    if !(32..=100_000).contains(&input.token_budget) {
        return Err("TOOL_ARGUMENT_INVALID: tokenBudget 必须在 32–100000 之间".into());
    }
    let mut unique = HashSet::new();
    if !(2..=6).contains(&input.members.len())
        || input
            .members
            .iter()
            .any(|member| expert(member).is_none() || !unique.insert(member.clone()))
    {
        return Err("TOOL_ARGUMENT_INVALID: 专家团必须选择 2–6 位不重复的已注册专家".into());
    }
    let mut conn = open_database(project_path)?;
    if let Ok(existing) = load_consultation(&conn, &input.request_id) {
        if existing.session_id != input.session_id || existing.user_request != input.message {
            return Err("TOOL_ARGUMENT_INVALID: requestId 已被其他专家团申请使用".into());
        }
        return Ok(existing);
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (project_id, revision): (String, i64) = tx
        .query_row("SELECT id, revision FROM projects LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| e.to_string())?;
    if input.selection.project_id != project_id || input.selection.project_revision != revision {
        return Err("REVISION_STALE: 当前选区不属于项目或已过期".into());
    }
    let session_project: String = tx
        .query_row(
            "SELECT project_id FROM agent_sessions WHERE id=?1 AND status='active'",
            [&input.session_id],
            |row| row.get(0),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: AgentSession 不存在或已关闭".to_string())?;
    if session_project != project_id {
        return Err("TOOL_ARGUMENT_INVALID: AgentSession 不属于当前项目".into());
    }

    let timestamp = now();
    let request_task_id = format!("{}:request", input.request_id);
    let selection_json = serde_json::to_string(&input.selection).map_err(|e| e.to_string())?;
    let members_json = serde_json::to_string(&input.members).map_err(|e| e.to_string())?;
    let empty_scope = serde_json::to_string(&WriteScope::default()).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO agent_messages (id, session_id, role, agent_type, content, created_at) VALUES (?1, ?2, 'user', 'main', ?3, ?4)",
        params![format!("{}:user", input.request_id), input.session_id, input.message, timestamp],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO agent_tasks (id, session_id, task_type, agent_type, selection_json, read_scope_json, write_scope_json, context_revision, status, model_provider, model_name, created_at) VALUES (?1, ?2, 'expert_team_request', 'main', ?3, ?4, ?5, ?6, 'waiting_for_user', ?7, ?8, ?9)",
        params![request_task_id, input.session_id, selection_json, serde_json::to_string(&input.selection.selected).map_err(|e| e.to_string())?, empty_scope, revision, input.provider, input.model, timestamp],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO expert_team_consultations (id, request_task_id, session_id, user_request, selection_json, members_json, cost_level, read_only, token_budget, base_revision, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'high', 1, ?7, ?8, 'awaiting_confirmation', ?9, ?9)",
        params![input.request_id, request_task_id, input.session_id, input.message, selection_json, members_json, input.token_budget as i64, revision, timestamp],
    ).map_err(|e| e.to_string())?;
    for member in &input.members {
        tx.execute(
            "INSERT INTO expert_team_members (id, consultation_id, expert_type, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'planned', ?4, ?4)",
            params![format!("{}:member:{member}", input.request_id), input.request_id, member, timestamp],
        ).map_err(|e| e.to_string())?;
    }
    let member_labels = input
        .members
        .iter()
        .filter_map(|member| expert(member).map(|definition| definition.display_name))
        .collect::<Vec<_>>();
    tx.execute(
        "INSERT INTO ai_cards (id, task_id, card_type, title, body, options_json, status, created_at) VALUES (?1, ?2, 'expert_team', '专家团会诊申请', ?3, ?4, 'open', ?5)",
        params![format!("{}:application", input.request_id), request_task_id, format!("主 Agent 建议由 {} 独立会诊。", member_labels.join("、")), json!({"consultationId": input.request_id, "members": input.members, "memberLabels": member_labels, "readOnly": true}).to_string(), timestamp],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO ai_cards (id, task_id, card_type, title, body, options_json, status, created_at) VALUES (?1, ?2, 'cost', '高成本操作确认', '专家团会同时启动多个独立 Agent 任务；确认前不会产生任何调用。', ?3, 'open', ?4)",
        params![format!("{}:cost", input.request_id), request_task_id, json!({"consultationId": input.request_id, "costLevel": "high", "readOnly": true, "requiresExplicitConfirmation": true}).to_string(), timestamp],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_sessions SET updated_at=?1 WHERE id=?2",
        params![timestamp, input.session_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    load_consultation(&conn, &input.request_id)
}

fn confirm_consultation(
    project_path: &Path,
    input: ConfirmExpertTeamInput,
    global_memories: Option<&[MemoryContextEntry]>,
    runtime: RuntimeState,
    emitter: Option<EventEmitter>,
) -> AppResult<ExpertTeamConsultation> {
    if !input.confirmed {
        return Err("CONFIRMATION_REQUIRED: 必须显式确认专家和高成本后才能启动".into());
    }
    let mut conn = open_database(project_path)?;
    let current = load_consultation(&conn, &input.consultation_id)?;
    if current.status != "awaiting_confirmation" {
        return Ok(current);
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let current_revision: i64 = tx
        .query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    if current_revision != current.base_revision {
        let timestamp = now();
        let result = json!({
            "summary": "专家团申请已过期，请按当前项目重新申请",
            "baseRevision": current.base_revision,
            "currentRevision": current_revision,
            "stale": true,
            "readOnly": true,
        });
        tx.execute(
            "UPDATE expert_team_consultations SET status='stale', result_json=?1, completed_at=?2, updated_at=?2 WHERE id=?3",
            params![result.to_string(), timestamp, current.id],
        ).map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE agent_tasks SET status='stale', result_json=?1, completed_at=?2 WHERE id=?3",
            params![result.to_string(), timestamp, current.request_task_id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE ai_cards SET status='resolved', resolution_json=?1, resolved_at=?2 WHERE task_id=?3 AND status='open'",
            params![json!({"action": "stale", "currentRevision": current_revision}).to_string(), timestamp, current.request_task_id],
        ).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        return Err("REVISION_STALE: 专家团申请基于的项目事实已经变化".into());
    }
    let timestamp = now();
    tx.execute(
        "UPDATE expert_team_consultations SET status='running', confirmed_at=?1, updated_at=?1 WHERE id=?2 AND status='awaiting_confirmation'",
        params![timestamp, current.id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_tasks SET status='running', started_at=?1 WHERE id=?2",
        params![timestamp, current.request_task_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE ai_cards SET status='resolved', resolution_json=?1, resolved_at=?2 WHERE task_id=?3 AND status='open' AND card_type IN ('expert_team', 'cost')",
        params![json!({"action": "confirmed", "costLevel": "high", "readOnly": true}).to_string(), timestamp, current.request_task_id],
    ).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;

    let prepared = match prepare_member_tasks(project_path, &current, global_memories) {
        Ok(prepared) => prepared,
        Err(error) => {
            fail_consultation(project_path, &current.id, &error)?;
            return Err(error);
        }
    };
    for task in prepared {
        let sink = member_event_sink(project_path.to_path_buf(), runtime.clone(), emitter.clone());
        if let Err(error) = runtime.start_task(task.input.clone(), sink) {
            mark_member_failed(
                project_path,
                task.input.task_id.as_deref().unwrap_or_default(),
                &error,
            )?;
            maybe_start_synthesis(project_path, &runtime, emitter.clone())?;
        }
    }
    let conn = open_database(project_path)?;
    load_consultation(&conn, &input.consultation_id)
}

fn prepare_member_tasks(
    project_path: &Path,
    consultation: &ExpertTeamConsultation,
    global_memories: Option<&[MemoryContextEntry]>,
) -> AppResult<Vec<PreparedRuntimeTask>> {
    let mut prepared = Vec::new();
    for member in &consultation.members {
        let task_id = format!("{}:task", member.id);
        let mut conn = open_database(project_path)?;
        let (project_id, revision): (String, i64) = conn
            .query_row("SELECT id, revision FROM projects LIMIT 1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|e| e.to_string())?;
        if revision != consultation.base_revision {
            return Err("REVISION_STALE: 准备专家上下文时项目事实发生变化".into());
        }
        let (request_provider, request_model): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT model_provider, model_name FROM agent_tasks WHERE id=?1",
                [&consultation.request_task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let (provider, model) = expert_model_override(
            &conn,
            &project_id,
            &member.expert_type,
            request_provider,
            request_model,
        )?;
        let timestamp = now();
        conn.execute(
            "INSERT INTO agent_tasks (id, session_id, task_type, agent_type, selection_json, read_scope_json, write_scope_json, context_revision, status, model_provider, model_name, created_at) VALUES (?1, ?2, 'expert_team_member', ?3, ?4, ?5, ?6, ?7, 'context_building', ?8, ?9, ?10)",
            params![task_id, consultation.session_id, member.expert_type, serde_json::to_string(&consultation.selection).map_err(|e| e.to_string())?, serde_json::to_string(&consultation.selection.selected).map_err(|e| e.to_string())?, serde_json::to_string(&WriteScope::default()).map_err(|e| e.to_string())?, revision, provider, model, timestamp],
        ).map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE expert_team_members SET task_id=?1, status='queued', updated_at=?2 WHERE id=?3",
            params![task_id, timestamp, member.id],
        )
        .map_err(|e| e.to_string())?;
        let package = build_context_with_memories(
            &mut conn,
            BuildContextInput {
                task_id: task_id.clone(),
                selection: consultation.selection.clone(),
                task_intent: "expert_team_consultation".into(),
                expert_type: member.expert_type.clone(),
                token_budget: consultation.token_budget,
            },
            global_memories,
        )?;
        let prompt = build_expert_prompt(
            &member.expert_type,
            "discussion",
            &format!(
                "专家团独立只读会诊。请只从你的专业职责分析，不参考或猜测其他专家意见。会诊问题：{}",
                consultation.user_request
            ),
            &WriteScope::default(),
            &package,
            None,
        )?;
        conn.execute(
            "UPDATE agent_tasks SET status='queued' WHERE id=?1",
            [&task_id],
        )
        .map_err(|e| e.to_string())?;
        prepared.push(PreparedRuntimeTask {
            input: RuntimeTaskInput {
                task_id: Some(task_id.clone()),
                session_id: Some(task_id),
                runtime_session_id: None,
                prompt,
                provider,
                model,
                system_prompt: None,
                thinking_level: None,
                attachments: Vec::new(),
            },
        });
    }
    Ok(prepared)
}

fn member_event_sink(
    project_path: PathBuf,
    runtime: RuntimeState,
    emitter: Option<EventEmitter>,
) -> RuntimeEventSink {
    let buffer = Arc::new(Mutex::new(String::new()));
    Arc::new(move |event| {
        handle_member_event(&project_path, &buffer, &event);
        if matches!(
            event,
            RuntimeEvent::TaskCompleted { .. }
                | RuntimeEvent::TaskFailed { .. }
                | RuntimeEvent::TaskCancelled { .. }
        ) {
            let _ = maybe_start_synthesis(&project_path, &runtime, emitter.clone());
        }
        if let Some(emit) = emitter.as_ref() {
            emit(event);
        }
    })
}

fn handle_member_event(project_path: &Path, buffer: &Mutex<String>, event: &RuntimeEvent) {
    match event {
        RuntimeEvent::TaskStarted { task_id } => {
            let _ = update_member_status(project_path, task_id, "running", true);
        }
        RuntimeEvent::TextDelta { delta, .. } => {
            if let Ok(mut text) = buffer.lock() {
                text.push_str(delta);
            }
        }
        RuntimeEvent::UsageUpdated { task_id, usage } => {
            if let Ok(conn) = open_database(project_path) {
                let _ = conn.execute(
                    "UPDATE agent_tasks SET usage_json=?1 WHERE id=?2",
                    params![usage.to_string(), task_id],
                );
            }
        }
        RuntimeEvent::TaskCompleted { task_id } => {
            let raw = buffer.lock().map(|text| text.clone()).unwrap_or_default();
            let _ = complete_member(project_path, task_id, &raw);
        }
        RuntimeEvent::TaskFailed { task_id, error } => {
            let _ = mark_member_failed(project_path, task_id, error);
        }
        RuntimeEvent::TaskCancelled { task_id } => {
            let _ = update_member_status(project_path, task_id, "cancelled", false);
        }
        RuntimeEvent::ToolCallRequested { .. } | RuntimeEvent::ToolCallCompleted { .. } => {}
    }
}

fn complete_member(project_path: &Path, task_id: &str, raw: &str) -> AppResult<()> {
    let mut conn = open_database(project_path)?;
    let current_status: String = conn
        .query_row(
            "SELECT status FROM agent_tasks WHERE id=?1 AND task_type='expert_team_member'",
            [task_id],
            |row| row.get(0),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: 专家团成员任务不存在".to_string())?;
    if current_status == "cancelled" {
        return Ok(());
    }
    let expert_type: String = conn
        .query_row(
            "SELECT expert_type FROM expert_team_members WHERE task_id=?1",
            [task_id],
            |row| row.get(0),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: 专家团成员不存在".to_string())?;
    let mut draft = serde_json::from_str::<MemberDraft>(raw).unwrap_or_else(|_| MemberDraft {
        summary: raw.trim().to_string(),
        ..MemberDraft::default()
    });
    if draft.summary.trim().is_empty() {
        draft.summary = "专家已完成只读分析".into();
    }
    if draft.patch_proposal.is_some() || !draft.permission_requests.is_empty() {
        draft
            .risks
            .push("专家团为只读会诊，已忽略模型返回的修改或扩权请求".into());
    }
    let result = json!({
        "expertType": expert_type,
        "summary": draft.summary,
        "findings": draft.findings,
        "patchProposal": null,
        "relatedImpacts": draft.related_impacts,
        "permissionRequests": [],
        "questions": draft.questions,
        "risks": draft.risks,
        "readOnly": true,
    });
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let timestamp = now();
    tx.execute(
        "UPDATE agent_tasks SET status='completed', result_json=?1, completed_at=?2 WHERE id=?3 AND status<>'cancelled'",
        params![result.to_string(), timestamp, task_id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE expert_team_members SET status='completed', result_json=?1, updated_at=?2 WHERE task_id=?3 AND status<>'cancelled'",
        params![result.to_string(), timestamp, task_id],
    ).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn update_member_status(
    project_path: &Path,
    task_id: &str,
    status: &str,
    started: bool,
) -> AppResult<()> {
    let mut conn = open_database(project_path)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let timestamp = now();
    if started {
        tx.execute(
            "UPDATE agent_tasks SET status=?1, started_at=?2 WHERE id=?3 AND status<>'cancelled'",
            params![status, timestamp, task_id],
        )
        .map_err(|e| e.to_string())?;
    } else {
        tx.execute(
            "UPDATE agent_tasks SET status=?1, completed_at=?2 WHERE id=?3 AND status<>'cancelled'",
            params![status, timestamp, task_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute(
        "UPDATE expert_team_members SET status=?1, updated_at=?2 WHERE task_id=?3 AND status<>'cancelled'",
        params![status, timestamp, task_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn mark_member_failed(project_path: &Path, task_id: &str, error: &str) -> AppResult<()> {
    let mut conn = open_database(project_path)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let timestamp = now();
    let error_json = json!({"message": error, "retryable": true}).to_string();
    tx.execute(
        "UPDATE agent_tasks SET status='failed', error_json=?1, completed_at=?2 WHERE id=?3 AND status<>'cancelled'",
        params![error_json, timestamp, task_id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE expert_team_members SET status='failed', error_json=?1, updated_at=?2 WHERE task_id=?3 AND status<>'cancelled'",
        params![error_json, timestamp, task_id],
    ).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn maybe_start_synthesis(
    project_path: &Path,
    runtime: &RuntimeState,
    emitter: Option<EventEmitter>,
) -> AppResult<()> {
    let Some(input) = prepare_synthesis(project_path)? else {
        return Ok(());
    };
    let task_id = input.task_id.clone().unwrap_or_default();
    let buffer = Arc::new(Mutex::new(String::new()));
    let sink_buffer = Arc::clone(&buffer);
    let sink_path = project_path.to_path_buf();
    let sink_emitter = emitter.clone();
    let sink: RuntimeEventSink = Arc::new(move |event| {
        handle_synthesis_event(&sink_path, &sink_buffer, &event);
        if let Some(emit) = sink_emitter.as_ref() {
            emit(event);
        }
    });
    if let Err(error) = runtime.start_task(input, sink) {
        fail_synthesis(project_path, &task_id, &error)?;
    }
    Ok(())
}

fn prepare_synthesis(project_path: &Path) -> AppResult<Option<RuntimeTaskInput>> {
    let mut conn = open_database(project_path)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let consultation_id: Option<String> = tx
        .query_row(
            "SELECT c.id FROM expert_team_consultations c
             WHERE c.status='running'
               AND NOT EXISTS (SELECT 1 FROM expert_team_members m WHERE m.consultation_id=c.id AND m.status IN ('planned','queued','running'))
             ORDER BY c.confirmed_at, c.id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(consultation_id) = consultation_id else {
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(None);
    };
    let (session_id, request_task_id, user_request, selection_json, base_revision): (
        String,
        String,
        String,
        String,
        i64,
    ) = tx
        .query_row(
            "SELECT session_id, request_task_id, user_request, selection_json, base_revision FROM expert_team_consultations WHERE id=?1",
            [&consultation_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(|e| e.to_string())?;
    let current_revision: i64 = tx
        .query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    let timestamp = now();
    if current_revision != base_revision {
        let result = json!({"summary": "会诊期间项目事实已变化，结果已过期", "baseRevision": base_revision, "currentRevision": current_revision, "stale": true, "readOnly": true});
        finish_consultation_transaction(
            &tx,
            &consultation_id,
            &request_task_id,
            &session_id,
            "stale",
            &result,
            &timestamp,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(None);
    }
    let failed: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM expert_team_members WHERE consultation_id=?1 AND status='failed'",
            [&consultation_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let cancelled: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM expert_team_members WHERE consultation_id=?1 AND status='cancelled'",
            [&consultation_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if failed > 0 || cancelled > 0 {
        let status = if failed > 0 { "failed" } else { "cancelled" };
        let result = json!({"summary": if failed > 0 { "部分专家任务失败，会诊未进入综合" } else { "专家团会诊已取消" }, "readOnly": true});
        finish_consultation_transaction(
            &tx,
            &consultation_id,
            &request_task_id,
            &session_id,
            status,
            &result,
            &timestamp,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(None);
    }

    let members = load_member_results(&tx, &consultation_id)?;
    let synthesis_task_id = format!("{consultation_id}:synthesis");
    let (provider, model): (Option<String>, Option<String>) = tx
        .query_row(
            "SELECT model_provider, model_name FROM agent_tasks WHERE id=?1",
            [&request_task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO agent_tasks (id, session_id, task_type, agent_type, selection_json, read_scope_json, write_scope_json, context_revision, status, model_provider, model_name, created_at) VALUES (?1, ?2, 'expert_team_synthesis', 'main', ?3, '[]', ?4, ?5, 'queued', ?6, ?7, ?8)",
        params![synthesis_task_id, session_id, selection_json, serde_json::to_string(&WriteScope::default()).map_err(|e| e.to_string())?, base_revision, provider, model, timestamp],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE expert_team_consultations SET status='synthesizing', synthesis_task_id=?1, updated_at=?2 WHERE id=?3 AND status='running'",
        params![synthesis_task_id, timestamp, consultation_id],
    ).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    let prompt = build_synthesis_prompt(&user_request, &members)?;
    Ok(Some(RuntimeTaskInput {
        task_id: Some(synthesis_task_id),
        session_id: Some(session_id),
        runtime_session_id: None,
        prompt,
        provider,
        model,
        system_prompt: None,
        thinking_level: None,
        attachments: Vec::new(),
    }))
}

fn build_synthesis_prompt(user_request: &str, members: &Value) -> AppResult<String> {
    Ok(format!(
        "你是创作工作台主 Agent。以下是多个专业 Agent 在互不查看彼此意见的前提下完成的独立只读会诊结果。请整合共识、明确分歧、给出建议和待确认问题，不得伪造一致意见，不得返回 patchProposal、权限申请、SQL、文件操作或任何直接写入命令。如需修改，只能建议用户另行发起修改提案。\n会诊问题：{}\n独立专家结果：{}\n只返回一个 JSON 对象，键必须为 summary、consensus、disagreements、recommendations、questions、risks。",
        user_request,
        serde_json::to_string(members).map_err(|e| e.to_string())?,
    ))
}

fn handle_synthesis_event(project_path: &Path, buffer: &Mutex<String>, event: &RuntimeEvent) {
    match event {
        RuntimeEvent::TaskStarted { task_id } => {
            if let Ok(conn) = open_database(project_path) {
                let _ = conn.execute(
                    "UPDATE agent_tasks SET status='running', started_at=?1 WHERE id=?2 AND status='queued'",
                    params![now(), task_id],
                );
            }
        }
        RuntimeEvent::TextDelta { delta, .. } => {
            if let Ok(mut text) = buffer.lock() {
                text.push_str(delta);
            }
        }
        RuntimeEvent::UsageUpdated { task_id, usage } => {
            if let Ok(conn) = open_database(project_path) {
                let _ = conn.execute(
                    "UPDATE agent_tasks SET usage_json=?1 WHERE id=?2",
                    params![usage.to_string(), task_id],
                );
            }
        }
        RuntimeEvent::TaskCompleted { task_id } => {
            let raw = buffer.lock().map(|text| text.clone()).unwrap_or_default();
            let _ = complete_synthesis(project_path, task_id, &raw);
        }
        RuntimeEvent::TaskFailed { task_id, error } => {
            let _ = fail_synthesis(project_path, task_id, error);
        }
        RuntimeEvent::TaskCancelled { task_id } => {
            let _ = cancel_synthesis(project_path, task_id);
        }
        RuntimeEvent::ToolCallRequested { .. } | RuntimeEvent::ToolCallCompleted { .. } => {}
    }
}

fn complete_synthesis(project_path: &Path, task_id: &str, raw: &str) -> AppResult<()> {
    let mut conn = open_database(project_path)?;
    let row: (String, String, String, i64, String) = conn
        .query_row(
            "SELECT c.id, c.request_task_id, c.session_id, c.base_revision, c.status FROM expert_team_consultations c WHERE c.synthesis_task_id=?1",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: 专家团综合任务不存在".to_string())?;
    if row.4 == "cancelled" {
        return Ok(());
    }
    let current_revision: i64 = conn
        .query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    let draft = serde_json::from_str::<SynthesisDraft>(raw).unwrap_or_else(|_| SynthesisDraft {
        summary: raw.trim().to_string(),
        ..SynthesisDraft::default()
    });
    let members = load_member_results(&conn, &row.0)?;
    let stale = current_revision != row.3;
    let result = json!({
        "summary": if draft.summary.trim().is_empty() { "主 Agent 已完成专家团综合" } else { &draft.summary },
        "consensus": draft.consensus,
        "disagreements": draft.disagreements,
        "recommendations": draft.recommendations,
        "questions": draft.questions,
        "risks": draft.risks,
        "memberResults": members,
        "patchProposal": null,
        "readOnly": true,
        "costLevel": "high",
        "baseRevision": row.3,
        "currentRevision": current_revision,
        "stale": stale,
    });
    let status = if stale { "stale" } else { "completed" };
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let timestamp = now();
    tx.execute(
        "UPDATE agent_tasks SET status=?1, result_json=?2, completed_at=?3 WHERE id=?4 AND status<>'cancelled'",
        params![status, result.to_string(), timestamp, task_id],
    ).map_err(|e| e.to_string())?;
    finish_consultation_transaction(&tx, &row.0, &row.1, &row.2, status, &result, &timestamp)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn fail_synthesis(project_path: &Path, task_id: &str, error: &str) -> AppResult<()> {
    let mut conn = open_database(project_path)?;
    let row: (String, String, String) = conn
        .query_row(
            "SELECT id, request_task_id, session_id FROM expert_team_consultations WHERE synthesis_task_id=?1",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: 专家团综合任务不存在".to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let timestamp = now();
    let error_json = json!({"message": error, "retryable": true});
    tx.execute(
        "UPDATE agent_tasks SET status='failed', error_json=?1, completed_at=?2 WHERE id=?3 AND status<>'cancelled'",
        params![error_json.to_string(), timestamp, task_id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE expert_team_consultations SET status='failed', error_json=?1, completed_at=?2, updated_at=?2 WHERE id=?3 AND status<>'cancelled'",
        params![error_json.to_string(), timestamp, row.0],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_tasks SET status='failed', error_json=?1, completed_at=?2 WHERE id=?3 AND status<>'cancelled'",
        params![error_json.to_string(), timestamp, row.1],
    ).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn cancel_synthesis(project_path: &Path, task_id: &str) -> AppResult<()> {
    let conn = open_database(project_path)?;
    conn.execute(
        "UPDATE agent_tasks SET status='cancelled', completed_at=?1 WHERE id=?2",
        params![now(), task_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn finish_consultation_transaction(
    tx: &rusqlite::Transaction<'_>,
    consultation_id: &str,
    request_task_id: &str,
    session_id: &str,
    status: &str,
    result: &Value,
    timestamp: &str,
) -> AppResult<()> {
    tx.execute(
        "UPDATE expert_team_consultations SET status=?1, result_json=?2, completed_at=?3, updated_at=?3 WHERE id=?4",
        params![status, result.to_string(), timestamp, consultation_id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_tasks SET status=?1, result_json=?2, completed_at=?3 WHERE id=?4",
        params![status, result.to_string(), timestamp, request_task_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT OR REPLACE INTO agent_messages (id, session_id, role, agent_type, content, structured_json, created_at) VALUES (?1, ?2, 'assistant', 'main', ?3, ?4, ?5)",
        params![format!("{consultation_id}:assistant"), session_id, result["summary"].as_str().unwrap_or_default(), result.to_string(), timestamp],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_sessions SET updated_at=?1 WHERE id=?2",
        params![timestamp, session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn fail_consultation(project_path: &Path, consultation_id: &str, error: &str) -> AppResult<()> {
    let mut conn = open_database(project_path)?;
    let consultation = load_consultation(&conn, consultation_id)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let timestamp = now();
    let error_json = json!({"message": error, "retryable": true});
    tx.execute(
        "UPDATE expert_team_consultations SET status='failed', error_json=?1, completed_at=?2, updated_at=?2 WHERE id=?3",
        params![error_json.to_string(), timestamp, consultation_id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE expert_team_members SET status='failed', error_json=?1, updated_at=?2 WHERE consultation_id=?3 AND status IN ('planned','queued')",
        params![error_json.to_string(), timestamp, consultation_id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_tasks SET status='failed', error_json=?1, completed_at=?2 WHERE id=?3 OR id IN (SELECT task_id FROM expert_team_members WHERE consultation_id=?4)",
        params![error_json.to_string(), timestamp, consultation.request_task_id, consultation_id],
    ).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn cancel_consultation(
    project_path: &Path,
    consultation_id: &str,
    runtime: &RuntimeState,
) -> AppResult<ExpertTeamConsultation> {
    let mut conn = open_database(project_path)?;
    let consultation = load_consultation(&conn, consultation_id)?;
    if matches!(
        consultation.status.as_str(),
        "completed" | "cancelled" | "failed" | "stale"
    ) {
        return Ok(consultation);
    }
    let mut task_ids = consultation
        .members
        .iter()
        .filter(|member| matches!(member.status.as_str(), "queued" | "running"))
        .filter_map(|member| member.task_id.clone())
        .collect::<Vec<_>>();
    if let Some(task_id) = consultation.synthesis_task_id.clone() {
        task_ids.push(task_id);
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let timestamp = now();
    let result = json!({"summary": "专家团会诊已取消", "readOnly": true, "costLevel": "high"});
    tx.execute(
        "UPDATE expert_team_consultations SET status='cancelled', result_json=?1, completed_at=?2, updated_at=?2 WHERE id=?3",
        params![result.to_string(), timestamp, consultation_id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE expert_team_members SET status='cancelled', updated_at=?1 WHERE consultation_id=?2 AND status IN ('planned','queued','running')",
        params![timestamp, consultation_id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_tasks SET status='cancelled', result_json=?1, completed_at=?2 WHERE id=?3 OR id IN (SELECT task_id FROM expert_team_members WHERE consultation_id=?4) OR id=?5",
        params![result.to_string(), timestamp, consultation.request_task_id, consultation_id, consultation.synthesis_task_id],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE ai_cards SET status='dismissed', resolution_json=?1, resolved_at=?2 WHERE task_id=?3 AND status='open'",
        params![json!({"action": "cancelled"}).to_string(), timestamp, consultation.request_task_id],
    ).map_err(|e| e.to_string())?;
    finish_consultation_transaction(
        &tx,
        consultation_id,
        &consultation.request_task_id,
        &consultation.session_id,
        "cancelled",
        &result,
        &timestamp,
    )?;
    tx.commit().map_err(|e| e.to_string())?;
    for task_id in task_ids {
        let _ = runtime.cancel_task(&task_id);
    }
    load_consultation(&conn, consultation_id)
}

fn list_consultations(
    project_path: &Path,
    session_id: &str,
) -> AppResult<Vec<ExpertTeamConsultation>> {
    let conn = open_database(project_path)?;
    let mut stmt = conn
        .prepare(
            "SELECT id FROM expert_team_consultations WHERE session_id=?1 ORDER BY created_at DESC, id DESC",
        )
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([session_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .map(|row| row.map_err(|e| e.to_string()))
        .collect::<AppResult<Vec<_>>>()?;
    ids.iter().map(|id| load_consultation(&conn, id)).collect()
}

fn load_consultation(
    conn: &rusqlite::Connection,
    consultation_id: &str,
) -> AppResult<ExpertTeamConsultation> {
    let row = conn
        .query_row(
            "SELECT id, request_task_id, session_id, user_request, selection_json, cost_level, read_only, token_budget, base_revision, status, synthesis_task_id, result_json, error_json, created_at, confirmed_at, completed_at, updated_at FROM expert_team_consultations WHERE id=?1",
            [consultation_id],
            |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?,
                row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, i64>(6)?, row.get::<_, i64>(7)?, row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?, row.get::<_, Option<String>>(10)?, row.get::<_, Option<String>>(11)?, row.get::<_, Option<String>>(12)?,
                row.get::<_, String>(13)?, row.get::<_, Option<String>>(14)?, row.get::<_, Option<String>>(15)?, row.get::<_, String>(16)?,
            )),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: 专家团申请不存在".to_string())?;
    Ok(ExpertTeamConsultation {
        id: row.0.clone(),
        request_task_id: row.1,
        session_id: row.2,
        user_request: row.3,
        selection: serde_json::from_str(&row.4).map_err(|e| e.to_string())?,
        members: load_members(conn, &row.0)?,
        cost_level: row.5,
        read_only: row.6 != 0,
        token_budget: row.7 as usize,
        base_revision: row.8,
        status: row.9,
        synthesis_task_id: row.10,
        result: row
            .11
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|e| e.to_string())?,
        error: row
            .12
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|e| e.to_string())?,
        application_card_id: format!("{}:application", row.0),
        cost_card_id: format!("{}:cost", row.0),
        created_at: row.13,
        confirmed_at: row.14,
        completed_at: row.15,
        updated_at: row.16,
    })
}

fn load_members(
    conn: &rusqlite::Connection,
    consultation_id: &str,
) -> AppResult<Vec<ExpertTeamMember>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, expert_type, task_id, status, result_json, error_json, created_at, updated_at FROM expert_team_members WHERE consultation_id=?1 ORDER BY created_at, id",
        )
        .map_err(|e| e.to_string())?;
    let members = stmt
        .query_map([consultation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .map(|row| {
            let row = row.map_err(|e| e.to_string())?;
            Ok(ExpertTeamMember {
                id: row.0,
                expert_type: row.1,
                task_id: row.2,
                status: row.3,
                result: row
                    .4
                    .map(|value| serde_json::from_str(&value))
                    .transpose()
                    .map_err(|e| e.to_string())?,
                error: row
                    .5
                    .map(|value| serde_json::from_str(&value))
                    .transpose()
                    .map_err(|e| e.to_string())?,
                created_at: row.6,
                updated_at: row.7,
            })
        })
        .collect();
    members
}

fn load_member_results(conn: &rusqlite::Connection, consultation_id: &str) -> AppResult<Value> {
    let members = load_members(conn, consultation_id)?;
    Ok(Value::Array(
        members
            .into_iter()
            .map(|member| {
                json!({
                    "expertType": member.expert_type,
                    "expertName": expert(&member.expert_type).map(|definition| definition.display_name).unwrap_or("专业 Agent"),
                    "status": member.status,
                    "result": member.result,
                })
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::mock::MockRuntime;
    use crate::context::ObjectRef;
    use crate::database::init_database;
    use std::thread;
    use std::time::{Duration, Instant};

    fn setup() -> (tempfile::TempDir, String, SelectionSnapshot) {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "专家团测试", "series").unwrap();
        let conn = open_database(temp.path()).unwrap();
        conn.execute(
            "INSERT INTO agent_sessions (id, project_id, scope_type, scope_id, title, status, created_at, updated_at) VALUES ('session', ?1, 'project', ?1, '主 Agent', 'active', ?2, ?2)",
            params![project.id, now()],
        ).unwrap();
        let selection = SelectionSnapshot {
            project_id: project.id.clone(),
            center: Some(ObjectRef {
                project_id: project.id.clone(),
                object_type: "project".into(),
                object_id: project.id.clone(),
                field: None,
            }),
            selected: vec![],
            project_revision: 0,
        };
        (temp, project.id, selection)
    }

    fn request_input(selection: SelectionSnapshot) -> RequestExpertTeamInput {
        RequestExpertTeamInput {
            request_id: "consultation".into(),
            session_id: "session".into(),
            message: "这一场的问题在哪里？".into(),
            selection,
            members: vec!["writer".into(), "director".into(), "cinematography".into()],
            token_budget: 8_000,
            provider: None,
            model: None,
        }
    }

    #[test]
    fn request_never_creates_or_starts_member_tasks_before_confirmation() {
        let (temp, _, selection) = setup();
        let consultation = request_consultation(temp.path(), request_input(selection)).unwrap();
        assert_eq!(consultation.status, "awaiting_confirmation");
        assert!(consultation
            .members
            .iter()
            .all(|member| member.task_id.is_none()));
        let runtime = RuntimeState::with_runtime(MockRuntime::new(vec![], Duration::ZERO));
        let error = confirm_consultation(
            temp.path(),
            ConfirmExpertTeamInput {
                consultation_id: consultation.id.clone(),
                confirmed: false,
            },
            None,
            runtime,
            None,
        )
        .unwrap_err();
        assert!(error.contains("CONFIRMATION_REQUIRED"));
        let conn = open_database(temp.path()).unwrap();
        let member_tasks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_tasks WHERE task_type='expert_team_member'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let contexts: i64 = conn
            .query_row("SELECT COUNT(*) FROM context_packages", [], |row| {
                row.get(0)
            })
            .unwrap();
        let cards: i64 = conn.query_row(
            "SELECT COUNT(*) FROM ai_cards WHERE task_id=?1 AND card_type IN ('expert_team','cost') AND status='open'",
            [&consultation.request_task_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!((member_tasks, contexts, cards), (0, 0, 2));
    }

    #[test]
    fn confirmed_team_uses_independent_readonly_contexts_and_main_synthesis() {
        let (temp, _, selection) = setup();
        request_consultation(temp.path(), request_input(selection)).unwrap();
        let response = json!({
            "summary": "独立意见",
            "findings": [{"topic": "节奏", "position": "需要收紧"}],
            "patchProposal": {"items": [{"unsafe": true}]},
            "questions": [],
            "risks": [],
            "consensus": ["节奏需要收紧"],
            "disagreements": [{"topic": "镜头长度", "positions": ["短", "长"]}],
            "recommendations": ["另行建立修改提案"]
        })
        .to_string();
        let runtime =
            RuntimeState::with_runtime(MockRuntime::new(vec![response], Duration::from_millis(5)));
        confirm_consultation(
            temp.path(),
            ConfirmExpertTeamInput {
                consultation_id: "consultation".into(),
                confirmed: true,
            },
            None,
            runtime,
            None,
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        let completed = loop {
            let conn = open_database(temp.path()).unwrap();
            let value = load_consultation(&conn, "consultation").unwrap();
            if matches!(value.status.as_str(), "completed" | "failed" | "stale") {
                break value;
            }
            assert!(Instant::now() < deadline, "专家团未在期限内完成");
            thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(completed.status, "completed");
        assert!(completed.synthesis_task_id.is_some());
        assert_eq!(completed.members.len(), 3);
        assert!(completed
            .members
            .iter()
            .all(|member| member.status == "completed"));
        assert_eq!(completed.result.as_ref().unwrap()["readOnly"], true);
        assert_eq!(
            completed.result.as_ref().unwrap()["patchProposal"],
            Value::Null
        );
        assert!(!completed.result.as_ref().unwrap()["disagreements"]
            .as_array()
            .unwrap()
            .is_empty());

        let conn = open_database(temp.path()).unwrap();
        let contexts: i64 = conn
            .query_row("SELECT COUNT(*) FROM context_packages", [], |row| {
                row.get(0)
            })
            .unwrap();
        let distinct_contexts: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT task_id) FROM context_packages",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let writable: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_tasks WHERE task_type='expert_team_member' AND write_scope_json <> '{\"refs\":[],\"protectedRefs\":[]}'",
            [],
            |row| row.get(0),
        ).unwrap();
        let proposals: i64 = conn
            .query_row("SELECT COUNT(*) FROM patch_proposals", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            (contexts, distinct_contexts, writable, proposals),
            (3, 3, 0, 0)
        );
    }

    #[test]
    fn cancellation_and_stale_confirmation_never_write_project_facts() {
        let (temp, _, selection) = setup();
        request_consultation(temp.path(), request_input(selection.clone())).unwrap();
        let runtime = RuntimeState::with_runtime(MockRuntime::new(
            vec![json!({"summary": "慢任务"}).to_string()],
            Duration::from_millis(200),
        ));
        confirm_consultation(
            temp.path(),
            ConfirmExpertTeamInput {
                consultation_id: "consultation".into(),
                confirmed: true,
            },
            None,
            runtime.clone(),
            None,
        )
        .unwrap();
        let cancelled = cancel_consultation(temp.path(), "consultation", &runtime).unwrap();
        assert_eq!(cancelled.status, "cancelled");
        assert!(cancelled
            .members
            .iter()
            .all(|member| member.status == "cancelled"));

        let (stale_temp, _, stale_selection) = setup();
        request_consultation(stale_temp.path(), request_input(stale_selection)).unwrap();
        open_database(stale_temp.path())
            .unwrap()
            .execute("UPDATE projects SET revision=1", [])
            .unwrap();
        let stale_runtime = RuntimeState::with_runtime(MockRuntime::new(vec![], Duration::ZERO));
        let error = confirm_consultation(
            stale_temp.path(),
            ConfirmExpertTeamInput {
                consultation_id: "consultation".into(),
                confirmed: true,
            },
            None,
            stale_runtime,
            None,
        )
        .unwrap_err();
        assert!(error.contains("REVISION_STALE"));
        let conn = open_database(stale_temp.path()).unwrap();
        let member_tasks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_tasks WHERE task_type='expert_team_member'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(member_tasks, 0);
    }
}
