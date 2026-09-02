use crate::agent_models::{model_choice_for_role, restore_agent_credentials};
use crate::agent_runtime::{
    ensure_agent_core_enabled, RuntimeAttachment, RuntimeEvent, RuntimeEventSink, RuntimeState,
    RuntimeTaskInput, RUNTIME_EVENT_NAME,
};
use crate::app_database::load_feature_flags;
use crate::context::{build_context_with_memories, BuildContextInput, SelectionSnapshot};
use crate::database::{now, open_database, AppResult};
use crate::memory::{active_global_memories, MemoryContextEntry};
use crate::permission::{
    create_card, propose_patch, CreateCardInput, ObjectRef as PermissionObjectRef, PatchItemInput,
    ProposePatchInput, WriteScope,
};
use base64::Engine;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertDefinition {
    pub expert_type: &'static str,
    pub display_name: &'static str,
    pub responsibilities: &'static [&'static str],
    pub default_read: &'static [&'static str],
    pub default_write: &'static [&'static str],
    pub prohibited: &'static [&'static str],
    pub system_instruction: &'static str,
}

const EXPERTS: &[ExpertDefinition] = &[
    ExpertDefinition {
        expert_type: "writer",
        display_name: "编剧 Agent",
        responsibilities: &["情节", "人物动机", "对白", "冲突", "结构", "伏笔", "节奏"],
        default_read: &["当前剧本或场", "相邻场", "角色资产", "项目结构"],
        default_write: &["选中的剧本文本、对白或场字段"],
        prohibited: &["未经申请修改分镜、资产或关键帧"],
        system_instruction: "从人物动机、冲突、对白和叙事结构判断，保持既有项目事实。",
    },
    ExpertDefinition {
        expert_type: "director",
        display_name: "导演 / 分镜 Agent",
        responsibilities: &[
            "剧本拆镜",
            "镜头顺序",
            "揭示顺序",
            "动作拆分",
            "连续性",
            "镜头节奏",
        ],
        default_read: &["当前场剧本", "当前场镜头", "前后场状态", "资产需求"],
        default_write: &["选中镜头或镜头组的导演与分镜字段"],
        prohibited: &["未经申请改写资产或关键帧事实"],
        system_instruction: "从镜头存在价值、信息揭示、主体切换、动作拆分和连续性判断。",
    },
    ExpertDefinition {
        expert_type: "cinematography",
        display_name: "摄影 Agent",
        responsibilities: &["景别", "机位", "拍摄方向", "构图", "运镜", "空间层次"],
        default_read: &["当前镜头", "前后镜头", "场景空间", "正式资产", "叙事目的"],
        default_write: &["选中镜头的摄影字段"],
        prohibited: &["改变剧情、对白或动作结果"],
        system_instruction: "只从摄影语言和空间关系提出建议，不改变剧情、对白与动作结果。",
    },
    ExpertDefinition {
        expert_type: "art",
        display_name: "美术 Agent",
        responsibilities: &["视觉定义", "色彩", "材质", "形态", "资产需求", "资产提示词"],
        default_read: &[
            "资产定义",
            "需求来源镜头",
            "正式资产图片",
            "视觉规则",
            "相关关键帧",
        ],
        default_write: &["选中资产或资产需求字段"],
        prohibited: &["自动把候选图设为正式资产"],
        system_instruction: "从角色、场景、道具的视觉一致性和可生成性判断，不选择正式图片。",
    },
    ExpertDefinition {
        expert_type: "keyframe",
        display_name: "关键帧 Agent",
        responsibilities: &[
            "静态画面",
            "资产组合",
            "人物比例",
            "场景空间",
            "关键帧提示词",
        ],
        default_read: &["当前镜头", "正式资产", "现有关键帧", "场景空间"],
        default_write: &["选中关键帧的描述与提示词字段"],
        prohibited: &["在提示词中改变镜头事实", "自动选择正式关键帧"],
        system_instruction: "把镜头事实落实为静态画面；发现矛盾时提出问题，不在提示词中偷改事实。",
    },
    ExpertDefinition {
        expert_type: "prompt",
        display_name: "提示词 Agent",
        responsibilities: &["视频提示词编译", "模型适配", "复杂度风险", "任务拆分建议"],
        default_read: &["生成任务", "关联镜头", "资产", "关键帧", "目标模型"],
        default_write: &["选中生成任务的提示词字段"],
        prohibited: &["调用视频生成"],
        system_instruction: "编译目标模型提示词并标明来源与风险；绝不调用视频生成。",
    },
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveIntentInput {
    pub message: String,
    pub workspace: Option<String>,
    pub selection: SelectionSnapshot,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedIntent {
    pub task_type: String,
    pub expert_type: Option<String>,
    pub confidence: f32,
    pub reason: String,
    pub clarification_question: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionInput {
    pub request_id: String,
    pub project_id: String,
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub title: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub project_id: String,
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub title: String,
    pub status: String,
    pub runtime_session_id: Option<String>,
    pub session_kind: String,
    pub parent_session_id: Option<String>,
    pub expert_type: Option<String>,
    pub last_active_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageInput {
    pub request_id: String,
    pub session_id: String,
    pub message: String,
    pub workspace: Option<String>,
    #[serde(default = "default_agent_mode")]
    pub mode: String,
    pub selection: SelectionSnapshot,
    pub write_scope: WriteScope,
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
    pub provider: Option<String>,
    pub model: Option<String>,
}

fn default_token_budget() -> usize {
    8_000
}

fn default_agent_mode() -> String {
    "edit".into()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub session_id: String,
    pub task_type: String,
    pub interaction_mode: String,
    pub agent_type: String,
    pub selection: Value,
    pub read_scope: Value,
    pub write_scope: Value,
    pub context_revision: i64,
    pub status: String,
    pub model_provider: Option<String>,
    pub model_name: Option<String>,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub agent_type: Option<String>,
    pub content: String,
    pub structured: Option<Value>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDispatch {
    pub session_id: String,
    pub task_id: String,
    pub route: ResolvedIntent,
    pub runtime_started: bool,
    pub status: String,
}

struct PreparedTask {
    dispatch: AgentDispatch,
    runtime_input: Option<RuntimeTaskInput>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionWorkingMemory {
    discussion_goal: Option<String>,
    recent_messages: Vec<Value>,
    unresolved_cards: Vec<Value>,
    pending_patch: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpertResultDraft {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    findings: Vec<Value>,
    patch_proposal: Option<PatchDraft>,
    #[serde(default)]
    related_impacts: Vec<Value>,
    #[serde(default)]
    permission_requests: Vec<Value>,
    #[serde(default)]
    questions: Vec<String>,
    #[serde(default)]
    risks: Vec<String>,
    #[serde(default)]
    problem_cards: Vec<AnalysisCardDraft>,
    #[serde(default)]
    suggestion_cards: Vec<AnalysisCardDraft>,
    #[serde(default)]
    affected_objects: Vec<PermissionObjectRef>,
    #[serde(default)]
    recommended_review_scope: Vec<String>,
    #[serde(default)]
    deep_analysis_requires_confirmation: bool,
    expert_team_suggestion: Option<ExpertTeamSuggestionDraft>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExpertTeamSuggestionDraft {
    #[serde(default)]
    reason: String,
    #[serde(default)]
    question: String,
    #[serde(default)]
    members: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisCardDraft {
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    related_ref: Option<PermissionObjectRef>,
    #[serde(default)]
    evidence: Vec<Value>,
    #[serde(default)]
    affected_objects: Vec<PermissionObjectRef>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchDraft {
    #[serde(default)]
    title: String,
    #[serde(default)]
    items: Vec<PatchItemInput>,
}

#[tauri::command]
pub fn agent_list_experts(app: tauri::AppHandle) -> AppResult<Vec<ExpertDefinition>> {
    ensure_expert_agents_enabled(&app)?;
    Ok(EXPERTS.to_vec())
}

#[tauri::command]
pub fn agent_resolve_intent(
    app: tauri::AppHandle,
    input: ResolveIntentInput,
) -> AppResult<ResolvedIntent> {
    ensure_expert_agents_enabled(&app)?;
    Ok(resolve_intent(&input))
}

#[tauri::command]
pub fn agent_create_session(
    app: tauri::AppHandle,
    project_path: String,
    input: CreateSessionInput,
) -> AppResult<AgentSession> {
    ensure_expert_agents_enabled(&app)?;
    create_session(Path::new(&project_path), input)
}

#[tauri::command]
pub fn agent_list_sessions(
    app: tauri::AppHandle,
    project_path: String,
) -> AppResult<Vec<AgentSession>> {
    ensure_expert_agents_enabled(&app)?;
    list_sessions(Path::new(&project_path))
}

#[tauri::command]
pub fn agent_close_session(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeState>,
    project_path: String,
    session_id: String,
) -> AppResult<AgentSession> {
    ensure_expert_agents_enabled(&app)?;
    ensure_session_can_close(Path::new(&project_path), &session_id)?;
    runtime.close_session(&session_id)?;
    update_session_status(Path::new(&project_path), &session_id, "closed")
}

#[tauri::command]
pub fn agent_resume_session(
    app: tauri::AppHandle,
    project_path: String,
    session_id: String,
) -> AppResult<AgentSession> {
    ensure_expert_agents_enabled(&app)?;
    update_session_status(Path::new(&project_path), &session_id, "active")
}

#[tauri::command]
pub fn agent_send_message(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeState>,
    project_path: String,
    input: SendMessageInput,
) -> AppResult<AgentDispatch> {
    ensure_expert_agents_enabled(&app)?;
    if input.mode == "change_analysis" {
        ensure_change_analysis_enabled(&app)?;
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    restore_agent_credentials(&app_data_dir, &runtime)?;
    let global_memories = if input.mode == "change_analysis"
        && load_feature_flags(&app_data_dir)?.get("memory") == Some(&true)
    {
        Some(active_global_memories(&app_data_dir)?)
    } else {
        None
    };
    let prepared = prepare_task_for_runtime(
        Path::new(&project_path),
        input,
        global_memories.as_deref(),
        true,
    )?;
    let role = prepared
        .dispatch
        .route
        .expert_type
        .as_deref()
        .unwrap_or("main")
        .to_string();
    let task_id = prepared.dispatch.task_id.clone();
    let Some(mut runtime_input) = prepared.runtime_input else {
        return Ok(prepared.dispatch);
    };
    let choice = model_choice_for_role(&app_data_dir, &role)?;
    runtime_input.provider = runtime_input.provider.or(choice.provider);
    runtime_input.model = runtime_input.model.or(choice.model);
    runtime_input.thinking_level = runtime_input.thinking_level.or(choice.thinking_level);
    open_database(Path::new(&project_path))?
        .execute(
            "UPDATE agent_tasks SET model_provider=?1, model_name=?2 WHERE id=?3",
            params![runtime_input.provider, runtime_input.model, task_id],
        )
        .map_err(|error| error.to_string())?;
    runtime_input.project_path = Some(project_path.clone());
    runtime_input.app_data_dir = Some(app_data_dir.to_string_lossy().into_owned());
    let buffer = Arc::new(Mutex::new(String::new()));
    let sink_buffer = Arc::clone(&buffer);
    let sink_path = PathBuf::from(&project_path);
    let sink_app = app.clone();
    let sink: RuntimeEventSink = Arc::new(move |event| {
        handle_runtime_event(&sink_path, &sink_buffer, &event);
        let _ = sink_app.emit(RUNTIME_EVENT_NAME, event);
    });
    let handle = match runtime.start_task(runtime_input, sink) {
        Ok(handle) => handle,
        Err(error) => {
            mark_task_failed(Path::new(&project_path), &task_id, &error)?;
            return Err(error);
        }
    };
    if let Some(runtime_session_id) = handle.runtime_session_id.as_deref() {
        if let Err(error) = bind_runtime_session(
            Path::new(&project_path),
            &prepared.dispatch.session_id,
            &task_id,
            runtime_session_id,
        ) {
            let _ = runtime.cancel_task(&task_id);
            mark_task_failed(Path::new(&project_path), &task_id, &error)?;
            return Err(error);
        }
    }
    Ok(AgentDispatch {
        runtime_started: true,
        status: "queued".into(),
        ..prepared.dispatch
    })
}

#[tauri::command]
pub fn agent_get_task(
    app: tauri::AppHandle,
    project_path: String,
    task_id: String,
) -> AppResult<AgentTask> {
    ensure_expert_agents_enabled(&app)?;
    let conn = open_database(Path::new(&project_path))?;
    mark_change_analysis_stale(&conn, &task_id)?;
    load_task(&conn, &task_id)
}

#[tauri::command]
pub fn agent_list_messages(
    app: tauri::AppHandle,
    project_path: String,
    session_id: String,
) -> AppResult<Vec<AgentMessage>> {
    ensure_expert_agents_enabled(&app)?;
    let conn = open_database(Path::new(&project_path))?;
    list_messages(&conn, &session_id)
}

pub(crate) fn ensure_expert_agents_enabled(app: &tauri::AppHandle) -> AppResult<()> {
    ensure_agent_core_enabled(app)?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    if load_feature_flags(&app_data_dir)?.get("expert_agents") != Some(&true) {
        return Err("专业 Agent 特性尚未启用".into());
    }
    Ok(())
}

fn ensure_change_analysis_enabled(app: &tauri::AppHandle) -> AppResult<()> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    if load_feature_flags(&app_data_dir)?.get("change_analysis") != Some(&true) {
        return Err("FEATURE_DISABLED: 本轮修改分析尚未启用".into());
    }
    Ok(())
}

pub(crate) fn expert(expert_type: &str) -> Option<&'static ExpertDefinition> {
    EXPERTS
        .iter()
        .find(|expert| expert.expert_type == expert_type)
}

fn resolve_intent(input: &ResolveIntentInput) -> ResolvedIntent {
    let message = input.message.to_lowercase();
    let mut scores = [0_i32; 6];
    score_keywords(
        &message,
        &[
            "这句", "对白", "台词", "动机", "冲突", "剧情", "伏笔", "反转", "编剧",
        ],
        &mut scores[0],
    );
    score_keywords(
        &message,
        &[
            "镜头顺序",
            "信息重复",
            "拆镜",
            "分镜",
            "揭示顺序",
            "连续性",
            "节奏",
            "导演",
        ],
        &mut scores[1],
    );
    score_keywords(
        &message,
        &[
            "构图",
            "景别",
            "机位",
            "拍摄方向",
            "运镜",
            "空间层次",
            "摄影",
        ],
        &mut scores[2],
    );
    score_keywords(
        &message,
        &[
            "背面", "外观", "色彩", "材质", "造型", "资产", "美术", "道具",
        ],
        &mut scores[3],
    );
    score_keywords(
        &message,
        &["关键帧", "人物比例", "起始帧", "中间帧", "结束帧"],
        &mut scores[4],
    );
    score_keywords(
        &message,
        &["seedance", "编译", "目标模型", "视频提示词", "模型适配"],
        &mut scores[5],
    );

    if let Some(center) = input.selection.center.as_ref() {
        match center.object_type.as_str() {
            "asset" | "assetRequirement" => scores[3] += 2,
            "keyframe" => scores[4] += 3,
            "generationTask" => scores[5] += 2,
            "scene" | "script" | "contentUnit" | "storyElement" | "storyElementOccurrence" => {
                scores[0] += 1
            }
            "shot" => {
                if let Some(field) = center.field.as_deref() {
                    match field {
                        "dialogue" | "narrative_purpose" | "new_information" => scores[0] += 3,
                        "sort_order" | "action" | "start_state" | "end_state" => scores[1] += 3,
                        "shot_size" | "camera_height" | "camera_direction" | "composition"
                        | "camera_movement" => scores[2] += 3,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(workspace) = input.workspace.as_deref() {
        match workspace {
            "script" => scores[0] += 1,
            "shots" => scores[1] += 1,
            "assets" => scores[3] += 1,
            "keyframes" => scores[4] += 1,
            "generation" => scores[5] += 1,
            _ => {}
        }
    }
    let max = scores.iter().copied().max().unwrap_or(0);
    let winners = scores
        .iter()
        .enumerate()
        .filter(|(_, score)| **score == max && max >= 2)
        .map(|(index, _)| EXPERTS[index].expert_type)
        .collect::<Vec<_>>();
    if winners.len() != 1 {
        return ResolvedIntent {
            task_type: "clarify".into(),
            expert_type: None,
            confidence: 0.0,
            reason: "请求缺少唯一的专业信号".into(),
            clarification_question: Some(
                "你希望我优先从剧情/分镜、摄影画面，还是美术与提示词方向处理？".into(),
            ),
        };
    }
    ResolvedIntent {
        task_type: task_intent(input, winners[0]),
        expert_type: Some(winners[0].into()),
        confidence: (max as f32 / 6.0).clamp(0.35, 1.0),
        reason: format!(
            "根据当前对象、字段、工作区和请求关键词路由到 {}",
            expert(winners[0]).unwrap().display_name
        ),
        clarification_question: None,
    }
}

fn task_intent(input: &ResolveIntentInput, expert_type: &str) -> String {
    let message = input.message.to_lowercase();
    let center = input.selection.center.as_ref();
    if center.is_some_and(|reference| {
        matches!(reference.object_type.as_str(), "project" | "contentUnit")
    }) && ["规划", "剧集结构", "集结构", "整季", "多季"]
        .iter()
        .any(|keyword| message.contains(keyword))
    {
        return "project_planning".into();
    }
    if center.is_some_and(|reference| {
        matches!(
            reference.object_type.as_str(),
            "storyElement" | "storyElementOccurrence"
        )
    }) {
        return "story_element_analysis".into();
    }
    if let Some(reference) = center {
        if reference.object_type == "shot" {
            return reference
                .field
                .as_deref()
                .map(|field| format!("shot_{field}"))
                .unwrap_or_else(|| "shot_analysis".into());
        }
        return match reference.object_type.as_str() {
            "asset" | "assetRequirement" => "asset_visual_definition".into(),
            "keyframe" => "keyframe_design".into(),
            "generationTask" => "prompt_compilation".into(),
            "scene" | "script" => "scene_writing".into(),
            _ => format!("{expert_type}_analysis"),
        };
    }
    format!("{expert_type}_analysis")
}

fn score_keywords(message: &str, keywords: &[&str], score: &mut i32) {
    *score += keywords
        .iter()
        .filter(|keyword| message.contains(**keyword))
        .count() as i32
        * 2;
}

fn create_session(project_path: &Path, input: CreateSessionInput) -> AppResult<AgentSession> {
    if input.request_id.trim().is_empty() || input.project_id.trim().is_empty() {
        return Err("TOOL_ARGUMENT_INVALID: requestId 和 projectId 不能为空".into());
    }
    let conn = open_database(project_path)?;
    if let Ok(existing) = load_session(&conn, &input.request_id) {
        if existing.project_id != input.project_id {
            return Err("TOOL_ARGUMENT_INVALID: requestId 已被其他项目使用".into());
        }
        return Ok(existing);
    }
    let actual_project_id: String = conn
        .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if actual_project_id != input.project_id {
        return Err("TOOL_ARGUMENT_INVALID: projectId 不属于当前项目".into());
    }
    let timestamp = now();
    conn.execute(
        "INSERT INTO agent_sessions (id, project_id, scope_type, scope_id, title, status, session_kind, session_status, last_active_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'active', 'main', 'active', ?6, ?6, ?6)",
        params![input.request_id, input.project_id, input.scope_type, input.scope_id, input.title, timestamp],
    )
    .map_err(|e| e.to_string())?;
    load_session(&conn, &input.request_id)
}

fn list_sessions(project_path: &Path) -> AppResult<Vec<AgentSession>> {
    let conn = open_database(project_path)?;
    let mut statement = conn
        .prepare(
            "SELECT id FROM agent_sessions
             WHERE project_id=(SELECT id FROM projects LIMIT 1) AND session_kind='main'
             ORDER BY COALESCE(last_active_at, updated_at) DESC, created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    ids.iter().map(|id| load_session(&conn, id)).collect()
}

fn ensure_session_can_close(project_path: &Path, session_id: &str) -> AppResult<()> {
    let conn = open_database(project_path)?;
    let session = load_session(&conn, session_id)?;
    let project_id: String = conn
        .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if session.project_id != project_id || session.session_kind != "main" {
        return Err("TOOL_ARGUMENT_INVALID: AgentSession 不属于当前项目主会话".into());
    }
    let busy: bool = conn
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM agent_tasks
               WHERE session_id=?1 AND status IN ('created','context_building','queued','running')
             ) OR EXISTS(
               SELECT 1 FROM expert_team_consultations
               WHERE session_id=?1 AND status IN ('awaiting_confirmation','running','synthesizing')
             )",
            [session_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if busy {
        return Err("SESSION_BUSY: 当前讨论仍有运行中任务或专家团会诊".into());
    }
    Ok(())
}

fn update_session_status(
    project_path: &Path,
    session_id: &str,
    status: &str,
) -> AppResult<AgentSession> {
    if !matches!(status, "active" | "closed") {
        return Err("TOOL_ARGUMENT_INVALID: AgentSession 状态无效".into());
    }
    let conn = open_database(project_path)?;
    let session = load_session(&conn, session_id)?;
    let project_id: String = conn
        .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if session.project_id != project_id || session.session_kind != "main" {
        return Err("TOOL_ARGUMENT_INVALID: AgentSession 不属于当前项目主会话".into());
    }
    let timestamp = now();
    conn.execute(
        "UPDATE agent_sessions
         SET status=?1, session_status=?1, last_active_at=?2, updated_at=?2
         WHERE id=?3",
        params![status, timestamp, session_id],
    )
    .map_err(|e| e.to_string())?;
    load_session(&conn, session_id)
}

#[cfg(test)]
fn prepare_task(
    project_path: &Path,
    input: SendMessageInput,
    global_memories: Option<&[MemoryContextEntry]>,
) -> AppResult<PreparedTask> {
    prepare_task_for_runtime(project_path, input, global_memories, false)
}

fn prepare_task_for_runtime(
    project_path: &Path,
    input: SendMessageInput,
    global_memories: Option<&[MemoryContextEntry]>,
    pi_sdk_runtime: bool,
) -> AppResult<PreparedTask> {
    if input.request_id.trim().is_empty() || input.message.trim().is_empty() {
        return Err("TOOL_ARGUMENT_INVALID: requestId 和 message 不能为空".into());
    }
    let mut conn = open_database(project_path)?;
    let task_id = format!("{}:task", input.request_id);
    if let Ok(existing) = load_task(&conn, &task_id) {
        let route = route_from_task(&existing);
        return Ok(PreparedTask {
            dispatch: AgentDispatch {
                session_id: existing.session_id,
                task_id: existing.id,
                route,
                runtime_started: false,
                status: existing.status,
            },
            runtime_input: None,
        });
    }
    if !matches!(
        input.mode.as_str(),
        "discussion" | "suggestion" | "edit" | "change_analysis"
    ) {
        return Err("TOOL_ARGUMENT_INVALID: Agent mode 无效".into());
    }
    let is_change_analysis = input.mode == "change_analysis";
    let route = if is_change_analysis {
        ResolvedIntent {
            task_type: "change_analysis".into(),
            expert_type: Some("main".into()),
            confidence: 1.0,
            reason: "用户主动触发 ChangeSet 分析".into(),
            clarification_question: None,
        }
    } else if pi_sdk_runtime {
        ResolvedIntent {
            task_type: "main_agent".into(),
            expert_type: Some("main".into()),
            confidence: 1.0,
            reason: "由 MainAgentSession 自主读取事实并决定是否调用专业 Agent".into(),
            clarification_question: None,
        }
    } else {
        resolve_intent(&ResolveIntentInput {
            message: input.message.clone(),
            workspace: input.workspace.clone(),
            selection: input.selection.clone(),
        })
    };
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (project_id, revision): (String, i64) = tx
        .query_row("SELECT id, revision FROM projects LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| e.to_string())?;
    if input.selection.project_id != project_id || input.selection.project_revision != revision {
        return Err("REVISION_STALE: 当前选区不属于项目或已过期".into());
    }
    if is_change_analysis {
        if !input.write_scope.refs.is_empty() || !input.write_scope.protected_refs.is_empty() {
            return Err("TOOL_ARGUMENT_INVALID: 本轮修改分析必须保持只读".into());
        }
        let center = input
            .selection
            .center
            .as_ref()
            .filter(|reference| reference.object_type == "changeSet")
            .ok_or_else(|| {
                "TOOL_ARGUMENT_INVALID: 本轮修改分析必须以 ChangeSet 为中心".to_string()
            })?;
        let valid: bool = tx
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM change_sets
                   WHERE id=?1 AND project_id=?2 AND source_type='user' AND status<>'undone'
                     AND EXISTS(SELECT 1 FROM changes WHERE change_set_id=change_sets.id)
                 )",
                params![center.object_id, project_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !valid {
            return Err("OBJECT_NOT_FOUND: 可分析的用户 ChangeSet 不存在或没有修改".into());
        }
    }
    let (session_project, runtime_session_id): (String, Option<String>) = tx
        .query_row(
            "SELECT project_id, runtime_session_id FROM agent_sessions WHERE id=?1 AND status='active' AND session_status='active'",
            [&input.session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: AgentSession 不存在或已关闭".to_string())?;
    if session_project != project_id {
        return Err("TOOL_ARGUMENT_INVALID: AgentSession 不属于当前项目".into());
    }
    let timestamp = now();
    tx.execute(
        "INSERT INTO agent_messages (id, session_id, role, agent_type, content, created_at) VALUES (?1, ?2, 'user', 'main', ?3, ?4)",
        params![input.request_id, input.session_id, input.message, timestamp],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO agent_tasks (id, session_id, task_type, interaction_mode, agent_type, selection_json, read_scope_json, write_scope_json, context_revision, base_revision, status, model_provider, model_name, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, 'context_building', ?10, ?11, ?12)",
        params![task_id, input.session_id, route.task_type, input.mode, route.expert_type.as_deref().unwrap_or("main"), serde_json::to_string(&input.selection).map_err(|e| e.to_string())?, serde_json::to_string(&input.selection.selected).map_err(|e| e.to_string())?, serde_json::to_string(&input.write_scope).map_err(|e| e.to_string())?, revision, input.provider, input.model, timestamp],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_sessions SET last_active_at=?1, updated_at=?1 WHERE id=?2",
        params![timestamp, input.session_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;

    if route.expert_type.is_none() {
        let result = json!({
            "summary": "需要先明确处理方向",
            "findings": [],
            "patchProposal": null,
            "relatedImpacts": [],
            "permissionRequests": [],
            "questions": [route.clarification_question.clone().unwrap_or_default()],
            "risks": []
        });
        finish_without_runtime(
            project_path,
            &task_id,
            &input.session_id,
            &result,
            "waiting_for_user",
        )?;
        return Ok(PreparedTask {
            dispatch: AgentDispatch {
                session_id: input.session_id,
                task_id,
                route,
                runtime_started: false,
                status: "waiting_for_user".into(),
            },
            runtime_input: None,
        });
    }

    let expert_type = route.expert_type.as_deref().unwrap();
    let package = if pi_sdk_runtime && !is_change_analysis {
        None
    } else {
        Some(
            match build_context_with_memories(
                &mut conn,
                BuildContextInput {
                    task_id: task_id.clone(),
                    selection: input.selection.clone(),
                    task_intent: route.task_type.clone(),
                    expert_type: expert_type.into(),
                    token_budget: input.token_budget,
                },
                global_memories,
            ) {
                Ok(package) => package,
                Err(error) => {
                    mark_task_failed(project_path, &task_id, &error)?;
                    return Err(error);
                }
            },
        )
    };
    let (provider, model) = if is_change_analysis || expert_type == "main" {
        (input.provider, input.model)
    } else {
        expert_model_override(&conn, &project_id, expert_type, input.provider, input.model)?
    };
    conn.execute(
        "UPDATE agent_tasks SET status='queued', model_provider=?1, model_name=?2 WHERE id=?3",
        params![provider, model, task_id],
    )
    .map_err(|e| e.to_string())?;
    let working_memory = if pi_sdk_runtime {
        None
    } else {
        Some(session_working_memory(
            &conn,
            &input.session_id,
            &input.request_id,
        )?)
    };
    let attachments = if is_change_analysis || (pi_sdk_runtime && expert_type == "main") {
        Vec::new()
    } else {
        visual_attachments(project_path, &conn, expert_type, &input.selection)?
    };
    let mut prompt = if is_change_analysis {
        build_change_analysis_prompt(
            &input.message,
            package.as_ref().ok_or("ContextPackage 未构建")?,
        )?
    } else if pi_sdk_runtime {
        build_tool_driven_prompt(
            expert_type,
            &input.mode,
            &input.message,
            &input.selection,
            &input.write_scope,
        )?
    } else {
        build_expert_prompt(
            expert_type,
            &input.mode,
            &input.message,
            &input.write_scope,
            package.as_ref().ok_or("ContextPackage 未构建")?,
            working_memory.as_ref(),
        )?
    };
    if !is_change_analysis {
        prompt.push_str(&crate::creative_settings::selection_prompt(&conn, &input.selection)?);
    }
    if !attachments.is_empty() {
        let names = attachments
            .iter()
            .map(|attachment| attachment.name.as_str())
            .collect::<Vec<_>>();
        prompt.push_str(&format!(
            "\n视觉附件（由 Runtime 单独传输，不是可访问路径）：{}。请结合实际画面判断；不得臆造附件中不可见的信息。",
            serde_json::to_string(&names).map_err(|e| e.to_string())?
        ));
    }
    Ok(PreparedTask {
        dispatch: AgentDispatch {
            session_id: input.session_id.clone(),
            task_id: task_id.clone(),
            route,
            runtime_started: false,
            status: "queued".into(),
        },
        runtime_input: Some(RuntimeTaskInput {
            task_id: Some(task_id),
            session_id: Some(input.session_id),
            runtime_session_id,
            project_path: None,
            app_data_dir: None,
            prompt,
            provider,
            model,
            system_prompt: None,
            thinking_level: None,
            allowed_tools: None,
            allow_call_expert: pi_sdk_runtime.then_some(true),
            result_tool_kind: pi_sdk_runtime.then_some("agent".into()),
            attachments,
        }),
    })
}

pub(crate) fn visual_attachments(
    project_path: &Path,
    conn: &rusqlite::Connection,
    expert_type: &str,
    selection: &SelectionSnapshot,
) -> AppResult<Vec<RuntimeAttachment>> {
    if !matches!(
        expert_type,
        "main" | "art" | "keyframe" | "cinematography" | "prompt"
    ) {
        return Ok(Vec::new());
    }
    let Some(center) = selection.center.as_ref() else {
        return Ok(Vec::new());
    };
    let mut paths = match center.object_type.as_str() {
        "asset" => query_visual_paths(
            conn,
            "SELECT file_path, COALESCE(NULLIF(label,''), file_path) FROM asset_media WHERE asset_id=?1 AND media_type='image' ORDER BY is_primary DESC, sort_order, id",
            &center.object_id,
        )?,
        "assetRequirement" => query_visual_paths(
            conn,
            "SELECT am.file_path, COALESCE(NULLIF(am.label,''), am.file_path) FROM asset_media am LEFT JOIN asset_media_requirements amr ON amr.asset_media_id=am.id WHERE am.media_type='image' AND (amr.asset_requirement_id=?1 OR am.asset_id=(SELECT asset_id FROM asset_requirements WHERE id=?1)) ORDER BY am.is_primary DESC, am.sort_order, am.id",
            &center.object_id,
        )?,
        "shot" => shot_visual_paths(conn, &center.object_id)?,
        "keyframe" => {
            let shot_id: String = conn
                .query_row(
                    "SELECT shot_id FROM keyframes WHERE id=?1",
                    [&center.object_id],
                    |row| row.get(0),
                )
                .map_err(|_| "OBJECT_NOT_FOUND: 当前关键帧不存在".to_string())?;
            shot_visual_paths(conn, &shot_id)?
        }
        "generationTask" => query_visual_paths(
            conn,
            "SELECT am.file_path, COALESCE(NULLIF(am.label,''), am.file_path) FROM generation_task_shots gts JOIN shot_assets sa ON sa.shot_id=gts.shot_id JOIN asset_media am ON am.asset_id=sa.asset_id WHERE gts.generation_task_id=?1 AND am.media_type='image' ORDER BY gts.sort_order, am.is_primary DESC, am.sort_order, am.id",
            &center.object_id,
        )?,
        _ => Vec::new(),
    };
    let mut seen = HashSet::new();
    paths.retain(|(path, _)| seen.insert(path.clone()));
    paths.truncate(8);
    encode_visual_attachments(project_path, paths)
}

fn shot_visual_paths(
    conn: &rusqlite::Connection,
    shot_id: &str,
) -> AppResult<Vec<(String, String)>> {
    let mut paths = query_visual_paths(
        conn,
        "SELECT am.file_path, COALESCE(NULLIF(am.label,''), a.name) FROM shot_assets sa JOIN assets a ON a.id=sa.asset_id JOIN asset_media am ON am.asset_id=a.id WHERE sa.shot_id=?1 AND am.media_type='image' ORDER BY am.is_primary DESC, am.sort_order, am.id",
        shot_id,
    )?;
    paths.extend(query_visual_paths(
        conn,
        "SELECT file_path, COALESCE(NULLIF(description,''), type || ' keyframe') FROM keyframes WHERE shot_id=?1 AND file_path IS NOT NULL AND file_path<>'' ORDER BY sort_order, id",
        shot_id,
    )?);
    Ok(paths)
}

fn query_visual_paths(
    conn: &rusqlite::Connection,
    sql: &str,
    object_id: &str,
) -> AppResult<Vec<(String, String)>> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([object_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn encode_visual_attachments(
    project_path: &Path,
    paths: Vec<(String, String)>,
) -> AppResult<Vec<RuntimeAttachment>> {
    let project_root = project_path
        .canonicalize()
        .map_err(|e| format!("读取项目目录失败：{e}"))?;
    paths
        .into_iter()
        .map(|(relative, label)| {
            let absolute = project_path
                .join(&relative)
                .canonicalize()
                .map_err(|e| format!("读取正式视觉附件失败（{relative}）：{e}"))?;
            if !absolute.starts_with(&project_root) {
                return Err("TOOL_ARGUMENT_INVALID: 正式视觉附件必须位于当前项目内".into());
            }
            let bytes = fs::read(&absolute)
                .map_err(|e| format!("读取正式视觉附件失败（{relative}）：{e}"))?;
            let mime_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                "image/png"
            } else if bytes.starts_with(b"\xff\xd8\xff") {
                "image/jpeg"
            } else if bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
                "image/webp"
            } else {
                return Err(format!(
                    "TOOL_ARGUMENT_INVALID: 正式视觉附件不是有效的 PNG、JPEG 或 WebP（{relative}）"
                ));
            };
            if bytes.len() > 20 * 1024 * 1024 {
                return Err(format!(
                    "TOOL_ARGUMENT_INVALID: 正式视觉附件超过 20MB（{relative}）"
                ));
            }
            Ok(RuntimeAttachment {
                name: if label.trim().is_empty() {
                    relative
                } else {
                    label
                },
                mime_type: mime_type.into(),
                data: base64::engine::general_purpose::STANDARD.encode(bytes),
            })
        })
        .collect()
}

fn build_tool_driven_prompt(
    expert_type: &str,
    mode: &str,
    user_message: &str,
    selection: &SelectionSnapshot,
    write_scope: &WriteScope,
) -> AppResult<String> {
    let (display_name, system_instruction) = if expert_type == "main" {
        (
            "主 Agent",
            "先自行读取项目事实；只有需要专业判断时才调用 call_expert，并在专家返回后综合，不得把专业选择预先交给 Rust 关键词路由。确认问题横跨至少两个专业方向时，只建议 expertTeamSuggestion，由用户确认专家与高成本后启动。",
        )
    } else {
        let definition = expert(expert_type).ok_or_else(|| "未知专业 Agent".to_string())?;
        (definition.display_name, definition.system_instruction)
    };
    let mode_rule = if mode == "edit" {
        "如需修改，只能在 submit_agent_result 中返回 patchProposal；工作台会继续执行权限和 stale 校验。"
    } else {
        "这是只读讨论，submit_agent_result 的 patchProposal 必须为 null。"
    };
    Ok(format!(
        "当前请求由真实 Pi MainAgentSession 处理，当前角色是{}。{}\n{}\n项目事实没有预先塞进本轮 Prompt。先调用 get_selection，再按问题调用 read_object、read_parent、read_children、read_neighbors、read_scene、read_shot_context、read_asset、read_generation_task、read_story_structure、search_project、read_active_memories 或 read_change_set；信息不足时继续调用工具，禁止猜测。需要专业判断时由你自行调用 call_expert(writer/director/cinematography/art/keyframe/prompt)，专业 Agent 会在独立 Pi AgentSession 中自行读取事实。工具返回的 projectRevision 才是当前事实版本。不得请求 SQL、文件路径、Shell、PowerShell 或任意文件访问。\n当前用户请求：{}\n启动选区引用：{}\n当前 WriteScope：{}\n结束前必须调用一次 submit_agent_result，键为 summary、findings、patchProposal、relatedImpacts、permissionRequests、questions、risks、expertTeamSuggestion。只有核对事实后确认问题横跨至少两个专业方向时，expertTeamSuggestion 才包含 reason、question、members（2–6 个 writer/director/cinematography/art/keyframe/prompt）；否则为 null。不得自行启动专家团。patchProposal.items 每项包含 objectType、objectId、fieldName、oldValue、newValue、reason；没有修改则为 null。不要依赖自由文本 JSON。",
        display_name,
        system_instruction,
        mode_rule,
        user_message,
        serde_json::to_string(selection).map_err(|e| e.to_string())?,
        serde_json::to_string(write_scope).map_err(|e| e.to_string())?,
    ))
}

pub(crate) fn build_expert_prompt(
    expert_type: &str,
    mode: &str,
    user_message: &str,
    write_scope: &WriteScope,
    package: &crate::context::ContextPackage,
    working_memory: Option<&SessionWorkingMemory>,
) -> AppResult<String> {
    let definition = expert(expert_type).ok_or_else(|| "未知专业 Agent".to_string())?;
    let mode_rule = if mode == "edit" {
        "编辑模式：确有必要时可返回结构化 patchProposal。"
    } else {
        "只读模式：patchProposal 必须为 null，不得申请写入。"
    };
    Ok(format!(
        "你是{}。{}\n{}\n禁止：{}。不得输出 SQL、文件操作或直接写入命令。只能依据 SessionWorkingMemory 与 ContextPackage。source=memory 的条目只是偏好或已确认共识，不是事实；它与项目事实冲突时必须以事实为准并明确指出冲突，绝不能用记忆覆盖事实。\nSessionWorkingMemory：{}\n当前用户请求：{}\n当前 WriteScope：{}\nContextPackage：{}\n只返回一个 JSON 对象，键必须为 summary、findings、patchProposal、relatedImpacts、permissionRequests、questions、risks。patchProposal.items 每项必须包含 objectType、objectId、fieldName、oldValue、newValue、reason；没有修改则 patchProposal 为 null。",
        definition.display_name,
        definition.system_instruction,
        mode_rule,
        definition.prohibited.join("；"),
        serde_json::to_string(working_memory.unwrap_or(&SessionWorkingMemory::default()))
            .map_err(|e| e.to_string())?,
        user_message,
        serde_json::to_string(write_scope).map_err(|e| e.to_string())?,
        serde_json::to_string(package).map_err(|e| e.to_string())?,
    ))
}

fn session_working_memory(
    conn: &rusqlite::Connection,
    session_id: &str,
    current_message_id: &str,
) -> AppResult<SessionWorkingMemory> {
    let mut stmt = conn
        .prepare(
            "SELECT role, content, structured_json FROM agent_messages
             WHERE session_id=?1 AND id<>?2 ORDER BY rowid DESC LIMIT 8",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id, current_message_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut recent_messages = rows
        .map(|row| {
            let (role, content, structured) = row.map_err(|e| e.to_string())?;
            Ok(json!({
                "role": role,
                "content": content,
                "structured": structured.and_then(|value| serde_json::from_str::<Value>(&value).ok()),
            }))
        })
        .collect::<AppResult<Vec<_>>>()?;
    recent_messages.reverse();
    drop(stmt);

    let discussion_goal = recent_messages.iter().rev().find_map(|message| {
        (message["role"] == "user")
            .then(|| message["content"].as_str().map(str::to_string))
            .flatten()
    });
    let mut card_stmt = conn
        .prepare(
            "SELECT card.card_type, card.title, card.body FROM ai_cards card
             JOIN agent_tasks task ON task.id=card.task_id
             WHERE task.session_id=?1 AND card.status='open'
             ORDER BY card.rowid DESC LIMIT 8",
        )
        .map_err(|e| e.to_string())?;
    let unresolved_cards = card_stmt
        .query_map([session_id], |row| {
            Ok(json!({
                "type": row.get::<_, String>(0)?,
                "title": row.get::<_, String>(1)?,
                "body": row.get::<_, String>(2)?,
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let pending_patch = conn
        .query_row(
            "SELECT proposal.id, proposal.title, proposal.status FROM patch_proposals proposal
             JOIN agent_tasks task ON task.id=proposal.task_id
             WHERE task.session_id=?1 AND proposal.status IN ('draft','pending','approved')
             ORDER BY proposal.rowid DESC LIMIT 1",
            [session_id],
            |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "title": row.get::<_, String>(1)?,
                    "status": row.get::<_, String>(2)?,
                }))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(SessionWorkingMemory {
        discussion_goal,
        recent_messages,
        unresolved_cards,
        pending_patch,
    })
}

fn build_change_analysis_prompt(
    user_message: &str,
    package: &crate::context::ContextPackage,
) -> AppResult<String> {
    Ok(format!(
        "你是创作工作台主 Agent，正在执行用户主动触发的‘分析本轮修改’。只依据 ContextPackage：中心 ChangeSet 含每个字段的 oldValue/newValue，affected/parent/neighbor/relation 是受影响对象、同场景或同剧集上下文及直接正式关系。source=memory 的条目只是偏好或已确认共识，不是事实；与项目事实冲突时事实始终优先，并明确指出冲突。\n\
         分析剧本动机与对白一致性、镜头时长/连续性/关键帧/生成任务、资产引用与跨阶段直接影响。默认只分析直接关系和同一剧集；若必须跨剧集深挖，只设置 deepAnalysisRequiresConfirmation=true，不自行扩大范围。\n\
         这是只读任务：不得修改 sync_status，不得返回写入建议，patchProposal 必须为 null。问题与建议必须给出具体差异证据；没有证据就不要生成卡片。\n\
         用户请求：{}\nContextPackage：{}\n\
         结束前调用一次 submit_agent_result，参数键为 summary、findings、patchProposal、relatedImpacts、permissionRequests、questions、risks、problemCards、suggestionCards、affectedObjects、recommendedReviewScope、deepAnalysisRequiresConfirmation；不要依赖自由文本 JSON。\n\
         problemCards/suggestionCards 每项包含 title、body、relatedRef（可为 null）、evidence、affectedObjects；对象引用包含 projectId、objectType、objectId、field（可省略）。affectedObjects 是去重后的直接受影响对象。",
        user_message,
        serde_json::to_string(package).map_err(|e| e.to_string())?,
    ))
}

pub(crate) fn expert_model_override(
    conn: &rusqlite::Connection,
    project_id: &str,
    expert_type: &str,
    provider: Option<String>,
    model: Option<String>,
) -> AppResult<(Option<String>, Option<String>)> {
    let override_row: Option<(i64, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT enabled, model_provider, model_name FROM project_expert_overrides WHERE project_id=?1 AND expert_type=?2",
            params![project_id, expert_type],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((enabled, override_provider, override_model)) = override_row {
        if enabled == 0 {
            return Err(format!(
                "{} 已在当前项目禁用",
                expert(expert_type).unwrap().display_name
            ));
        }
        Ok((override_provider.or(provider), override_model.or(model)))
    } else {
        Ok((provider, model))
    }
}

fn handle_runtime_event(project_path: &Path, buffer: &Mutex<String>, event: &RuntimeEvent) {
    match event {
        RuntimeEvent::TaskStarted { task_id } => {
            if let Ok(conn) = open_database(project_path) {
                let _ = conn.execute(
                    "UPDATE agent_tasks SET status='running', started_at=?1 WHERE id=?2",
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
        RuntimeEvent::StructuredResult { result, .. } => {
            if let Ok(mut text) = buffer.lock() {
                *text = result.to_string();
            }
        }
        RuntimeEvent::TaskCompleted { task_id } => {
            let text = buffer.lock().map(|value| value.clone()).unwrap_or_default();
            if let Err(error) = complete_agent_task(project_path, task_id, &text) {
                let _ = mark_task_failed(project_path, task_id, &error);
            }
        }
        RuntimeEvent::TaskFailed { task_id, error } => {
            let _ = mark_task_failed(project_path, task_id, error);
        }
        RuntimeEvent::TaskCancelled { task_id } => {
            if let Ok(conn) = open_database(project_path) {
                let _ = conn.execute(
                    "UPDATE agent_tasks SET status='cancelled', completed_at=?1 WHERE id=?2",
                    params![now(), task_id],
                );
            }
        }
        RuntimeEvent::ToolCallRequested { .. } | RuntimeEvent::ToolCallCompleted { .. } => {}
    }
}

fn complete_agent_task(project_path: &Path, task_id: &str, raw: &str) -> AppResult<Value> {
    let conn = open_database(project_path)?;
    let (session_id, revision, task_type, interaction_mode): (String, i64, String, String) = conn
        .query_row(
            "SELECT session_id, context_revision, task_type, interaction_mode FROM agent_tasks WHERE id=?1",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: AgentTask 不存在".to_string())?;
    let current_revision: i64 = conn
        .query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    drop(conn);
    let is_change_analysis = task_type == "change_analysis";
    let stale = current_revision != revision;
    let mut draft =
        serde_json::from_str::<ExpertResultDraft>(raw).unwrap_or_else(|_| ExpertResultDraft {
            summary: raw.trim().to_string(),
            ..ExpertResultDraft::default()
        });
    if draft.summary.trim().is_empty() {
        draft.summary = "专业 Agent 已完成分析".into();
    }
    let proposed_patch = draft
        .patch_proposal
        .take()
        .filter(|patch| !patch.items.is_empty());
    if interaction_mode != "edit" && proposed_patch.is_some() {
        draft.risks.push("只读模式已忽略模型返回的修改提案".into());
    }
    if stale {
        draft.risks.push(format!(
            "分析期间项目已从 revision {revision} 变为 {current_revision}，结果已过期"
        ));
    }
    let patch_value =
        if let Some(patch) = proposed_patch.filter(|_| interaction_mode == "edit" && !stale) {
            let proposal = propose_patch(
                project_path,
                ProposePatchInput {
                    request_id: format!("{task_id}:proposal"),
                    task_id: task_id.into(),
                    base_revision: revision,
                    title: if patch.title.trim().is_empty() {
                        "专业 Agent 修改提案".into()
                    } else {
                        patch.title
                    },
                    items: patch.items,
                },
            )?;
            Some(serde_json::to_value(proposal).map_err(|e| e.to_string())?)
        } else {
            None
        };
    let card_ids = if is_change_analysis {
        materialize_analysis_cards(
            project_path,
            task_id,
            revision,
            current_revision,
            &draft,
            stale,
        )?
    } else {
        Vec::new()
    };
    let expert_team_suggestion = draft
        .expert_team_suggestion
        .take()
        .and_then(normalize_expert_team_suggestion);
    let status = if stale {
        "stale"
    } else if patch_value.is_some()
        || !draft.questions.is_empty()
        || !card_ids.is_empty()
        || expert_team_suggestion.is_some()
    {
        "waiting_for_user"
    } else {
        "completed"
    };
    let result = json!({
        "summary": draft.summary,
        "findings": draft.findings,
        "patchProposal": patch_value,
        "relatedImpacts": draft.related_impacts,
        "permissionRequests": draft.permission_requests,
        "questions": draft.questions,
        "risks": draft.risks,
        "problemCards": draft.problem_cards,
        "suggestionCards": draft.suggestion_cards,
        "affectedObjects": draft.affected_objects,
        "recommendedReviewScope": draft.recommended_review_scope,
        "deepAnalysisRequiresConfirmation": draft.deep_analysis_requires_confirmation,
        "expertTeamSuggestion": expert_team_suggestion,
        "cardIds": card_ids,
        "baseRevision": revision,
        "currentRevision": current_revision,
        "stale": stale,
    });
    finish_without_runtime(project_path, task_id, &session_id, &result, status)?;
    Ok(result)
}

fn normalize_expert_team_suggestion(
    suggestion: ExpertTeamSuggestionDraft,
) -> Option<ExpertTeamSuggestionDraft> {
    let mut unique = HashSet::new();
    let valid = !suggestion.reason.trim().is_empty()
        && !suggestion.question.trim().is_empty()
        && (2..=6).contains(&suggestion.members.len())
        && suggestion
            .members
            .iter()
            .all(|member| expert(member).is_some() && unique.insert(member.clone()));
    valid.then_some(suggestion)
}

fn materialize_analysis_cards(
    project_path: &Path,
    task_id: &str,
    base_revision: i64,
    current_revision: i64,
    draft: &ExpertResultDraft,
    stale: bool,
) -> AppResult<Vec<String>> {
    let mut ids = Vec::new();
    for (card_type, cards) in [
        ("problem", draft.problem_cards.as_slice()),
        ("suggestion", draft.suggestion_cards.as_slice()),
    ] {
        for (index, card) in cards.iter().enumerate() {
            let created = create_card(
                project_path,
                CreateCardInput {
                    request_id: format!("{task_id}:{card_type}:{index}"),
                    task_id: task_id.into(),
                    card_type: card_type.into(),
                    related_ref: card.related_ref.clone(),
                    title: if card.title.trim().is_empty() {
                        if card_type == "problem" {
                            "发现潜在问题".into()
                        } else {
                            "建议复查".into()
                        }
                    } else {
                        card.title.clone()
                    },
                    body: card.body.clone(),
                    options: json!({
                        "evidence": card.evidence,
                        "affectedObjects": card.affected_objects,
                        "recommendedReviewScope": draft.recommended_review_scope,
                        "deepAnalysisRequiresConfirmation": draft.deep_analysis_requires_confirmation,
                        "baseRevision": base_revision,
                    }),
                },
            )?;
            ids.push(created.id);
        }
    }
    if stale {
        let created = create_card(
            project_path,
            CreateCardInput {
                request_id: format!("{task_id}:stale"),
                task_id: task_id.into(),
                card_type: "stale".into(),
                related_ref: None,
                title: "分析结果已过期".into(),
                body: format!(
                    "分析基于 revision {base_revision}，当前项目是 revision {current_revision}，请重新分析本轮修改。"
                ),
                options: json!({
                    "baseRevision": base_revision,
                    "currentRevision": current_revision,
                }),
            },
        )?;
        ids.push(created.id);
    }
    Ok(ids)
}

fn finish_without_runtime(
    project_path: &Path,
    task_id: &str,
    session_id: &str,
    result: &Value,
    status: &str,
) -> AppResult<()> {
    let mut conn = open_database(project_path)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let timestamp = now();
    tx.execute(
        "UPDATE agent_tasks SET status=?1, result_json=?2, completed_at=?3 WHERE id=?4",
        params![status, result.to_string(), timestamp, task_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT OR REPLACE INTO agent_messages (id, session_id, role, agent_type, content, structured_json, created_at) VALUES (?1, ?2, 'assistant', 'main', ?3, ?4, ?5)",
        params![format!("{task_id}:assistant"), session_id, result["summary"].as_str().unwrap_or_default(), result.to_string(), timestamp],
    ).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_sessions SET updated_at=?1 WHERE id=?2",
        params![timestamp, session_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn mark_task_failed(project_path: &Path, task_id: &str, error: &str) -> AppResult<()> {
    let conn = open_database(project_path)?;
    conn.execute(
        "UPDATE agent_tasks SET status='failed', error_json=?1, completed_at=?2 WHERE id=?3",
        params![
            json!({"message": error, "retryable": true, "projectFactsChanged": false}).to_string(),
            now(),
            task_id
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn mark_change_analysis_stale(conn: &rusqlite::Connection, task_id: &str) -> AppResult<()> {
    let row: Option<(String, i64, String, Option<String>)> = conn
        .query_row(
            "SELECT task_type, context_revision, status, result_json FROM agent_tasks WHERE id=?1",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((task_type, base_revision, status, result_raw)) = row else {
        return Ok(());
    };
    if task_type != "change_analysis"
        || status == "stale"
        || !matches!(status.as_str(), "completed" | "waiting_for_user")
    {
        return Ok(());
    }
    let current_revision: i64 = conn
        .query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    if current_revision == base_revision {
        return Ok(());
    }
    let mut result = result_raw
        .as_deref()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .unwrap_or_else(|| json!({}));
    if let Some(object) = result.as_object_mut() {
        object.insert("stale".into(), Value::Bool(true));
        object.insert("baseRevision".into(), json!(base_revision));
        object.insert("currentRevision".into(), json!(current_revision));
        let risks = object
            .entry("risks")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(risks) = risks.as_array_mut() {
            risks.push(json!(format!(
                "项目已从 revision {base_revision} 变为 {current_revision}，请重新分析"
            )));
        }
    }
    conn.execute(
        "UPDATE agent_tasks SET status='stale', result_json=?1 WHERE id=?2",
        params![result.to_string(), task_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn bind_runtime_session(
    project_path: &Path,
    session_id: &str,
    task_id: &str,
    runtime_session_id: &str,
) -> AppResult<()> {
    let mut conn = open_database(project_path)?;
    let current: Option<String> = conn
        .query_row(
            "SELECT runtime_session_id FROM agent_sessions WHERE id=?1",
            [session_id],
            |row| row.get(0),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: AgentSession 不存在".to_string())?;
    if current
        .as_deref()
        .is_some_and(|value| value != runtime_session_id)
    {
        return Err("SESSION_MAPPING_CONFLICT: Pi Session 映射不一致".into());
    }
    let timestamp = now();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_sessions
         SET runtime_session_id=?1, last_active_at=?2, updated_at=?2
         WHERE id=?3",
        params![runtime_session_id, timestamp, session_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE agent_tasks SET pi_session_id=?1 WHERE id=?2 AND session_id=?3",
        params![runtime_session_id, task_id, session_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

fn load_session(conn: &rusqlite::Connection, session_id: &str) -> AppResult<AgentSession> {
    conn.query_row(
        "SELECT id, project_id, scope_type, scope_id, title, session_status,
                runtime_session_id, session_kind, parent_session_id, expert_type,
                COALESCE(last_active_at, updated_at), created_at, updated_at
         FROM agent_sessions WHERE id=?1",
        [session_id],
        |row| {
            Ok(AgentSession {
                id: row.get(0)?,
                project_id: row.get(1)?,
                scope_type: row.get(2)?,
                scope_id: row.get(3)?,
                title: row.get(4)?,
                status: row.get(5)?,
                runtime_session_id: row.get(6)?,
                session_kind: row.get(7)?,
                parent_session_id: row.get(8)?,
                expert_type: row.get(9)?,
                last_active_at: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        },
    )
    .map_err(|_| "OBJECT_NOT_FOUND: AgentSession 不存在".into())
}

fn load_task(conn: &rusqlite::Connection, task_id: &str) -> AppResult<AgentTask> {
    conn.query_row(
        "SELECT id, session_id, task_type, interaction_mode, agent_type, selection_json, read_scope_json, write_scope_json, context_revision, status, model_provider, model_name, result_json, error_json, created_at, started_at, completed_at FROM agent_tasks WHERE id=?1",
        [task_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, String>(7)?, row.get::<_, i64>(8)?, row.get::<_, String>(9)?, row.get::<_, Option<String>>(10)?, row.get::<_, Option<String>>(11)?, row.get::<_, Option<String>>(12)?, row.get::<_, Option<String>>(13)?, row.get::<_, String>(14)?, row.get::<_, Option<String>>(15)?, row.get::<_, Option<String>>(16)?)),
    ).map_err(|_| "OBJECT_NOT_FOUND: AgentTask 不存在".to_string()).and_then(|row| Ok(AgentTask {
        id: row.0, session_id: row.1, task_type: row.2, interaction_mode: row.3, agent_type: row.4,
        selection: serde_json::from_str(&row.5).map_err(|e| e.to_string())?,
        read_scope: serde_json::from_str(&row.6).map_err(|e| e.to_string())?,
        write_scope: serde_json::from_str(&row.7).map_err(|e| e.to_string())?,
        context_revision: row.8, status: row.9, model_provider: row.10, model_name: row.11,
        result: row.12.map(|value| serde_json::from_str(&value)).transpose().map_err(|e| e.to_string())?,
        error: row.13.map(|value| serde_json::from_str(&value)).transpose().map_err(|e| e.to_string())?,
        created_at: row.14, started_at: row.15, completed_at: row.16,
    }))
}

fn list_messages(conn: &rusqlite::Connection, session_id: &str) -> AppResult<Vec<AgentMessage>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, role, agent_type, content, structured_json, created_at FROM agent_messages WHERE session_id=?1 ORDER BY created_at, id",
        )
        .map_err(|e| e.to_string())?;
    let messages = stmt
        .query_map([session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .map(|row| {
            let row = row.map_err(|e| e.to_string())?;
            Ok(AgentMessage {
                id: row.0,
                session_id: row.1,
                role: row.2,
                agent_type: row.3,
                content: row.4,
                structured: row
                    .5
                    .map(|value| serde_json::from_str(&value))
                    .transpose()
                    .map_err(|e| e.to_string())?,
                created_at: row.6,
            })
        })
        .collect();
    messages
}

fn route_from_task(task: &AgentTask) -> ResolvedIntent {
    ResolvedIntent {
        task_type: task.task_type.clone(),
        expert_type: (task.agent_type != "main").then(|| task.agent_type.clone()),
        confidence: 1.0,
        reason: "幂等请求返回已存在任务".into(),
        clarification_question: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ObjectRef;
    use crate::database::init_database;
    use crate::mutation::{execute_mutations_in_transaction, MutationRequest};
    use serde_json::Map;

    fn selection(
        project_id: &str,
        object_type: &str,
        object_id: &str,
        field: Option<&str>,
    ) -> SelectionSnapshot {
        let center = ObjectRef {
            project_id: project_id.into(),
            object_type: object_type.into(),
            object_id: object_id.into(),
            field: field.map(str::to_string),
        };
        SelectionSnapshot {
            project_id: project_id.into(),
            center: Some(center.clone()),
            selected: vec![center],
            project_revision: 0,
        }
    }

    #[test]
    fn registry_contains_six_bounded_experts() {
        assert_eq!(EXPERTS.len(), 6);
        assert_eq!(
            EXPERTS
                .iter()
                .map(|expert| expert.expert_type)
                .collect::<Vec<_>>(),
            vec![
                "writer",
                "director",
                "cinematography",
                "art",
                "keyframe",
                "prompt"
            ]
        );
        assert!(EXPERTS.iter().all(|expert| !expert.prohibited.is_empty()));
    }

    #[test]
    fn main_agent_accepts_only_bounded_cross_discipline_team_suggestions() {
        let valid = normalize_expert_team_suggestion(ExpertTeamSuggestionDraft {
            reason: "镜头节奏、构图和对白动机互相影响".into(),
            question: "请编剧、导演和摄影分别判断镜头04的改法".into(),
            members: vec!["writer".into(), "director".into(), "cinematography".into()],
        })
        .unwrap();
        assert_eq!(valid.members.len(), 3);
        assert!(normalize_expert_team_suggestion(ExpertTeamSuggestionDraft {
            reason: "重复角色".into(),
            question: "复查".into(),
            members: vec!["writer".into(), "writer".into()],
        })
        .is_none());
        assert!(normalize_expert_team_suggestion(ExpertTeamSuggestionDraft {
            reason: "越界角色".into(),
            question: "复查".into(),
            members: vec!["writer".into(), "video_generator".into()],
        })
        .is_none());
    }

    #[test]
    fn routes_acceptance_examples_and_clarifies_ambiguous_request() {
        let cases = [
            ("这句不像她", "shot", "dialogue", "writer"),
            ("这三个镜头信息重复", "shot", "title", "director"),
            ("构图太平", "shot", "composition", "cinematography"),
            ("奶牛猫背面怎么设计", "asset", "description", "art"),
            (
                "这个镜头关键帧人物比例不对",
                "keyframe",
                "description",
                "keyframe",
            ),
            (
                "编译成 Seedance 提示词",
                "generationTask",
                "prompt",
                "prompt",
            ),
        ];
        for (message, object_type, field, expected) in cases {
            let route = resolve_intent(&ResolveIntentInput {
                message: message.into(),
                workspace: None,
                selection: selection("project", object_type, "object", Some(field)),
            });
            assert_eq!(route.expert_type.as_deref(), Some(expected), "{message}");
        }
        let ambiguous = resolve_intent(&ResolveIntentInput {
            message: "这个镜头整体不好".into(),
            workspace: Some("shots".into()),
            selection: selection("project", "shot", "shot", None),
        });
        assert_eq!(ambiguous.task_type, "clarify");
        assert!(ambiguous.clarification_question.is_some());
    }

    #[test]
    fn persists_main_session_mapping_and_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "会话测试", "short").unwrap();
        let session = create_session(
            temp.path(),
            CreateSessionInput {
                request_id: "main-session".into(),
                project_id: project.id,
                scope_type: "project".into(),
                scope_id: None,
                title: "第一次讨论".into(),
            },
        )
        .unwrap();
        assert_eq!(session.status, "active");
        assert_eq!(session.session_kind, "main");
        assert_eq!(list_sessions(temp.path()).unwrap().len(), 1);

        let conn = open_database(temp.path()).unwrap();
        conn.execute(
            "INSERT INTO agent_tasks
             (id, session_id, task_type, interaction_mode, agent_type, selection_json,
              read_scope_json, write_scope_json, context_revision, status, created_at)
             VALUES ('task', ?1, 'discussion', 'discussion', 'main', '{}', '[]', '{}', 0, 'completed', ?2)",
            params![session.id, now()],
        )
        .unwrap();
        drop(conn);

        bind_runtime_session(temp.path(), &session.id, "task", "pi-session").unwrap();
        let mapped = list_sessions(temp.path()).unwrap().remove(0);
        assert_eq!(mapped.runtime_session_id.as_deref(), Some("pi-session"));
        assert!(bind_runtime_session(temp.path(), &session.id, "task", "other-session").is_err());

        let closed = update_session_status(temp.path(), &session.id, "closed").unwrap();
        assert_eq!(closed.status, "closed");
        let resumed = update_session_status(temp.path(), &session.id, "active").unwrap();
        assert_eq!(resumed.status, "active");
        assert_eq!(resumed.runtime_session_id.as_deref(), Some("pi-session"));
    }

    #[test]
    fn refuses_to_close_a_busy_main_session() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "忙碌会话", "short").unwrap();
        let session = create_session(
            temp.path(),
            CreateSessionInput {
                request_id: "busy-session".into(),
                project_id: project.id,
                scope_type: "project".into(),
                scope_id: None,
                title: "运行中讨论".into(),
            },
        )
        .unwrap();
        let conn = open_database(temp.path()).unwrap();
        conn.execute(
            "INSERT INTO agent_tasks
             (id, session_id, task_type, interaction_mode, agent_type, selection_json,
              read_scope_json, write_scope_json, context_revision, status, created_at)
             VALUES ('running-task', ?1, 'discussion', 'discussion', 'main', '{}', '[]', '{}', 0, 'running', ?2)",
            params![session.id, now()],
        )
        .unwrap();
        drop(conn);
        assert!(ensure_session_can_close(temp.path(), &session.id)
            .unwrap_err()
            .starts_with("SESSION_BUSY"));
    }

    #[test]
    fn pi_sdk_main_turn_uses_minimal_tool_driven_startup_prompt() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "工具化上下文", "short").unwrap();
        let conn = open_database(temp.path()).unwrap();
        let timestamp = now();
        conn.execute("INSERT INTO content_units (id, project_id, type, name, sort_order, created_at, updated_at) VALUES ('unit', ?1, 'short', '正片', 0, ?2, ?2)", params![project.id, timestamp]).unwrap();
        conn.execute("INSERT INTO scripts (id, content_unit_id, title, created_at, updated_at) VALUES ('script', 'unit', '正片', ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO scenes (id, script_id, title, sort_order, created_at, updated_at) VALUES ('scene', 'script', '场01', 0, ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO shots (id, scene_id, sort_order, title, composition, created_at, updated_at) VALUES ('shot', 'scene', 0, '镜头04', '不应进入启动包的旧构图事实', ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("UPDATE content_units SET creative_settings_json=?1 WHERE id='unit'", [json!({"contentType":"documentary","style":{"dimensions":{"visual":"东方水墨画风"}}}).to_string()]).unwrap();
        drop(conn);
        let session = create_session(
            temp.path(),
            CreateSessionInput {
                request_id: "tool-session".into(),
                project_id: project.id.clone(),
                scope_type: "project".into(),
                scope_id: Some(project.id.clone()),
                title: "工具讨论".into(),
            },
        )
        .unwrap();
        let prepared = prepare_task_for_runtime(
            temp.path(),
            SendMessageInput {
                request_id: "tool-message".into(),
                session_id: session.id,
                message: "这个镜头为什么不够有压迫感？".into(),
                workspace: Some("shots".into()),
                mode: "discussion".into(),
                selection: selection(&project.id, "shot", "shot", Some("composition")),
                write_scope: WriteScope::default(),
                token_budget: 8_000,
                provider: None,
                model: None,
            },
            None,
            true,
        )
        .unwrap();
        assert_eq!(prepared.dispatch.route.expert_type.as_deref(), Some("main"));
        assert_eq!(prepared.dispatch.route.task_type, "main_agent");
        let runtime_input = prepared.runtime_input.unwrap();
        let prompt = runtime_input.prompt;
        assert!(prompt.contains("read_shot_context"));
        assert!(prompt.contains("call_expert"));
        assert!(prompt.contains("submit_agent_result"));
        assert!(!prompt.contains("不应进入启动包的旧构图事实"));
        assert!(prompt.contains("东方水墨画风") && prompt.contains("documentary"));
        assert!(runtime_input.system_prompt.is_none());
        assert!(runtime_input.provider.is_none() && runtime_input.model.is_none());
        assert!(prompt.len() < 3_000);
        assert!(runtime_input.attachments.is_empty());
        assert_eq!(runtime_input.result_tool_kind.as_deref(), Some("agent"));
        let conn = open_database(temp.path()).unwrap();
        let visible: String = conn.query_row("SELECT content FROM agent_messages WHERE role='user'", [], |row| row.get(0)).unwrap();
        assert_eq!(visible, "这个镜头为什么不够有压迫感？");
        assert_eq!(
            conn.query_row(
                "SELECT agent_type FROM agent_tasks WHERE id='tool-message:task'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "main"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM context_packages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn prepares_single_expert_context_and_materializes_patch_proposal() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "Agent 测试", "short").unwrap();
        let conn = open_database(temp.path()).unwrap();
        let timestamp = now();
        conn.execute("INSERT INTO content_units (id, project_id, type, name, sort_order, created_at, updated_at) VALUES ('unit', ?1, 'short', '正片', 0, ?2, ?2)", params![project.id, timestamp]).unwrap();
        conn.execute("INSERT INTO scripts (id, content_unit_id, title, created_at, updated_at) VALUES ('script', 'unit', '正片', ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO scenes (id, script_id, title, sort_order, created_at, updated_at) VALUES ('scene', 'script', '场01', 0, ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO shots (id, scene_id, sort_order, title, composition, created_at, updated_at) VALUES ('shot', 'scene', 0, '镜头04', '旧构图', ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO assets (id, project_id, type, name, created_at, updated_at) VALUES ('asset', ?1, 'character', '女主角', ?2, ?2)", params![project.id, timestamp]).unwrap();
        conn.execute("INSERT INTO asset_media (id, asset_id, file_path, label, is_primary, created_at, updated_at) VALUES ('media', 'asset', 'assets/hero.png', '女主角正面设定', 1, ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO shot_assets (id, shot_id, asset_id, role, created_at, updated_at) VALUES ('shot-asset', 'shot', 'asset', 'subject', ?1, ?1)", [&timestamp]).unwrap();
        fs::create_dir_all(temp.path().join("assets")).unwrap();
        let png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
        fs::write(temp.path().join("assets/hero.png"), png).unwrap();
        drop(conn);
        let session = create_session(
            temp.path(),
            CreateSessionInput {
                request_id: "session".into(),
                project_id: project.id.clone(),
                scope_type: "shot".into(),
                scope_id: Some("shot".into()),
                title: "镜头协作".into(),
            },
        )
        .unwrap();
        let selected = selection(&project.id, "shot", "shot", Some("composition"));
        let scope = WriteScope {
            refs: selected
                .selected
                .clone()
                .into_iter()
                .map(|reference| crate::permission::ObjectRef {
                    project_id: reference.project_id,
                    object_type: reference.object_type,
                    object_id: reference.object_id,
                    field: reference.field,
                })
                .collect(),
            protected_refs: vec![],
        };
        let prepared = prepare_task(
            temp.path(),
            SendMessageInput {
                request_id: "message".into(),
                session_id: session.id.clone(),
                message: "构图太平，请增强空间层次".into(),
                workspace: Some("shots".into()),
                mode: "edit".into(),
                selection: selected.clone(),
                write_scope: scope.clone(),
                token_budget: 800,
                provider: None,
                model: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(
            prepared.dispatch.route.expert_type.as_deref(),
            Some("cinematography")
        );
        let runtime_input = prepared.runtime_input.unwrap();
        assert!(runtime_input.prompt.contains("ContextPackage"));
        assert!(runtime_input.prompt.contains("patchProposal"));
        assert!(runtime_input.prompt.contains("女主角正面设定"));
        assert_eq!(runtime_input.attachments.len(), 1);
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM context_packages WHERE task_id=?1",
                [&prepared.dispatch.task_id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        drop(conn);
        let result = complete_agent_task(temp.path(), &prepared.dispatch.task_id, r#"{"summary":"三个方案：方案一增加前景，方案二人物后移，方案三改用高机位","findings":[],"patchProposal":{"title":"构图优化","items":[{"objectType":"shot","objectId":"shot","fieldName":"composition","oldValue":"旧构图","newValue":"前景遮挡、中景主体、后景纵深","reason":"增强空间层次"}]},"relatedImpacts":[],"permissionRequests":[],"questions":[],"risks":[]}"#).unwrap();
        assert_eq!(result["patchProposal"]["status"], "pending");
        assert_eq!(
            result["patchProposal"]["items"][0]["permissionState"],
            "allowed"
        );
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT status FROM agent_tasks WHERE id=?1",
                [&prepared.dispatch.task_id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "waiting_for_user"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM patch_proposals WHERE task_id=?1",
                [&prepared.dispatch.task_id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM agent_messages WHERE session_id='session'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        drop(conn);

        let readonly = prepare_task(
            temp.path(),
            SendMessageInput {
                request_id: "readonly-message".into(),
                session_id: session.id.clone(),
                message: "用第二个，但把人物再往后放一点".into(),
                workspace: Some("shots".into()),
                mode: "suggestion".into(),
                selection: selected.clone(),
                write_scope: scope.clone(),
                token_budget: 800,
                provider: None,
                model: None,
            },
            None,
        )
        .unwrap();
        let readonly_prompt = readonly.runtime_input.as_ref().unwrap().prompt.clone();
        assert!(readonly_prompt.contains("方案二人物后移"));
        assert!(readonly_prompt.contains("用第二个，但把人物再往后放一点"));
        assert_eq!(readonly.dispatch.route.task_type, "shot_composition");
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT interaction_mode FROM agent_tasks WHERE id=?1",
                [&readonly.dispatch.task_id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "suggestion"
        );
        drop(conn);
        let readonly_result = complete_agent_task(
            temp.path(),
            &readonly.dispatch.task_id,
            r#"{"summary":"已按方案二理解：人物后移并保留前景层次","patchProposal":{"title":"不应落库","items":[{"objectType":"shot","objectId":"shot","fieldName":"composition","oldValue":"旧构图","newValue":"错误修改","reason":"模型越权"}]},"risks":[]}"#,
        )
        .unwrap();
        assert!(readonly_result["patchProposal"].is_null());
        assert_eq!(
            readonly_result["risks"][0],
            "只读模式已忽略模型返回的修改提案"
        );
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM patch_proposals WHERE task_id=?1",
                [&readonly.dispatch.task_id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(conn);

        let third = prepare_task(
            temp.path(),
            SendMessageInput {
                request_id: "third-message".into(),
                session_id: session.id,
                message: "继续微调，人物稍向左".into(),
                workspace: Some("shots".into()),
                mode: "discussion".into(),
                selection: selected,
                write_scope: WriteScope::default(),
                token_budget: 1_600,
                provider: None,
                model: None,
            },
            None,
        )
        .unwrap();
        let third_prompt = third.runtime_input.unwrap().prompt;
        assert!(third_prompt.contains("方案二人物后移"));
        assert!(third_prompt.contains("人物后移并保留前景层次"));
        assert!(third_prompt.contains("继续微调，人物稍向左"));
        open_database(temp.path())
            .unwrap()
            .execute("UPDATE projects SET revision=revision+1", [])
            .unwrap();
        let stale = complete_agent_task(
            temp.path(),
            &third.dispatch.task_id,
            r#"{"summary":"基于旧构图的继续建议","findings":[],"patchProposal":null,"relatedImpacts":[],"permissionRequests":[],"questions":[],"risks":[]}"#,
        )
        .unwrap();
        assert_eq!(stale["stale"], true);
        assert_eq!(
            open_database(temp.path())
                .unwrap()
                .query_row(
                    "SELECT status FROM agent_tasks WHERE id=?1",
                    [&third.dispatch.task_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "stale"
        );
    }

    #[test]
    fn analyzes_change_set_only_on_explicit_readonly_task_and_marks_stale() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "Change Analysis", "short").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        let timestamp = now();
        conn.execute("INSERT INTO content_units (id, project_id, type, name, sort_order, created_at, updated_at) VALUES ('unit', ?1, 'short', '正片', 0, ?2, ?2)", params![project.id, timestamp]).unwrap();
        conn.execute("INSERT INTO scripts (id, content_unit_id, title, created_at, updated_at) VALUES ('script', 'unit', '正片', ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO scenes (id, script_id, title, sort_order, created_at, updated_at) VALUES ('scene', 'script', '场01', 0, ?1, ?1)", [&timestamp]).unwrap();
        conn.execute("INSERT INTO shots (id, scene_id, sort_order, title, duration, dialogue, camera_movement, created_at, updated_at) VALUES ('shot', 'scene', 0, '镜头04', 3, '较长对白', '', ?1, ?1)", [&timestamp]).unwrap();
        let tx = conn.transaction().unwrap();
        let mutation = execute_mutations_in_transaction(
            &tx,
            vec![MutationRequest {
                action: "patch".into(),
                entity_type: "shot".into(),
                object_id: Some("shot".into()),
                values: Map::from_iter([("duration".into(), json!(1.0))]),
                change_set_id: None,
                change_set_name: Some("本轮修改".into()),
                source_type: None,
                source_id: None,
            }],
            None,
            None,
            None,
            None,
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM agent_tasks", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(conn);
        let session = create_session(
            temp.path(),
            CreateSessionInput {
                request_id: "session".into(),
                project_id: project.id.clone(),
                scope_type: "project".into(),
                scope_id: Some(project.id.clone()),
                title: "主 Agent".into(),
            },
        )
        .unwrap();
        let mut analysis_selection =
            selection(&project.id, "changeSet", &mutation.change_set_id, None);
        analysis_selection.project_revision = mutation.revision;
        let prepared = prepare_task(
            temp.path(),
            SendMessageInput {
                request_id: "analyze".into(),
                session_id: session.id,
                message: "分析本轮修改".into(),
                workspace: Some("shots".into()),
                mode: "change_analysis".into(),
                selection: analysis_selection,
                write_scope: WriteScope::default(),
                token_budget: 2_000,
                provider: None,
                model: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(prepared.dispatch.route.task_type, "change_analysis");
        assert_eq!(prepared.dispatch.route.expert_type.as_deref(), Some("main"));
        let prompt = prepared.runtime_input.unwrap().prompt;
        assert!(prompt.contains("oldValue"));
        assert!(prompt.contains("不得修改 sync_status"));

        let raw = format!(
            r#"{{"summary":"镜头时长变化需要复查","findings":[],"patchProposal":null,"relatedImpacts":[],"permissionRequests":[],"questions":[],"risks":[],"problemCards":[{{"title":"对白容量不足","body":"1 秒可能无法容纳现有对白","relatedRef":{{"projectId":"{}","objectType":"shot","objectId":"shot","field":"duration"}},"evidence":["duration 3 → 1"],"affectedObjects":[]}}],"suggestionCards":[{{"title":"复查相邻镜头连续性","body":"确认动作衔接","relatedRef":null,"evidence":[],"affectedObjects":[]}}],"affectedObjects":[{{"projectId":"{}","objectType":"shot","objectId":"shot"}}],"recommendedReviewScope":["当前场相邻镜头"],"deepAnalysisRequiresConfirmation":false}}"#,
            project.id, project.id
        );
        let result = complete_agent_task(temp.path(), &prepared.dispatch.task_id, &raw).unwrap();
        assert_eq!(result["stale"], false);
        assert_eq!(result["affectedObjects"][0]["objectId"], "shot");
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM ai_cards WHERE task_id=?1 AND card_type IN ('problem', 'suggestion')",
                [&prepared.dispatch.task_id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM patch_proposals WHERE task_id=?1",
                [&prepared.dispatch.task_id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        conn.execute("UPDATE projects SET revision=revision+1", [])
            .unwrap();
        mark_change_analysis_stale(&conn, &prepared.dispatch.task_id).unwrap();
        let task = load_task(&conn, &prepared.dispatch.task_id).unwrap();
        assert_eq!(task.status, "stale");
        assert_eq!(task.result.unwrap()["stale"], true);
    }
}
