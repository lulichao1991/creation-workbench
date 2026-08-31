use crate::app_database::{load_feature_flags, open_app_database};
use crate::database::{now, open_database, AppResult};
use crate::mutation::{execute_mutations_in_transaction, MutationRequest};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::Path;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfile {
    pub key: String,
    pub display_name: String,
    pub provider: String,
    pub prompt_format: String,
    pub max_duration_hint: Option<f64>,
    pub max_shots_hint: Option<i64>,
    pub image_reference_rules: String,
    pub supports_start_end_frame: bool,
    pub recommended_constraints: Vec<String>,
    pub prohibited_patterns: Vec<String>,
    pub version: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveModelProfileInput {
    pub key: String,
    pub display_name: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default = "default_prompt_format")]
    pub prompt_format: String,
    pub max_duration_hint: Option<f64>,
    pub max_shots_hint: Option<i64>,
    #[serde(default)]
    pub image_reference_rules: String,
    #[serde(default)]
    pub supports_start_end_frame: bool,
    #[serde(default)]
    pub recommended_constraints: Vec<String>,
    #[serde(default)]
    pub prohibited_patterns: Vec<String>,
    pub version: String,
}

fn default_prompt_format() -> String {
    "plain_text".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplate {
    pub id: String,
    pub scope: String,
    pub project_id: Option<String>,
    pub model_profile_key: String,
    pub name: String,
    pub version: String,
    pub template_body: String,
    pub conditional_rules: Value,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePromptTemplateInput {
    pub id: String,
    pub scope: String,
    pub project_id: Option<String>,
    pub model_profile_key: String,
    pub name: String,
    pub version: String,
    pub template_body: String,
    #[serde(default = "empty_object")]
    pub conditional_rules: Value,
    #[serde(default = "default_true")]
    pub active: bool,
}

fn empty_object() -> Value {
    json!({})
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilePromptInput {
    pub request_id: String,
    pub generation_task_id: String,
    pub model_profile_key: String,
    pub template_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetCurrentPromptInput {
    pub compilation_id: String,
    pub prompt: String,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptWarning {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMapEntry {
    pub start: usize,
    pub end: usize,
    pub source_type: String,
    pub source_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCompilation {
    pub id: String,
    pub generation_task_id: String,
    pub model_profile_key: String,
    pub model_profile_version: String,
    pub template_id: String,
    pub template_version: String,
    pub source_revision: i64,
    pub compiled_prompt: String,
    pub user_override: Option<String>,
    pub current_prompt: Option<String>,
    pub source_map: Vec<SourceMapEntry>,
    pub warnings: Vec<PromptWarning>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug)]
struct ShotFact {
    id: String,
    title: String,
    duration: f64,
    shot_size: String,
    composition: String,
    camera_movement: String,
    subjects: String,
    action: String,
    dialogue: String,
    environment: String,
    start_state: String,
    end_state: String,
    assets: Vec<(String, String, String, bool)>,
    keyframes: Vec<(String, String, String)>,
}

#[derive(Debug)]
struct Segment {
    text: String,
    source_type: String,
    source_id: String,
    label: String,
}

fn ensure_enabled(app: &tauri::AppHandle) -> AppResult<std::path::PathBuf> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    if load_feature_flags(&app_data_dir)?.get("prompt_compiler") == Some(&true) {
        Ok(app_data_dir)
    } else {
        Err("提示词编译器尚未启用".into())
    }
}

fn validate_nonempty(values: &[(&str, &str)]) -> AppResult<()> {
    if let Some((label, _)) = values.iter().find(|(_, value)| value.trim().is_empty()) {
        return Err(format!("TOOL_ARGUMENT_INVALID: {label}不能为空"));
    }
    Ok(())
}

fn load_profile(conn: &Connection, key: &str) -> AppResult<ModelProfile> {
    conn.query_row(
        "SELECT key,display_name,provider,prompt_format,max_duration_hint,max_shots_hint,image_reference_rules,supports_start_end_frame,recommended_constraints_json,prohibited_patterns_json,version,created_at,updated_at FROM model_profiles WHERE key=?1",
        [key],
        |row| {
            Ok(ModelProfile {
                key: row.get(0)?, display_name: row.get(1)?, provider: row.get(2)?, prompt_format: row.get(3)?,
                max_duration_hint: row.get(4)?, max_shots_hint: row.get(5)?, image_reference_rules: row.get(6)?,
                supports_start_end_frame: row.get::<_, i64>(7)? != 0,
                recommended_constraints: serde_json::from_str(&row.get::<_, String>(8)?).unwrap_or_default(),
                prohibited_patterns: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
                version: row.get(10)?, created_at: row.get(11)?, updated_at: row.get(12)?,
            })
        },
    )
    .map_err(|_| "OBJECT_NOT_FOUND: 模型档案不存在".to_string())
}

fn load_template(conn: &Connection, id: &str) -> AppResult<PromptTemplate> {
    conn.query_row(
        "SELECT id,scope,project_id,model_profile_key,name,version,template_body,conditional_rules_json,active,created_at,updated_at FROM prompt_templates WHERE id=?1",
        [id],
        |row| {
            Ok(PromptTemplate {
                id: row.get(0)?, scope: row.get(1)?, project_id: row.get(2)?, model_profile_key: row.get(3)?,
                name: row.get(4)?, version: row.get(5)?, template_body: row.get(6)?,
                conditional_rules: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_else(|_| json!({})),
                active: row.get::<_, i64>(8)? != 0, created_at: row.get(9)?, updated_at: row.get(10)?,
            })
        },
    )
    .map_err(|_| "OBJECT_NOT_FOUND: 提示词模板不存在".to_string())
}

fn save_profile(app_data_dir: &Path, input: SaveModelProfileInput) -> AppResult<ModelProfile> {
    validate_nonempty(&[
        ("模型档案 Key", &input.key),
        ("模型档案名称", &input.display_name),
        ("版本", &input.version),
    ])?;
    if input.max_duration_hint.is_some_and(|value| value <= 0.0)
        || input.max_shots_hint.is_some_and(|value| value <= 0)
    {
        return Err("TOOL_ARGUMENT_INVALID: 时长和镜头数提示必须大于 0".into());
    }
    let conn = open_app_database(app_data_dir)?;
    let existing: Option<String> = conn
        .query_row(
            "SELECT created_at FROM model_profiles WHERE key=?1",
            [&input.key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let timestamp = now();
    conn.execute(
        "INSERT INTO model_profiles (key,display_name,provider,prompt_format,max_duration_hint,max_shots_hint,image_reference_rules,supports_start_end_frame,recommended_constraints_json,prohibited_patterns_json,version,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) ON CONFLICT(key) DO UPDATE SET display_name=excluded.display_name,provider=excluded.provider,prompt_format=excluded.prompt_format,max_duration_hint=excluded.max_duration_hint,max_shots_hint=excluded.max_shots_hint,image_reference_rules=excluded.image_reference_rules,supports_start_end_frame=excluded.supports_start_end_frame,recommended_constraints_json=excluded.recommended_constraints_json,prohibited_patterns_json=excluded.prohibited_patterns_json,version=excluded.version,updated_at=excluded.updated_at",
        params![input.key,input.display_name,input.provider,input.prompt_format,input.max_duration_hint,input.max_shots_hint,input.image_reference_rules,i64::from(input.supports_start_end_frame),json!(input.recommended_constraints).to_string(),json!(input.prohibited_patterns).to_string(),input.version,existing.unwrap_or_else(||timestamp.clone()),timestamp],
    ).map_err(|e| format!("保存模型档案失败：{e}"))?;
    load_profile(&conn, &input.key)
}

fn save_template(app_data_dir: &Path, input: SavePromptTemplateInput) -> AppResult<PromptTemplate> {
    validate_nonempty(&[
        ("模板 ID", &input.id),
        ("模型档案 Key", &input.model_profile_key),
        ("模板名称", &input.name),
        ("模板版本", &input.version),
    ])?;
    if !["global", "project"].contains(&input.scope.as_str())
        || (input.scope == "project"
            && input
                .project_id
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty())
    {
        return Err("TOOL_ARGUMENT_INVALID: 模板作用域无效".into());
    }
    if !input.template_body.contains("{{shots}}") {
        return Err("TOOL_ARGUMENT_INVALID: 模板必须包含 {{shots}}".into());
    }
    let conn = open_app_database(app_data_dir)?;
    load_profile(&conn, &input.model_profile_key)?;
    let existing: Option<String> = conn
        .query_row(
            "SELECT created_at FROM prompt_templates WHERE id=?1",
            [&input.id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let timestamp = now();
    conn.execute(
        "INSERT INTO prompt_templates (id,scope,project_id,model_profile_key,name,version,template_body,conditional_rules_json,active,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(id) DO UPDATE SET scope=excluded.scope,project_id=excluded.project_id,model_profile_key=excluded.model_profile_key,name=excluded.name,version=excluded.version,template_body=excluded.template_body,conditional_rules_json=excluded.conditional_rules_json,active=excluded.active,updated_at=excluded.updated_at",
        params![input.id,input.scope,input.project_id,input.model_profile_key,input.name,input.version,input.template_body,input.conditional_rules.to_string(),i64::from(input.active),existing.unwrap_or_else(||timestamp.clone()),timestamp],
    ).map_err(|e|format!("保存提示词模板失败：{e}"))?;
    load_template(&conn, &input.id)
}

fn list_profiles(app_data_dir: &Path) -> AppResult<Vec<ModelProfile>> {
    let conn = open_app_database(app_data_dir)?;
    let mut stmt = conn
        .prepare("SELECT key FROM model_profiles ORDER BY display_name,key")
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    ids.iter().map(|id| load_profile(&conn, id)).collect()
}

fn list_templates(app_data_dir: &Path, project_id: Option<&str>) -> AppResult<Vec<PromptTemplate>> {
    let conn = open_app_database(app_data_dir)?;
    let mut stmt = conn.prepare("SELECT id FROM prompt_templates WHERE active=1 AND (scope='global' OR (scope='project' AND project_id=?1)) ORDER BY name,id").map_err(|e|e.to_string())?;
    let ids = stmt
        .query_map([project_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    ids.iter().map(|id| load_template(&conn, id)).collect()
}

fn load_shots(conn: &Connection, task_id: &str) -> AppResult<Vec<ShotFact>> {
    let mut stmt = conn.prepare(
        "SELECT s.id,s.title,s.duration,s.shot_size,s.composition,s.camera_movement,s.subjects,s.action,s.dialogue,s.environment,s.start_state,s.end_state FROM generation_task_shots g JOIN shots s ON s.id=g.shot_id WHERE g.generation_task_id=?1 ORDER BY g.sort_order,s.id"
    ).map_err(|e|e.to_string())?;
    let base = stmt
        .query_map([task_id], |row| {
            Ok(ShotFact {
                id: row.get(0)?,
                title: row.get(1)?,
                duration: row.get(2)?,
                shot_size: row.get(3)?,
                composition: row.get(4)?,
                camera_movement: row.get(5)?,
                subjects: row.get(6)?,
                action: row.get(7)?,
                dialogue: row.get(8)?,
                environment: row.get(9)?,
                start_state: row.get(10)?,
                end_state: row.get(11)?,
                assets: vec![],
                keyframes: vec![],
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    base.into_iter().map(|mut shot| {
        let mut assets = conn.prepare("SELECT a.id,a.name,sa.role,EXISTS(SELECT 1 FROM asset_media am WHERE am.asset_id=a.id) FROM shot_assets sa JOIN assets a ON a.id=sa.asset_id WHERE sa.shot_id=?1 ORDER BY sa.role,a.name,a.id").map_err(|e|e.to_string())?;
        shot.assets = assets.query_map([&shot.id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get::<_,i64>(3)? != 0))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        let mut frames = conn.prepare("SELECT id,type,file_path FROM keyframes WHERE shot_id=?1 AND file_path IS NOT NULL AND status='ready' ORDER BY sort_order,id").map_err(|e|e.to_string())?;
        shot.keyframes = frames.query_map([&shot.id], |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        Ok(shot)
    }).collect()
}

fn join_nonempty(items: &[(&str, &str)]) -> String {
    items
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(label, value)| format!("{label}: {}", value.trim()))
        .collect::<Vec<_>>()
        .join("；")
}

fn render_segments(segments: &[Segment]) -> (String, Vec<SourceMapEntry>) {
    let mut text = String::new();
    let mut map = vec![];
    for segment in segments {
        if !text.is_empty() {
            text.push('\n');
        }
        let start = text.len();
        text.push_str(&segment.text);
        map.push(SourceMapEntry {
            start,
            end: text.len(),
            source_type: segment.source_type.clone(),
            source_id: segment.source_id.clone(),
            label: segment.label.clone(),
        });
    }
    (text, map)
}

fn render_template(
    template: &str,
    values: &[(&str, String, Vec<SourceMapEntry>)],
) -> (String, Vec<SourceMapEntry>) {
    let mut output = String::new();
    let mut source_map = vec![];
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let token_rest = &rest[start + 2..];
        let Some(end) = token_rest.find("}}") else {
            output.push_str(&rest[start..]);
            return (output, source_map);
        };
        let token = token_rest[..end].trim();
        if let Some((_, value, entries)) = values.iter().find(|(key, _, _)| *key == token) {
            let offset = output.len();
            output.push_str(value);
            source_map.extend(entries.iter().cloned().map(|mut entry| {
                entry.start += offset;
                entry.end += offset;
                entry
            }));
        } else {
            output.push_str(&rest[start..start + end + 4]);
        }
        rest = &token_rest[end + 2..];
    }
    output.push_str(rest);
    (output.trim_end().to_string(), source_map)
}

fn compile_prompt(
    app_data_dir: &Path,
    project_path: &Path,
    input: CompilePromptInput,
) -> AppResult<PromptCompilation> {
    validate_nonempty(&[
        ("请求 ID", &input.request_id),
        ("生成任务 ID", &input.generation_task_id),
        ("模型档案 Key", &input.model_profile_key),
        ("模板 ID", &input.template_id),
    ])?;
    let app_conn = open_app_database(app_data_dir)?;
    let profile = load_profile(&app_conn, &input.model_profile_key)?;
    let template = load_template(&app_conn, &input.template_id)?;
    if !template.active || template.model_profile_key != profile.key {
        return Err("TOOL_ARGUMENT_INVALID: 模板未启用或不属于所选模型档案".into());
    }
    let conn = open_database(project_path)?;
    if let Ok(existing) = load_compilation(&conn, &input.request_id) {
        if existing.generation_task_id == input.generation_task_id
            && existing.model_profile_key == input.model_profile_key
            && existing.template_id == input.template_id
        {
            return Ok(existing);
        }
        return Err("REQUEST_ID_CONFLICT: 请求 ID 已用于其他编译参数".into());
    }
    let (task_name, task_duration, project_id, revision): (String,f64,String,i64) = conn.query_row("SELECT gt.name,gt.duration,p.id,p.revision FROM generation_tasks gt JOIN projects p WHERE gt.id=?1 LIMIT 1", [&input.generation_task_id], |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).map_err(|_|"OBJECT_NOT_FOUND: 生成任务不存在".to_string())?;
    if template.scope == "project" && template.project_id.as_deref() != Some(&project_id) {
        return Err("TOOL_ARGUMENT_INVALID: 项目模板不属于当前项目".into());
    }
    let shots = load_shots(&conn, &input.generation_task_id)?;
    let mut warnings = vec![];
    if shots.is_empty() {
        warnings.push(PromptWarning {
            code: "NO_SHOTS".into(),
            severity: "error".into(),
            message: "生成任务没有镜头，编译结果不完整".into(),
            source_id: Some(input.generation_task_id.clone()),
        });
    }
    if profile
        .max_shots_hint
        .is_some_and(|max| shots.len() as i64 > max)
    {
        warnings.push(PromptWarning {
            code: "SHOT_LIMIT".into(),
            severity: "warning".into(),
            message: format!(
                "镜头数 {} 超过模型档案建议上限 {}",
                shots.len(),
                profile.max_shots_hint.unwrap_or_default()
            ),
            source_id: Some(input.generation_task_id.clone()),
        });
    }
    if profile
        .max_duration_hint
        .is_some_and(|max| task_duration > max)
    {
        warnings.push(PromptWarning {
            code: "DURATION_LIMIT".into(),
            severity: "warning".into(),
            message: format!(
                "总时长 {task_duration:.1}s 超过模型档案建议上限 {:.1}s",
                profile.max_duration_hint.unwrap_or_default()
            ),
            source_id: Some(input.generation_task_id.clone()),
        });
    }

    let header_segments = vec![Segment {
        text: format!(
            "任务：{task_name}\n目标模型：{}（{}）\n总时长：{task_duration:.1}s",
            profile.display_name, profile.version
        ),
        source_type: "generationTask".into(),
        source_id: input.generation_task_id.clone(),
        label: "生成任务".into(),
    }];
    let mut style_stmt = conn.prepare("SELECT id,content FROM project_memories WHERE status='active' AND category IN ('style','visual','visual_style') ORDER BY priority DESC,updated_at,id").map_err(|e|e.to_string())?;
    let style_segments = style_stmt
        .query_map([], |row| {
            Ok(Segment {
                text: row.get(1)?,
                source_type: "projectMemory".into(),
                source_id: row.get(0)?,
                label: "项目视觉规则".into(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut shot_segments = vec![];
    for (index, shot) in shots.iter().enumerate() {
        if shot.keyframes.is_empty() {
            warnings.push(PromptWarning {
                code: "MISSING_KEYFRAME".into(),
                severity: "warning".into(),
                message: format!("镜头 {} 没有正式关键帧", shot.title),
                source_id: Some(shot.id.clone()),
            });
        }
        if !profile.supports_start_end_frame
            && shot
                .keyframes
                .iter()
                .any(|(_, kind, _)| kind == "start" || kind == "end")
        {
            warnings.push(PromptWarning {
                code: "UNSUPPORTED_START_END_FRAME".into(),
                severity: "warning".into(),
                message: format!("模型档案不支持起止帧，但镜头 {} 已配置起止帧", shot.title),
                source_id: Some(shot.id.clone()),
            });
        }
        for (asset_id, name, _, has_media) in &shot.assets {
            if !has_media {
                warnings.push(PromptWarning {
                    code: "MISSING_ASSET_MEDIA".into(),
                    severity: "warning".into(),
                    message: format!("资产 {name} 没有正式媒体"),
                    source_id: Some(asset_id.clone()),
                });
            }
        }
        if shot.subjects.chars().count() + shot.action.chars().count() > 240 {
            warnings.push(PromptWarning {
                code: "SHOT_COMPLEXITY".into(),
                severity: "info".into(),
                message: format!("镜头 {} 的主体与动作描述较复杂", shot.title),
                source_id: Some(shot.id.clone()),
            });
        }
        let details = join_nonempty(&[
            ("景别", &shot.shot_size),
            ("构图", &shot.composition),
            ("运镜", &shot.camera_movement),
            ("主体", &shot.subjects),
            ("动作", &shot.action),
            ("对白", &shot.dialogue),
            ("环境", &shot.environment),
            ("起始状态", &shot.start_state),
            ("结束状态", &shot.end_state),
        ]);
        let assets = shot
            .assets
            .iter()
            .map(|(_, name, role, has_media)| {
                format!(
                    "{name}({role}{})",
                    if *has_media {
                        ",正式媒体"
                    } else {
                        ",缺少媒体"
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("、");
        let frames = shot
            .keyframes
            .iter()
            .map(|(_, kind, path)| format!("{kind}:{path}"))
            .collect::<Vec<_>>()
            .join("、");
        shot_segments.push(Segment {
            text: format!(
                "镜头 {}｜{}｜{:.1}s\n{}\n资产：{}\n关键帧：{}",
                index + 1,
                shot.title,
                shot.duration,
                if details.is_empty() {
                    "未填写画面事实"
                } else {
                    &details
                },
                if assets.is_empty() { "无" } else { &assets },
                if frames.is_empty() { "无" } else { &frames }
            ),
            source_type: "shot".into(),
            source_id: shot.id.clone(),
            label: format!("镜头 {}", index + 1),
        });
    }
    let mut constraint_segments = profile
        .recommended_constraints
        .iter()
        .enumerate()
        .map(|(index, text)| Segment {
            text: text.clone(),
            source_type: "modelProfile".into(),
            source_id: profile.key.clone(),
            label: format!("建议约束 {}", index + 1),
        })
        .collect::<Vec<_>>();
    if !profile.image_reference_rules.trim().is_empty() {
        constraint_segments.push(Segment {
            text: profile.image_reference_rules.clone(),
            source_type: "modelProfile".into(),
            source_id: profile.key.clone(),
            label: "参考图规则".into(),
        });
    }
    let (header, header_map) = render_segments(&header_segments);
    let (visual, visual_map) = render_segments(&style_segments);
    let (shot_text, shot_map) = render_segments(&shot_segments);
    let (constraints, constraint_map) = render_segments(&constraint_segments);
    let (compiled_prompt, source_map) = render_template(
        &template.template_body,
        &[
            ("header", header, header_map),
            ("visual_rules", visual, visual_map),
            ("shots", shot_text, shot_map),
            ("constraints", constraints, constraint_map),
        ],
    );
    for pattern in &profile.prohibited_patterns {
        if !pattern.is_empty() && compiled_prompt.contains(pattern) {
            warnings.push(PromptWarning {
                code: "PROHIBITED_PATTERN".into(),
                severity: "warning".into(),
                message: format!("编译结果包含模型档案禁止模式：{pattern}"),
                source_id: Some(profile.key.clone()),
            });
        }
    }
    let timestamp = now();
    conn.execute("INSERT INTO prompt_compilations (id,generation_task_id,model_profile_key,model_profile_version,template_id,template_version,source_revision,compiled_prompt,source_map_json,warnings_json,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'compiled',?11,?11)",params![input.request_id,input.generation_task_id,profile.key,profile.version,template.id,template.version,revision,compiled_prompt,json!(source_map).to_string(),json!(warnings).to_string(),timestamp]).map_err(|e|format!("保存提示词编译记录失败：{e}"))?;
    load_compilation(&conn, &input.request_id)
}

fn load_compilation(conn: &Connection, id: &str) -> AppResult<PromptCompilation> {
    conn.query_row("SELECT id,generation_task_id,model_profile_key,model_profile_version,template_id,template_version,source_revision,compiled_prompt,user_override,current_prompt,source_map_json,warnings_json,status,created_at,updated_at FROM prompt_compilations WHERE id=?1",[id],|row|Ok(PromptCompilation{id:row.get(0)?,generation_task_id:row.get(1)?,model_profile_key:row.get(2)?,model_profile_version:row.get(3)?,template_id:row.get(4)?,template_version:row.get(5)?,source_revision:row.get(6)?,compiled_prompt:row.get(7)?,user_override:row.get(8)?,current_prompt:row.get(9)?,source_map:serde_json::from_str(&row.get::<_,String>(10)?).unwrap_or_default(),warnings:serde_json::from_str(&row.get::<_,String>(11)?).unwrap_or_default(),status:row.get(12)?,created_at:row.get(13)?,updated_at:row.get(14)?})).map_err(|_|"OBJECT_NOT_FOUND: 编译记录不存在".to_string())
}

fn list_compilations(project_path: &Path, task_id: &str) -> AppResult<Vec<PromptCompilation>> {
    let conn = open_database(project_path)?;
    let mut stmt=conn.prepare("SELECT id FROM prompt_compilations WHERE generation_task_id=?1 ORDER BY created_at DESC,id DESC").map_err(|e|e.to_string())?;
    let ids = stmt
        .query_map([task_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    ids.iter().map(|id| load_compilation(&conn, id)).collect()
}

fn set_current_prompt(
    project_path: &Path,
    input: SetCurrentPromptInput,
) -> AppResult<PromptCompilation> {
    if input.prompt.trim().is_empty() {
        return Err("TOOL_ARGUMENT_INVALID: 正式提示词不能为空".into());
    }
    let mut conn = open_database(project_path)?;
    let current_revision: i64 = conn
        .query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    if current_revision != input.expected_revision {
        return Err(format!(
            "REVISION_CONFLICT: 项目已从修订 {} 更新到 {}，请刷新后重试",
            input.expected_revision, current_revision
        ));
    }
    let compilation = load_compilation(&conn, &input.compilation_id)?;
    let prompt = input.prompt.trim().to_string();
    let mut values = Map::new();
    values.insert("prompt".into(), Value::String(prompt.clone()));
    values.insert(
        "target_model".into(),
        Value::String(compilation.model_profile_key.clone()),
    );
    let mutation = MutationRequest {
        action: "patch".into(),
        entity_type: "generationTask".into(),
        object_id: Some(compilation.generation_task_id.clone()),
        values,
        change_set_id: None,
        change_set_name: None,
        source_type: Some("prompt_compiler".into()),
        source_id: Some(compilation.id.clone()),
    };
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    execute_mutations_in_transaction(
        &tx,
        vec![mutation],
        None,
        Some("设为当前正式提示词".into()),
        Some("prompt_compiler".into()),
        Some(compilation.id.clone()),
    )?;
    tx.execute("UPDATE prompt_compilations SET status='compiled',current_prompt=NULL,updated_at=?1 WHERE generation_task_id=?2 AND status='current'",params![now(),compilation.generation_task_id]).map_err(|e|e.to_string())?;
    let user_override = if prompt == compilation.compiled_prompt {
        None
    } else {
        Some(prompt.clone())
    };
    tx.execute("UPDATE prompt_compilations SET status='current',user_override=?1,current_prompt=?2,updated_at=?3 WHERE id=?4",params![user_override,prompt,now(),compilation.id]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    load_compilation(&conn, &input.compilation_id)
}

#[tauri::command]
pub fn prompt_list_profiles(app: tauri::AppHandle) -> AppResult<Vec<ModelProfile>> {
    let dir = ensure_enabled(&app)?;
    list_profiles(&dir)
}
#[tauri::command]
pub fn prompt_save_profile(
    app: tauri::AppHandle,
    input: SaveModelProfileInput,
) -> AppResult<ModelProfile> {
    let dir = ensure_enabled(&app)?;
    save_profile(&dir, input)
}
#[tauri::command]
pub fn prompt_list_templates(
    app: tauri::AppHandle,
    project_id: Option<String>,
) -> AppResult<Vec<PromptTemplate>> {
    let dir = ensure_enabled(&app)?;
    list_templates(&dir, project_id.as_deref())
}
#[tauri::command]
pub fn prompt_save_template(
    app: tauri::AppHandle,
    input: SavePromptTemplateInput,
) -> AppResult<PromptTemplate> {
    let dir = ensure_enabled(&app)?;
    save_template(&dir, input)
}
#[tauri::command]
pub fn prompt_compile(
    app: tauri::AppHandle,
    project_path: String,
    input: CompilePromptInput,
) -> AppResult<PromptCompilation> {
    let dir = ensure_enabled(&app)?;
    compile_prompt(&dir, Path::new(&project_path), input)
}
#[tauri::command]
pub fn prompt_list_compilations(
    app: tauri::AppHandle,
    project_path: String,
    generation_task_id: String,
) -> AppResult<Vec<PromptCompilation>> {
    ensure_enabled(&app)?;
    list_compilations(Path::new(&project_path), &generation_task_id)
}
#[tauri::command]
pub fn prompt_set_current(
    app: tauri::AppHandle,
    project_path: String,
    input: SetCurrentPromptInput,
) -> AppResult<PromptCompilation> {
    ensure_enabled(&app)?;
    set_current_prompt(Path::new(&project_path), input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{init_database, new_id};
    use crate::mutation::{apply_batch_mutation, BatchMutationRequest};

    fn request(
        action: &str,
        entity_type: &str,
        id: Option<&str>,
        values: Value,
    ) -> MutationRequest {
        MutationRequest {
            action: action.into(),
            entity_type: entity_type.into(),
            object_id: id.map(str::to_string),
            values: values.as_object().cloned().unwrap_or_default(),
            change_set_id: None,
            change_set_name: None,
            source_type: None,
            source_id: None,
        }
    }
    fn setup() -> (tempfile::TempDir, tempfile::TempDir, String) {
        let app = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let descriptor = init_database(project.path(), "编译测试", "short").unwrap();
        save_profile(
            app.path(),
            SaveModelProfileInput {
                key: "model-a".into(),
                display_name: "模型 A".into(),
                provider: "test".into(),
                prompt_format: "plain_text".into(),
                max_duration_hint: Some(4.0),
                max_shots_hint: Some(1),
                image_reference_rules: "保持参考图主体一致".into(),
                supports_start_end_frame: false,
                recommended_constraints: vec!["画面连续".into()],
                prohibited_patterns: vec!["禁词".into()],
                version: "1.2".into(),
            },
        )
        .unwrap();
        save_template(app.path(),SavePromptTemplateInput{id:"template-a".into(),scope:"global".into(),project_id:None,model_profile_key:"model-a".into(),name:"模板 A".into(),version:"2.1".into(),template_body:"{{header}}\n视觉规则\n{{visual_rules}}\n镜头清单\n{{shots}}\n约束\n{{constraints}}".into(),conditional_rules:json!({}),active:true}).unwrap();
        save_profile(
            app.path(),
            SaveModelProfileInput {
                key: "model-b".into(),
                display_name: "模型 B".into(),
                provider: "test".into(),
                prompt_format: "plain_text".into(),
                max_duration_hint: None,
                max_shots_hint: None,
                image_reference_rules: "".into(),
                supports_start_end_frame: true,
                recommended_constraints: vec!["模型 B 专属约束".into()],
                prohibited_patterns: vec![],
                version: "3.0".into(),
            },
        )
        .unwrap();
        save_template(
            app.path(),
            SavePromptTemplateInput {
                id: "template-b".into(),
                scope: "global".into(),
                project_id: None,
                model_profile_key: "model-b".into(),
                name: "模板 B".into(),
                version: "3.1".into(),
                template_body: "B-MODEL\n{{shots}}\n{{constraints}}".into(),
                conditional_rules: json!({}),
                active: true,
            },
        )
        .unwrap();
        let unit = new_id();
        let script = new_id();
        let scene = new_id();
        let shot_a = new_id();
        let shot_b = new_id();
        let task = new_id();
        let asset = new_id();
        apply_batch_mutation(project.path().to_string_lossy().into(),BatchMutationRequest{mutations:vec![
            request("create","contentUnit",Some(&unit),json!({"project_id":descriptor.id,"type":"short","name":"正片","sort_order":0})),request("create","script",Some(&script),json!({"content_unit_id":unit,"title":"正片"})),request("create","scene",Some(&scene),json!({"script_id":script,"title":"场","sort_order":0})),
            request("create","shot",Some(&shot_a),json!({"scene_id":scene,"title":"镜头 A","sort_order":0,"duration":3.0,"subjects":"角色","action":"前进"})),request("create","shot",Some(&shot_b),json!({"scene_id":scene,"title":"镜头 B","sort_order":1,"duration":3.0,"subjects":"角色","action":"停下"})),
            request("create","asset",Some(&asset),json!({"project_id":descriptor.id,"type":"character","name":"主角"})),request("create","shotAsset",None,json!({"shot_id":shot_a,"asset_id":asset,"role":"subject"})),request("create","generationTask",Some(&task),json!({"content_unit_id":unit,"name":"任务","prompt":"人工原稿"})),request("create","generationTaskShot",None,json!({"generation_task_id":task,"shot_id":shot_a,"sort_order":0})),request("create","generationTaskShot",None,json!({"generation_task_id":task,"shot_id":shot_b,"sort_order":1}))],change_set_id:None,change_set_name:Some("测试数据".into()),source_type:None,source_id:None}).unwrap();
        (app, project, task)
    }

    #[test]
    fn compiles_stably_with_trace_and_warnings_without_overwriting_prompt() {
        let (app, project, task) = setup();
        let first = compile_prompt(
            app.path(),
            project.path(),
            CompilePromptInput {
                request_id: "compile-1".into(),
                generation_task_id: task.clone(),
                model_profile_key: "model-a".into(),
                template_id: "template-a".into(),
            },
        )
        .unwrap();
        let second = compile_prompt(
            app.path(),
            project.path(),
            CompilePromptInput {
                request_id: "compile-2".into(),
                generation_task_id: task.clone(),
                model_profile_key: "model-a".into(),
                template_id: "template-a".into(),
            },
        )
        .unwrap();
        assert_eq!(first.compiled_prompt, second.compiled_prompt);
        assert_eq!(
            (
                first.model_profile_version.as_str(),
                first.template_version.as_str()
            ),
            ("1.2", "2.1")
        );
        assert!(first.warnings.iter().any(|w| w.code == "SHOT_LIMIT"));
        assert!(first.warnings.iter().any(|w| w.code == "DURATION_LIMIT"));
        assert!(first.warnings.iter().any(|w| w.code == "MISSING_KEYFRAME"));
        assert!(first
            .warnings
            .iter()
            .any(|w| w.code == "MISSING_ASSET_MEDIA"));
        for entry in &first.source_map {
            assert!(!first.compiled_prompt[entry.start..entry.end].is_empty());
        }
        let conn = open_database(project.path()).unwrap();
        let prompt: String = conn
            .query_row(
                "SELECT prompt FROM generation_tasks WHERE id=?1",
                [task],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(prompt, "人工原稿");
    }

    #[test]
    fn setting_override_is_atomic_and_recompile_preserves_it() {
        let (app, project, task) = setup();
        let compiled = compile_prompt(
            app.path(),
            project.path(),
            CompilePromptInput {
                request_id: "compile-current".into(),
                generation_task_id: task.clone(),
                model_profile_key: "model-a".into(),
                template_id: "template-a".into(),
            },
        )
        .unwrap();
        let current = set_current_prompt(
            project.path(),
            SetCurrentPromptInput {
                compilation_id: compiled.id.clone(),
                prompt: "人工覆盖版".into(),
                expected_revision: compiled.source_revision,
            },
        )
        .unwrap();
        assert_eq!(current.user_override.as_deref(), Some("人工覆盖版"));
        assert_eq!(current.status, "current");
        let _new = compile_prompt(
            app.path(),
            project.path(),
            CompilePromptInput {
                request_id: "compile-new".into(),
                generation_task_id: task.clone(),
                model_profile_key: "model-a".into(),
                template_id: "template-a".into(),
            },
        )
        .unwrap();
        let conn = open_database(project.path()).unwrap();
        let prompt: String = conn
            .query_row(
                "SELECT prompt FROM generation_tasks WHERE id=?1",
                [task],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(prompt, "人工覆盖版");
        assert_eq!(
            load_compilation(&conn, &compiled.id)
                .unwrap()
                .user_override
                .as_deref(),
            Some("人工覆盖版")
        );
    }

    #[test]
    fn same_task_compiles_different_model_specific_prompts() {
        let (app, project, task) = setup();
        let model_a = compile_prompt(
            app.path(),
            project.path(),
            CompilePromptInput {
                request_id: "model-a-result".into(),
                generation_task_id: task.clone(),
                model_profile_key: "model-a".into(),
                template_id: "template-a".into(),
            },
        )
        .unwrap();
        let model_b = compile_prompt(
            app.path(),
            project.path(),
            CompilePromptInput {
                request_id: "model-b-result".into(),
                generation_task_id: task,
                model_profile_key: "model-b".into(),
                template_id: "template-b".into(),
            },
        )
        .unwrap();
        assert_ne!(model_a.compiled_prompt, model_b.compiled_prompt);
        assert!(model_b.compiled_prompt.starts_with("B-MODEL"));
        assert_eq!(model_b.model_profile_version, "3.0");
        assert_eq!(model_b.template_version, "3.1");
    }
}
