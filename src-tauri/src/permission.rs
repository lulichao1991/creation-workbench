use crate::agent_runtime::ensure_agent_core_enabled;
use crate::database::{new_id, now, open_database, AppResult};
use crate::mutation::{execute_mutations_in_transaction, read_field_value, MutationRequest};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::Path;

const CARD_TYPES: &[&str] = &[
    "problem",
    "question",
    "permission",
    "suggestion",
    "expert_team",
    "cost",
    "stale",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ObjectRef {
    pub project_id: String,
    pub object_type: String,
    pub object_id: String,
    pub field: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriteScope {
    #[serde(default)]
    pub refs: Vec<ObjectRef>,
    #[serde(default)]
    pub protected_refs: Vec<ObjectRef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchItemInput {
    pub object_type: String,
    pub object_id: String,
    pub field_name: String,
    pub old_value: Value,
    pub new_value: Value,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposePatchInput {
    pub request_id: String,
    pub task_id: String,
    pub base_revision: i64,
    pub title: String,
    pub items: Vec<PatchItemInput>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchItem {
    pub id: String,
    pub object_type: String,
    pub object_id: String,
    pub field_name: String,
    pub old_value: Value,
    pub new_value: Value,
    pub reason: String,
    pub permission_state: String,
    pub apply_state: String,
    pub sort_order: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchProposal {
    pub id: String,
    pub task_id: String,
    pub base_revision: i64,
    pub title: String,
    pub status: String,
    pub items: Vec<PatchItem>,
    pub permission_card_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPatchInput {
    pub proposal_id: String,
    #[serde(default)]
    pub approved_item_ids: Vec<String>,
    #[serde(default)]
    pub rejected_item_ids: Vec<String>,
    pub permission_card_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPatchResponse {
    pub proposal_id: String,
    pub status: String,
    pub applied_item_ids: Vec<String>,
    pub rejected_item_ids: Vec<String>,
    pub change_set_id: Option<String>,
    pub revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCardInput {
    pub request_id: String,
    pub task_id: String,
    pub card_type: String,
    pub related_ref: Option<ObjectRef>,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub options: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCard {
    pub id: String,
    pub task_id: String,
    pub card_type: String,
    pub related_ref: Option<ObjectRef>,
    pub title: String,
    pub body: String,
    pub options: Value,
    pub status: String,
    pub resolution: Option<Value>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCardInput {
    pub card_id: String,
    pub status: String,
    pub resolution: Value,
}

#[tauri::command]
pub fn patch_propose(
    app: tauri::AppHandle,
    project_path: String,
    input: ProposePatchInput,
) -> AppResult<PatchProposal> {
    ensure_agent_core_enabled(&app)?;
    propose_patch(Path::new(&project_path), input)
}

#[tauri::command]
pub fn patch_get(
    app: tauri::AppHandle,
    project_path: String,
    proposal_id: String,
) -> AppResult<PatchProposal> {
    ensure_agent_core_enabled(&app)?;
    let conn = open_database(Path::new(&project_path))?;
    load_proposal(&conn, &proposal_id)
}

#[tauri::command]
pub fn patch_apply(
    app: tauri::AppHandle,
    project_path: String,
    input: ApplyPatchInput,
) -> AppResult<ApplyPatchResponse> {
    ensure_agent_core_enabled(&app)?;
    apply_patch(Path::new(&project_path), input)
}

#[tauri::command]
pub fn patch_reject(
    app: tauri::AppHandle,
    project_path: String,
    proposal_id: String,
) -> AppResult<PatchProposal> {
    ensure_agent_core_enabled(&app)?;
    reject_patch(Path::new(&project_path), &proposal_id)
}

#[tauri::command]
pub fn card_create(
    app: tauri::AppHandle,
    project_path: String,
    input: CreateCardInput,
) -> AppResult<AiCard> {
    ensure_agent_core_enabled(&app)?;
    create_card(Path::new(&project_path), input)
}

#[tauri::command]
pub fn card_get(app: tauri::AppHandle, project_path: String, card_id: String) -> AppResult<AiCard> {
    ensure_agent_core_enabled(&app)?;
    let conn = open_database(Path::new(&project_path))?;
    load_card(&conn, &card_id)
}

#[tauri::command]
pub fn card_list(
    app: tauri::AppHandle,
    project_path: String,
    task_id: String,
) -> AppResult<Vec<AiCard>> {
    ensure_agent_core_enabled(&app)?;
    let conn = open_database(Path::new(&project_path))?;
    list_cards(&conn, &task_id)
}

#[tauri::command]
pub fn card_resolve(
    app: tauri::AppHandle,
    project_path: String,
    input: ResolveCardInput,
) -> AppResult<AiCard> {
    ensure_agent_core_enabled(&app)?;
    resolve_card(Path::new(&project_path), input)
}

pub(crate) fn propose_patch(
    project_path: &Path,
    input: ProposePatchInput,
) -> AppResult<PatchProposal> {
    if input.request_id.trim().is_empty() || input.task_id.trim().is_empty() {
        return Err("TOOL_ARGUMENT_INVALID: requestId 和 taskId 不能为空".into());
    }
    if input.items.is_empty() {
        return Err("TOOL_ARGUMENT_INVALID: 修改提案不能为空".into());
    }
    let mut conn = open_database(project_path)?;
    if let Ok(existing) = load_proposal(&conn, &input.request_id) {
        if existing.task_id != input.task_id {
            return Err("TOOL_ARGUMENT_INVALID: requestId 已被其他任务使用".into());
        }
        return Ok(existing);
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (project_id, revision) = project_identity(&tx)?;
    if revision != input.base_revision {
        return Err(format!(
            "REVISION_STALE: 提案 revision {} != 当前 revision {revision}",
            input.base_revision
        ));
    }
    let write_scope = task_write_scope(&tx, &input.task_id)?;
    let mut seen = HashSet::new();
    let mut prepared = Vec::with_capacity(input.items.len());
    for (index, item) in input.items.into_iter().enumerate() {
        let key = format!(
            "{}\0{}\0{}",
            item.object_type, item.object_id, item.field_name
        );
        if !seen.insert(key) {
            return Err("TOOL_ARGUMENT_INVALID: 同一提案不能重复修改同一字段".into());
        }
        let current = read_field_value(&tx, &item.object_type, &item.object_id, &item.field_name)
            .map_err(|error| format!("OBJECT_NOT_FOUND: {error}"))?;
        if current != item.old_value {
            return Err(format!(
                "REVISION_STALE: {}:{} 的旧值不匹配",
                item.object_type, item.object_id
            ));
        }
        let permission_state = permission_for(
            &tx,
            &write_scope,
            &project_id,
            &item.object_type,
            &item.object_id,
            &item.field_name,
        );
        prepared.push((new_id(), index as i64, item, permission_state));
    }
    let timestamp = now();
    tx.execute(
        "INSERT INTO patch_proposals (id, task_id, base_revision, title, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)",
        params![input.request_id, input.task_id, input.base_revision, input.title, timestamp],
    )
    .map_err(|e| e.to_string())?;
    for (id, sort_order, item, permission_state) in &prepared {
        tx.execute(
            "INSERT INTO patch_items (id, proposal_id, object_type, object_id, field_name, old_value_json, new_value_json, reason, permission_state, apply_state, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10)",
            params![id, input.request_id, item.object_type, item.object_id, item.field_name, serde_json::to_string(&item.old_value).map_err(|e| e.to_string())?, serde_json::to_string(&item.new_value).map_err(|e| e.to_string())?, item.reason, permission_state, sort_order],
        )
        .map_err(|e| e.to_string())?;
    }
    if prepared
        .iter()
        .any(|item| item.3 == "requires_confirmation")
    {
        insert_permission_card(
            &tx,
            &input.request_id,
            &input.task_id,
            &project_id,
            &write_scope,
            &prepared,
        )?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    load_proposal(&conn, &input.request_id)
}

fn apply_patch(project_path: &Path, input: ApplyPatchInput) -> AppResult<ApplyPatchResponse> {
    let mut conn = open_database(project_path)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let proposal = load_proposal(&tx, &input.proposal_id)?;
    if !matches!(proposal.status.as_str(), "draft" | "pending" | "approved") {
        return Err(format!(
            "TOOL_ARGUMENT_INVALID: 提案状态为 {}，不能应用",
            proposal.status
        ));
    }
    let (project_id, revision) = project_identity(&tx)?;
    if revision != proposal.base_revision {
        // ponytail: expire on any revision; add per-object impact tracking only if concurrent proposal throughput requires it.
        return mark_stale_and_finish(tx, &proposal.id, revision, "项目 revision 已变化");
    }
    let write_scope = task_write_scope(&tx, &proposal.task_id)?;
    for item in &proposal.items {
        let current_permission = permission_for(
            &tx,
            &write_scope,
            &project_id,
            &item.object_type,
            &item.object_id,
            &item.field_name,
        );
        if current_permission != item.permission_state {
            return mark_stale_and_finish(tx, &proposal.id, revision, "写入范围已变化");
        }
        match read_field_value(&tx, &item.object_type, &item.object_id, &item.field_name) {
            Ok(current) if current == item.old_value => {}
            Ok(_) => return mark_stale_and_finish(tx, &proposal.id, revision, "字段旧值已变化"),
            Err(_) => return mark_stale_and_finish(tx, &proposal.id, revision, "对象已删除"),
        }
    }

    let known = proposal
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let approved = input
        .approved_item_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let rejected = input
        .rejected_item_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if !approved.is_disjoint(&rejected)
        || approved
            .iter()
            .chain(rejected.iter())
            .any(|id| !known.contains(id))
    {
        return Err("TOOL_ARGUMENT_INVALID: 批准与拒绝的 PatchItem 无效".into());
    }
    let confirmation_ids = proposal
        .items
        .iter()
        .filter(|item| item.permission_state == "requires_confirmation")
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    if confirmation_ids
        .iter()
        .any(|id| !approved.contains(id) && !rejected.contains(id))
    {
        return Err("WRITE_SCOPE_DENIED: 写入范围外字段必须逐项批准或拒绝".into());
    }
    if proposal
        .items
        .iter()
        .any(|item| item.permission_state == "denied" && approved.contains(item.id.as_str()))
    {
        return Err("WRITE_SCOPE_DENIED: 保护字段不能批准".into());
    }
    if !confirmation_ids.is_empty() {
        validate_permission_card(
            &tx,
            input.permission_card_id.as_deref(),
            &proposal.task_id,
            &proposal.id,
        )?;
    }

    let mut mutations = Vec::new();
    let mut applied_ids = Vec::new();
    let mut rejected_ids = Vec::new();
    for item in &proposal.items {
        let should_reject = rejected.contains(item.id.as_str());
        let should_apply = item.permission_state == "allowed"
            || (item.permission_state == "requires_confirmation"
                && approved.contains(item.id.as_str()));
        let apply_state = if item.permission_state == "denied" {
            "denied"
        } else if should_reject {
            rejected_ids.push(item.id.clone());
            "rejected"
        } else if should_apply {
            let mut values = Map::new();
            values.insert(item.field_name.clone(), item.new_value.clone());
            mutations.push(MutationRequest {
                action: "patch".into(),
                entity_type: item.object_type.clone(),
                object_id: Some(item.object_id.clone()),
                values,
                change_set_id: None,
                change_set_name: None,
                source_type: Some("agent".into()),
                source_id: Some(proposal.id.clone()),
            });
            applied_ids.push(item.id.clone());
            "applied"
        } else {
            "denied"
        };
        tx.execute(
            "UPDATE patch_items SET apply_state=?1 WHERE id=?2",
            params![apply_state, item.id],
        )
        .map_err(|e| e.to_string())?;
    }

    let mutation_result = if mutations.is_empty() {
        None
    } else {
        Some(execute_mutations_in_transaction(
            &tx,
            mutations,
            None,
            Some(proposal.title.clone()),
            Some("agent".into()),
            Some(proposal.id.clone()),
        )?)
    };
    let status = if applied_ids.is_empty() {
        "rejected"
    } else {
        "applied"
    };
    tx.execute(
        "UPDATE patch_proposals SET status=?1, updated_at=?2 WHERE id=?3",
        params![status, now(), proposal.id],
    )
    .map_err(|e| e.to_string())?;
    if let Some(card_id) = input.permission_card_id.as_deref() {
        resolve_permission_card(&tx, card_id, &applied_ids, &rejected_ids)?;
    }
    let result_revision = mutation_result
        .as_ref()
        .map_or(revision, |result| result.revision);
    let change_set_id = mutation_result.map(|result| result.change_set_id);
    tx.commit().map_err(|e| e.to_string())?;
    Ok(ApplyPatchResponse {
        proposal_id: proposal.id,
        status: status.into(),
        applied_item_ids: applied_ids,
        rejected_item_ids: rejected_ids,
        change_set_id,
        revision: result_revision,
    })
}

fn reject_patch(project_path: &Path, proposal_id: &str) -> AppResult<PatchProposal> {
    let mut conn = open_database(project_path)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let proposal = load_proposal(&tx, proposal_id)?;
    if !matches!(proposal.status.as_str(), "draft" | "pending" | "approved") {
        return Err(format!(
            "TOOL_ARGUMENT_INVALID: 提案状态为 {}，不能拒绝",
            proposal.status
        ));
    }
    tx.execute(
        "UPDATE patch_items SET apply_state=CASE WHEN permission_state='denied' THEN 'denied' ELSE 'rejected' END WHERE proposal_id=?1",
        [proposal_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE patch_proposals SET status='rejected', updated_at=?1 WHERE id=?2",
        params![now(), proposal_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE ai_cards SET status='dismissed', resolved_at=?1, resolution_json=?2 WHERE id=?3 AND status='open'",
        params![now(), json!({"action": "keep_current"}).to_string(), permission_card_id(proposal_id)],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    load_proposal(&conn, proposal_id)
}

pub(crate) fn create_card(project_path: &Path, input: CreateCardInput) -> AppResult<AiCard> {
    if input.request_id.trim().is_empty() || !CARD_TYPES.contains(&input.card_type.as_str()) {
        return Err("TOOL_ARGUMENT_INVALID: AI Card requestId 或 type 无效".into());
    }
    let conn = open_database(project_path)?;
    if let Ok(existing) = load_card(&conn, &input.request_id) {
        if existing.task_id != input.task_id {
            return Err("TOOL_ARGUMENT_INVALID: requestId 已被其他任务使用".into());
        }
        return Ok(existing);
    }
    let task_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_tasks WHERE id=?1)",
            [&input.task_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !task_exists {
        return Err("OBJECT_NOT_FOUND: Agent 任务不存在".into());
    }
    conn.execute(
        "INSERT INTO ai_cards (id, task_id, card_type, related_ref_json, title, body, options_json, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8)",
        params![input.request_id, input.task_id, input.card_type, input.related_ref.as_ref().map(serde_json::to_string).transpose().map_err(|e| e.to_string())?, input.title, input.body, serde_json::to_string(&input.options).map_err(|e| e.to_string())?, now()],
    )
    .map_err(|e| e.to_string())?;
    load_card(&conn, &input.request_id)
}

fn resolve_card(project_path: &Path, input: ResolveCardInput) -> AppResult<AiCard> {
    if !matches!(input.status.as_str(), "resolved" | "dismissed") {
        return Err("TOOL_ARGUMENT_INVALID: Card 只能 resolved 或 dismissed".into());
    }
    let conn = open_database(project_path)?;
    let card_type: String = conn
        .query_row(
            "SELECT card_type FROM ai_cards WHERE id=?1 AND status='open'",
            [&input.card_id],
            |row| row.get(0),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: 待处理 AI Card 不存在".to_string())?;
    if card_type == "permission" {
        return Err(
            "TOOL_ARGUMENT_INVALID: 权限卡必须通过 patch_apply 或 patch_reject 处理".into(),
        );
    }
    let changed = conn
        .execute(
            "UPDATE ai_cards SET status=?1, resolution_json=?2, resolved_at=?3 WHERE id=?4 AND status='open'",
            params![input.status, serde_json::to_string(&input.resolution).map_err(|e| e.to_string())?, now(), input.card_id],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("OBJECT_NOT_FOUND: 待处理 AI Card 不存在".into());
    }
    load_card(&conn, &input.card_id)
}

fn project_identity(tx: &Transaction<'_>) -> AppResult<(String, i64)> {
    tx.query_row("SELECT id, revision FROM projects LIMIT 1", [], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })
    .map_err(|e| e.to_string())
}

fn task_write_scope(tx: &Transaction<'_>, task_id: &str) -> AppResult<WriteScope> {
    let scope_json: String = tx
        .query_row(
            "SELECT write_scope_json FROM agent_tasks WHERE id=?1",
            [task_id],
            |row| row.get(0),
        )
        .map_err(|_| "OBJECT_NOT_FOUND: Agent 任务不存在".to_string())?;
    serde_json::from_str(&scope_json)
        .map_err(|e| format!("TOOL_ARGUMENT_INVALID: write_scope_json 无效：{e}"))
}

fn permission_for(
    tx: &Transaction<'_>,
    scope: &WriteScope,
    project_id: &str,
    object_type: &str,
    object_id: &str,
    field: &str,
) -> String {
    if scope
        .protected_refs
        .iter()
        .any(|reference| reference_matches(reference, project_id, object_type, object_id, field))
    {
        "denied".into()
    } else if scope
        .refs
        .iter()
        .any(|reference| reference_matches(reference, project_id, object_type, object_id, field))
        || (object_type == "storyElementOccurrence"
            && occurrence_belongs_to_selected_element(tx, scope, project_id, object_id))
    {
        "allowed".into()
    } else {
        "requires_confirmation".into()
    }
}

fn occurrence_belongs_to_selected_element(
    tx: &Transaction<'_>,
    scope: &WriteScope,
    project_id: &str,
    occurrence_id: &str,
) -> bool {
    let element_id = tx
        .query_row(
            "SELECT story_element_id FROM story_element_occurrences WHERE id=?1",
            [occurrence_id],
            |row| row.get::<_, String>(0),
        )
        .ok();
    element_id.is_some_and(|element_id| {
        scope.refs.iter().any(|reference| {
            reference.project_id == project_id
                && reference.object_type == "storyElement"
                && reference.object_id == element_id
                && reference.field.is_none()
        })
    })
}

fn reference_matches(
    reference: &ObjectRef,
    project_id: &str,
    object_type: &str,
    object_id: &str,
    field: &str,
) -> bool {
    reference.project_id == project_id
        && reference.object_type == object_type
        && reference.object_id == object_id
        && reference
            .field
            .as_deref()
            .is_none_or(|allowed| allowed == field)
}

fn insert_permission_card(
    tx: &Transaction<'_>,
    proposal_id: &str,
    task_id: &str,
    project_id: &str,
    scope: &WriteScope,
    items: &[(String, i64, PatchItemInput, String)],
) -> AppResult<()> {
    let requested = items
        .iter()
        .filter(|item| item.3 == "requires_confirmation")
        .map(|item| json!({"itemId": item.0, "objectType": item.2.object_type, "objectId": item.2.object_id, "field": item.2.field_name, "reason": item.2.reason}))
        .collect::<Vec<_>>();
    let first = items
        .iter()
        .find(|item| item.3 == "requires_confirmation")
        .map(|item| json!({"projectId": project_id, "objectType": item.2.object_type, "objectId": item.2.object_id, "field": item.2.field_name}));
    let options = json!({
        "proposalId": proposal_id,
        "currentWriteScope": scope,
        "requestedScope": requested,
        "oneTimeOnly": true,
        "impact": "批准项将通过一个可撤销的 Agent ChangeSet 写入项目事实",
        "actions": ["allow_once", "keep_current", "change_scope", "discuss"]
    });
    tx.execute(
        "INSERT INTO ai_cards (id, task_id, card_type, related_ref_json, title, body, options_json, status, created_at) VALUES (?1, ?2, 'permission', ?3, '需要扩大本次写入范围', '部分修改超出当前授权范围，请逐项确认。', ?4, 'open', ?5)",
        params![permission_card_id(proposal_id), task_id, first.map(|value| value.to_string()), options.to_string(), now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn validate_permission_card(
    tx: &Transaction<'_>,
    card_id: Option<&str>,
    task_id: &str,
    proposal_id: &str,
) -> AppResult<()> {
    let expected = permission_card_id(proposal_id);
    if card_id != Some(expected.as_str()) {
        return Err("WRITE_SCOPE_DENIED: 缺少对应的权限申请卡确认".into());
    }
    let row: Option<(String, String, String)> = tx
        .query_row(
            "SELECT task_id, status, options_json FROM ai_cards WHERE id=?1 AND card_type='permission'",
            [expected],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((card_task_id, status, options_json)) = row else {
        return Err("WRITE_SCOPE_DENIED: 权限申请卡不存在".into());
    };
    let options: Value = serde_json::from_str(&options_json).map_err(|e| e.to_string())?;
    if card_task_id != task_id || status != "open" || options["proposalId"] != proposal_id {
        return Err("WRITE_SCOPE_DENIED: 权限申请卡不属于当前提案或已处理".into());
    }
    Ok(())
}

fn resolve_permission_card(
    tx: &Transaction<'_>,
    card_id: &str,
    approved_ids: &[String],
    rejected_ids: &[String],
) -> AppResult<()> {
    tx.execute(
        "UPDATE ai_cards SET status='resolved', resolution_json=?1, resolved_at=?2 WHERE id=?3 AND status='open'",
        params![json!({"approvedItemIds": approved_ids, "rejectedItemIds": rejected_ids, "oneTimeOnly": true}).to_string(), now(), card_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn mark_stale_and_finish(
    tx: Transaction<'_>,
    proposal_id: &str,
    revision: i64,
    reason: &str,
) -> AppResult<ApplyPatchResponse> {
    tx.execute(
        "UPDATE patch_items SET permission_state='stale', apply_state='stale' WHERE proposal_id=?1 AND apply_state='pending'",
        [proposal_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE patch_proposals SET status='stale', updated_at=?1 WHERE id=?2",
        params![now(), proposal_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE ai_cards SET status='resolved', resolution_json=?1, resolved_at=?2 WHERE id=?3 AND status='open'",
        params![json!({"stale": true, "reason": reason}).to_string(), now(), permission_card_id(proposal_id)],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Err(format!(
        "REVISION_STALE: {reason}；当前 revision {revision}"
    ))
}

fn load_proposal(conn: &rusqlite::Connection, proposal_id: &str) -> AppResult<PatchProposal> {
    let mut proposal = conn
        .query_row(
            "SELECT id, task_id, base_revision, title, status, created_at, updated_at FROM patch_proposals WHERE id=?1",
            [proposal_id],
            |row| {
                Ok(PatchProposal {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    base_revision: row.get(2)?,
                    title: row.get(3)?,
                    status: row.get(4)?,
                    items: Vec::new(),
                    permission_card_id: None,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .map_err(|_| "OBJECT_NOT_FOUND: 修改提案不存在".to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, object_type, object_id, field_name, old_value_json, new_value_json, reason, permission_state, apply_state, sort_order FROM patch_items WHERE proposal_id=?1 ORDER BY sort_order, id")
        .map_err(|e| e.to_string())?;
    proposal.items = stmt
        .query_map([proposal_id], |row| {
            let old_json: String = row.get(4)?;
            let new_json: String = row.get(5)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                old_json,
                new_json,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .map(|row| {
            let row = row.map_err(|e| e.to_string())?;
            Ok(PatchItem {
                id: row.0,
                object_type: row.1,
                object_id: row.2,
                field_name: row.3,
                old_value: serde_json::from_str(&row.4).map_err(|e| e.to_string())?,
                new_value: serde_json::from_str(&row.5).map_err(|e| e.to_string())?,
                reason: row.6,
                permission_state: row.7,
                apply_state: row.8,
                sort_order: row.9,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let card_id = permission_card_id(proposal_id);
    let has_card: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM ai_cards WHERE id=?1)",
            [&card_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    proposal.permission_card_id = has_card.then_some(card_id);
    Ok(proposal)
}

fn load_card(conn: &rusqlite::Connection, card_id: &str) -> AppResult<AiCard> {
    let row = conn
        .query_row(
            "SELECT id, task_id, card_type, related_ref_json, title, body, options_json, status, resolution_json, created_at, resolved_at FROM ai_cards WHERE id=?1",
            [card_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<String>>(10)?,
                ))
            },
        )
        .map_err(|_| "OBJECT_NOT_FOUND: AI Card 不存在".to_string())?;
    Ok(AiCard {
        id: row.0,
        task_id: row.1,
        card_type: row.2,
        related_ref: row
            .3
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|e| e.to_string())?,
        title: row.4,
        body: row.5,
        options: row
            .6
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| Value::Array(Vec::new())),
        status: row.7,
        resolution: row
            .8
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|e| e.to_string())?,
        created_at: row.9,
        resolved_at: row.10,
    })
}

fn list_cards(conn: &rusqlite::Connection, task_id: &str) -> AppResult<Vec<AiCard>> {
    let mut stmt = conn
        .prepare("SELECT id FROM ai_cards WHERE task_id=?1 ORDER BY created_at, id")
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([task_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    ids.iter().map(|id| load_card(conn, id)).collect()
}

fn permission_card_id(proposal_id: &str) -> String {
    format!("{proposal_id}:permission")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{init_database, new_id};
    use crate::mutation::{apply_mutation, MutationRequest};

    fn fixture() -> (tempfile::TempDir, String, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "权限测试", "short").unwrap();
        let shot_id = new_id();
        let conn = open_database(temp.path()).unwrap();
        conn.execute("INSERT INTO content_units (id, project_id, type, name, sort_order, created_at, updated_at) VALUES ('unit', ?1, 'short', '正片', 0, ?2, ?2)", params![project.id, now()]).unwrap();
        conn.execute("INSERT INTO scripts (id, content_unit_id, title, created_at, updated_at) VALUES ('script', 'unit', '正片', ?1, ?1)", [now()]).unwrap();
        conn.execute("INSERT INTO scenes (id, script_id, title, sort_order, created_at, updated_at) VALUES ('scene', 'script', '场01', 0, ?1, ?1)", [now()]).unwrap();
        conn.execute("INSERT INTO shots (id, scene_id, sort_order, title, composition, action, dialogue, created_at, updated_at) VALUES (?1, 'scene', 0, '镜头04', '旧构图', '保护动作', '旧对白', ?2, ?2)", params![shot_id, now()]).unwrap();
        let session_id = new_id();
        let task_id = new_id();
        let scope = WriteScope {
            refs: vec![ObjectRef {
                project_id: project.id.clone(),
                object_type: "shot".into(),
                object_id: shot_id.clone(),
                field: Some("composition".into()),
            }],
            protected_refs: vec![ObjectRef {
                project_id: project.id.clone(),
                object_type: "shot".into(),
                object_id: shot_id.clone(),
                field: Some("action".into()),
            }],
        };
        conn.execute("INSERT INTO agent_sessions (id, project_id, scope_type, scope_id, title, status, created_at, updated_at) VALUES (?1, ?2, 'shot', ?3, '测试', 'active', ?4, ?4)", params![session_id, project.id, shot_id, now()]).unwrap();
        conn.execute("INSERT INTO agent_tasks (id, session_id, task_type, agent_type, selection_json, read_scope_json, write_scope_json, context_revision, status, created_at) VALUES (?1, ?2, 'edit', 'photography', '{}', '{}', ?3, 0, 'waiting_for_user', ?4)", params![task_id, session_id, serde_json::to_string(&scope).unwrap(), now()]).unwrap();
        drop(conn);
        (temp, project.id, task_id, shot_id)
    }

    fn item(object_id: &str, field: &str, old: &str, new: &str) -> PatchItemInput {
        PatchItemInput {
            object_type: "shot".into(),
            object_id: object_id.into(),
            field_name: field.into(),
            old_value: Value::String(old.into()),
            new_value: Value::String(new.into()),
            reason: format!("修改 {field}"),
        }
    }

    #[test]
    fn selected_story_element_allows_only_its_occurrence_fields() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "故事结构权限", "series").unwrap();
        let mut conn = open_database(temp.path()).unwrap();
        conn.execute("INSERT INTO content_units (id, project_id, type, name, sort_order, created_at, updated_at) VALUES ('episode', ?1, 'episode', 'EP01', 0, ?2, ?2)", params![project.id, now()]).unwrap();
        conn.execute("INSERT INTO story_elements (id, project_id, type, name, created_at, updated_at) VALUES ('selected-line', ?1, 'mainline', '主线', ?2, ?2), ('other-line', ?1, 'foreshadow', '伏笔', ?2, ?2)", params![project.id, now()]).unwrap();
        conn.execute("INSERT INTO story_element_occurrences (id, story_element_id, content_unit_id, occurrence_type, created_at, updated_at) VALUES ('selected-node', 'selected-line', 'episode', '推进', ?1, ?1), ('other-node', 'other-line', 'episode', '埋下', ?1, ?1)", [now()]).unwrap();
        let tx = conn.transaction().unwrap();
        let scope = WriteScope {
            refs: vec![ObjectRef {
                project_id: project.id.clone(),
                object_type: "storyElement".into(),
                object_id: "selected-line".into(),
                field: None,
            }],
            protected_refs: vec![],
        };
        assert_eq!(
            permission_for(
                &tx,
                &scope,
                &project.id,
                "storyElementOccurrence",
                "selected-node",
                "description"
            ),
            "allowed"
        );
        assert_eq!(
            permission_for(
                &tx,
                &scope,
                &project.id,
                "storyElementOccurrence",
                "other-node",
                "description"
            ),
            "requires_confirmation"
        );
    }

    #[test]
    fn classifies_scope_and_builds_diff_with_permission_card() {
        let (temp, _, task_id, shot_id) = fixture();
        let proposal = propose_patch(
            temp.path(),
            ProposePatchInput {
                request_id: "proposal-a".into(),
                task_id,
                base_revision: 0,
                title: "摄影建议".into(),
                items: vec![
                    item(&shot_id, "composition", "旧构图", "新构图"),
                    item(&shot_id, "dialogue", "旧对白", "新对白"),
                    item(&shot_id, "action", "保护动作", "越权动作"),
                ],
            },
        )
        .unwrap();
        assert_eq!(
            proposal
                .items
                .iter()
                .map(|item| item.permission_state.as_str())
                .collect::<Vec<_>>(),
            vec!["allowed", "requires_confirmation", "denied"]
        );
        assert_eq!(proposal.items[0].old_value, "旧构图");
        assert_eq!(proposal.items[0].new_value, "新构图");
        assert_eq!(
            proposal.permission_card_id.as_deref(),
            Some("proposal-a:permission")
        );
        let permission_card = load_card(
            &open_database(temp.path()).unwrap(),
            proposal.permission_card_id.as_deref().unwrap(),
        )
        .unwrap();
        assert_eq!(permission_card.card_type, "permission");
        assert_eq!(permission_card.status, "open");
        assert_eq!(permission_card.options["proposalId"], "proposal-a");
        assert_eq!(
            propose_patch(
                temp.path(),
                ProposePatchInput {
                    request_id: "proposal-a".into(),
                    task_id: proposal.task_id,
                    base_revision: 0,
                    title: "ignored".into(),
                    items: vec![item(&shot_id, "composition", "旧构图", "ignored")]
                }
            )
            .unwrap()
            .items[0]
                .new_value,
            "新构图"
        );
    }

    #[test]
    fn creates_idempotent_ai_card_and_resolves_non_permission_card() {
        let (temp, project_id, task_id, shot_id) = fixture();
        let input = CreateCardInput {
            request_id: "card-a".into(),
            task_id: task_id.clone(),
            card_type: "question".into(),
            related_ref: Some(ObjectRef {
                project_id,
                object_type: "shot".into(),
                object_id: shot_id,
                field: None,
            }),
            title: "需要确认".into(),
            body: "请选择方案".into(),
            options: json!([{"id": "a", "label": "方案 A"}]),
        };
        let card = create_card(temp.path(), input).unwrap();
        assert_eq!(card.status, "open");
        assert_eq!(card.options[0]["id"], "a");
        let duplicate = create_card(
            temp.path(),
            CreateCardInput {
                request_id: "card-a".into(),
                task_id,
                card_type: "question".into(),
                related_ref: None,
                title: "不会覆盖".into(),
                body: String::new(),
                options: Value::Null,
            },
        )
        .unwrap();
        assert_eq!(duplicate.title, "需要确认");
        let resolved = resolve_card(
            temp.path(),
            ResolveCardInput {
                card_id: card.id,
                status: "resolved".into(),
                resolution: json!({"optionId": "a"}),
            },
        )
        .unwrap();
        assert_eq!(resolved.status, "resolved");
        assert_eq!(resolved.resolution.unwrap()["optionId"], "a");
    }

    #[test]
    fn refuses_unconfirmed_out_of_scope_write_without_changing_facts() {
        let (temp, _, task_id, shot_id) = fixture();
        let proposal = propose_patch(
            temp.path(),
            ProposePatchInput {
                request_id: "proposal-b".into(),
                task_id,
                base_revision: 0,
                title: "对白建议".into(),
                items: vec![item(&shot_id, "dialogue", "旧对白", "新对白")],
            },
        )
        .unwrap();
        let error = apply_patch(
            temp.path(),
            ApplyPatchInput {
                proposal_id: proposal.id,
                approved_item_ids: vec![],
                rejected_item_ids: vec![],
                permission_card_id: None,
            },
        )
        .unwrap_err();
        assert!(error.contains("WRITE_SCOPE_DENIED"));
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            read_field_value(
                &conn.unchecked_transaction().unwrap(),
                "shot",
                &shot_id,
                "dialogue"
            )
            .unwrap(),
            "旧对白"
        );
    }

    #[test]
    fn applies_approved_batch_once_and_never_applies_protected_field() {
        let (temp, _, task_id, shot_id) = fixture();
        let proposal = propose_patch(
            temp.path(),
            ProposePatchInput {
                request_id: "proposal-c".into(),
                task_id,
                base_revision: 0,
                title: "摄影批量建议".into(),
                items: vec![
                    item(&shot_id, "composition", "旧构图", "新构图"),
                    item(&shot_id, "dialogue", "旧对白", "新对白"),
                    item(&shot_id, "action", "保护动作", "越权动作"),
                ],
            },
        )
        .unwrap();
        let dialogue_id = proposal.items[1].id.clone();
        let result = apply_patch(
            temp.path(),
            ApplyPatchInput {
                proposal_id: proposal.id,
                approved_item_ids: vec![dialogue_id],
                rejected_item_ids: vec![],
                permission_card_id: proposal.permission_card_id,
            },
        )
        .unwrap();
        assert_eq!(result.status, "applied");
        assert_eq!(result.revision, 1);
        assert_eq!(result.applied_item_ids.len(), 2);
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT composition FROM shots WHERE id=?1",
                [&shot_id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "新构图"
        );
        assert_eq!(
            conn.query_row(
                "SELECT dialogue FROM shots WHERE id=?1",
                [&shot_id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "新对白"
        );
        assert_eq!(
            conn.query_row("SELECT action FROM shots WHERE id=?1", [&shot_id], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
            "保护动作"
        );
        assert_eq!(
            conn.query_row(
                "SELECT source_type FROM change_sets WHERE id=?1",
                [result.change_set_id.unwrap()],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "agent"
        );
    }

    #[test]
    fn stale_revision_marks_proposal_and_writes_nothing() {
        let (temp, project_id, task_id, shot_id) = fixture();
        let proposal = propose_patch(
            temp.path(),
            ProposePatchInput {
                request_id: "proposal-d".into(),
                task_id,
                base_revision: 0,
                title: "过期建议".into(),
                items: vec![item(&shot_id, "composition", "旧构图", "新构图")],
            },
        )
        .unwrap();
        apply_mutation(
            temp.path().to_string_lossy().into_owned(),
            MutationRequest {
                action: "patch".into(),
                entity_type: "project".into(),
                object_id: Some(project_id),
                values: Map::from_iter([(
                    String::from("description"),
                    Value::String("手工修改".into()),
                )]),
                change_set_id: None,
                change_set_name: Some("手工修改".into()),
                source_type: None,
                source_id: None,
            },
        )
        .unwrap();
        let error = apply_patch(
            temp.path(),
            ApplyPatchInput {
                proposal_id: proposal.id.clone(),
                approved_item_ids: vec![],
                rejected_item_ids: vec![],
                permission_card_id: None,
            },
        )
        .unwrap_err();
        assert!(error.contains("REVISION_STALE"));
        let loaded = load_proposal(&open_database(temp.path()).unwrap(), &proposal.id).unwrap();
        assert_eq!(loaded.status, "stale");
        assert_eq!(loaded.items[0].apply_state, "stale");
    }

    #[test]
    fn old_value_mismatch_marks_proposal_stale() {
        let (temp, _, task_id, shot_id) = fixture();
        let proposal = propose_patch(
            temp.path(),
            ProposePatchInput {
                request_id: "proposal-e".into(),
                task_id,
                base_revision: 0,
                title: "旧值建议".into(),
                items: vec![item(&shot_id, "composition", "旧构图", "新构图")],
            },
        )
        .unwrap();
        let conn = open_database(temp.path()).unwrap();
        conn.execute(
            "UPDATE shots SET composition='旁路修改' WHERE id=?1",
            [&shot_id],
        )
        .unwrap();
        drop(conn);
        assert!(apply_patch(
            temp.path(),
            ApplyPatchInput {
                proposal_id: proposal.id.clone(),
                approved_item_ids: vec![],
                rejected_item_ids: vec![],
                permission_card_id: None
            }
        )
        .unwrap_err()
        .contains("REVISION_STALE"));
        assert_eq!(
            load_proposal(&open_database(temp.path()).unwrap(), &proposal.id)
                .unwrap()
                .status,
            "stale"
        );
    }

    #[test]
    fn rejecting_permission_request_never_writes_project_fact() {
        let (temp, _, task_id, shot_id) = fixture();
        let proposal = propose_patch(
            temp.path(),
            ProposePatchInput {
                request_id: "proposal-f".into(),
                task_id,
                base_revision: 0,
                title: "拒绝建议".into(),
                items: vec![item(&shot_id, "dialogue", "旧对白", "新对白")],
            },
        )
        .unwrap();
        let rejected = reject_patch(temp.path(), &proposal.id).unwrap();
        assert_eq!(rejected.status, "rejected");
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT dialogue FROM shots WHERE id=?1",
                [&shot_id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "旧对白"
        );
        assert_eq!(
            conn.query_row("SELECT revision FROM projects LIMIT 1", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn refuses_object_outside_multi_selection() {
        let (temp, _, task_id, _) = fixture();
        let conn = open_database(temp.path()).unwrap();
        conn.execute("INSERT INTO shots (id, scene_id, sort_order, title, composition, created_at, updated_at) VALUES ('unselected', 'scene', 1, '未选镜头', '未选构图', ?1, ?1)", [now()]).unwrap();
        drop(conn);
        let proposal = propose_patch(
            temp.path(),
            ProposePatchInput {
                request_id: "proposal-g".into(),
                task_id,
                base_revision: 0,
                title: "多选越权".into(),
                items: vec![item("unselected", "composition", "未选构图", "越权构图")],
            },
        )
        .unwrap();
        assert_eq!(proposal.items[0].permission_state, "requires_confirmation");
        assert!(apply_patch(
            temp.path(),
            ApplyPatchInput {
                proposal_id: proposal.id,
                approved_item_ids: vec![],
                rejected_item_ids: vec![],
                permission_card_id: None,
            },
        )
        .unwrap_err()
        .contains("WRITE_SCOPE_DENIED"));
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT composition FROM shots WHERE id='unselected'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "未选构图"
        );
    }

    #[test]
    fn deleted_object_marks_proposal_stale() {
        let (temp, _, task_id, shot_id) = fixture();
        let proposal = propose_patch(
            temp.path(),
            ProposePatchInput {
                request_id: "proposal-h".into(),
                task_id,
                base_revision: 0,
                title: "删除后过期".into(),
                items: vec![item(&shot_id, "composition", "旧构图", "新构图")],
            },
        )
        .unwrap();
        let conn = open_database(temp.path()).unwrap();
        conn.execute("DELETE FROM shots WHERE id=?1", [&shot_id])
            .unwrap();
        drop(conn);
        assert!(apply_patch(
            temp.path(),
            ApplyPatchInput {
                proposal_id: proposal.id.clone(),
                approved_item_ids: vec![],
                rejected_item_ids: vec![],
                permission_card_id: None,
            },
        )
        .unwrap_err()
        .contains("REVISION_STALE"));
        assert_eq!(
            load_proposal(&open_database(temp.path()).unwrap(), &proposal.id)
                .unwrap()
                .status,
            "stale"
        );
    }

    #[test]
    fn changed_write_scope_marks_proposal_stale() {
        let (temp, project_id, task_id, shot_id) = fixture();
        let proposal = propose_patch(
            temp.path(),
            ProposePatchInput {
                request_id: "proposal-i".into(),
                task_id: task_id.clone(),
                base_revision: 0,
                title: "权限变化".into(),
                items: vec![item(&shot_id, "composition", "旧构图", "新构图")],
            },
        )
        .unwrap();
        let changed_scope = WriteScope {
            refs: vec![ObjectRef {
                project_id,
                object_type: "shot".into(),
                object_id: shot_id,
                field: Some("dialogue".into()),
            }],
            protected_refs: vec![],
        };
        let conn = open_database(temp.path()).unwrap();
        conn.execute(
            "UPDATE agent_tasks SET write_scope_json=?1 WHERE id=?2",
            params![serde_json::to_string(&changed_scope).unwrap(), task_id],
        )
        .unwrap();
        drop(conn);
        assert!(apply_patch(
            temp.path(),
            ApplyPatchInput {
                proposal_id: proposal.id.clone(),
                approved_item_ids: vec![],
                rejected_item_ids: vec![],
                permission_card_id: None,
            },
        )
        .unwrap_err()
        .contains("REVISION_STALE"));
        assert_eq!(
            load_proposal(&open_database(temp.path()).unwrap(), &proposal.id)
                .unwrap()
                .items[0]
                .permission_state,
            "stale"
        );
    }
}
