use crate::database::{
    descriptor_from_conn, init_database, new_id, now, open_database, project_state, AppResult,
    ProjectDescriptor, AGENT_TABLES,
};
use base64::Engine;
use rusqlite::params;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub default_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaData {
    pub mime_type: String,
    pub data: String,
}

#[tauri::command]
pub fn get_default_workspace() -> AppResult<WorkspaceInfo> {
    let documents = dirs::document_dir().ok_or_else(|| "无法定位文档目录".to_string())?;
    let root = documents.join("AI视频工作台");
    fs::create_dir_all(&root).map_err(|e| format!("创建默认项目目录失败：{e}"))?;
    Ok(WorkspaceInfo {
        default_path: root.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn list_projects(root_path: String) -> AppResult<Vec<ProjectDescriptor>> {
    let root = PathBuf::from(root_path);
    if !root.exists() {
        fs::create_dir_all(&root).map_err(|e| format!("创建项目根目录失败：{e}"))?;
    }
    let mut projects = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| format!("读取项目目录失败：{e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() && path.join("project.db").is_file() {
            if let Ok(conn) = open_database(&path) {
                if let Ok(project) = descriptor_from_conn(&conn, &path) {
                    projects.push(project);
                }
            }
        }
    }
    projects.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(projects)
}

#[tauri::command]
pub fn create_project(
    root_path: String,
    name: String,
    structure_type: String,
) -> AppResult<ProjectDescriptor> {
    let clean_name = sanitize_name(&name);
    if clean_name.is_empty() {
        return Err("项目名称不能为空".into());
    }
    let project_path = unique_project_path(Path::new(&root_path), &clean_name);
    fs::create_dir_all(&project_path).map_err(|e| format!("创建项目目录失败：{e}"))?;
    for directory in [
        "assets/characters",
        "assets/locations",
        "assets/props",
        "keyframes",
        "references",
        "imports",
        "exports",
        "cache",
        "backups",
    ] {
        fs::create_dir_all(project_path.join(directory))
            .map_err(|e| format!("创建项目子目录失败：{e}"))?;
    }
    match init_database(&project_path, name.trim(), &structure_type) {
        Ok(project) => Ok(project),
        Err(error) => {
            let _ = fs::remove_dir_all(&project_path);
            Err(error)
        }
    }
}

#[tauri::command]
pub fn open_project(project_path: String) -> AppResult<ProjectDescriptor> {
    let path = PathBuf::from(project_path);
    let conn = open_database(&path)?;
    descriptor_from_conn(&conn, &path)
}

#[tauri::command]
pub fn copy_project(project_path: String, new_name: String) -> AppResult<ProjectDescriptor> {
    let source = validate_project_path(&project_path)?;
    let parent = source
        .parent()
        .ok_or_else(|| "项目目录没有父目录".to_string())?;
    let clean_name = sanitize_name(&new_name);
    if clean_name.is_empty() {
        return Err("副本名称不能为空".into());
    }
    let target = unique_project_path(parent, &clean_name);
    copy_directory(&source, &target)?;
    let conn = open_database(&target)?;
    let old_project_id: String = conn
        .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let new_project_id = new_id();
    let result = (|| -> AppResult<()> {
        conn.execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;")
            .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE projects SET id=?1, name=?2, revision=0, updated_at=?3 WHERE id=?4",
            params![new_project_id, new_name.trim(), now(), old_project_id],
        )
        .map_err(|e| format!("更新项目副本失败：{e}"))?;
        for table in ["content_units", "assets", "relations", "story_elements"] {
            conn.execute(
                &format!("UPDATE {table} SET project_id=?1 WHERE project_id=?2"),
                params![new_project_id, old_project_id],
            )
            .map_err(|e| e.to_string())?;
        }
        for table in AGENT_TABLES {
            conn.execute(&format!("DELETE FROM {table}"), [])
                .map_err(|e| e.to_string())?;
        }
        conn.execute(
            "UPDATE graph_layouts SET scope_id=?1 WHERE scope_type='project' AND scope_id=?2",
            params![new_project_id, old_project_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE project_memories SET scope_id=?1 WHERE scope_type='project' AND scope_id=?2",
            params![new_project_id, old_project_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM snapshots", [])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM changes", [])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM change_sets", [])
            .map_err(|e| e.to_string())?;
        conn.execute_batch("COMMIT; PRAGMA foreign_keys=ON;")
            .map_err(|e| e.to_string())?;
        let violations: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .map_err(|e| e.to_string())?;
        if violations != 0 {
            return Err("复制后的项目存在外键错误".into());
        }
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if integrity != "ok" {
            return Err(format!("复制后的项目完整性检查失败：{integrity}"));
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = conn.execute_batch("ROLLBACK; PRAGMA foreign_keys=ON;");
        let _ = fs::remove_dir_all(&target);
        return Err(error);
    }
    descriptor_from_conn(&conn, &target)
}

#[tauri::command]
pub fn delete_project(project_path: String) -> AppResult<()> {
    let path = validate_project_path(&project_path)?;
    let db = path.join("project.db");
    if !db.is_file() {
        return Err("拒绝删除：目标不是有效的工作台项目".into());
    }
    fs::remove_dir_all(&path).map_err(|e| format!("删除项目失败：{e}"))
}

#[tauri::command]
pub fn load_project_state(project_path: String) -> AppResult<serde_json::Value> {
    let conn = open_database(Path::new(&project_path))?;
    project_state(&conn)
}

#[tauri::command]
pub fn import_project_file(
    project_path: String,
    source_path: String,
    category: String,
) -> AppResult<String> {
    let project = validate_project_path(&project_path)?;
    let source = PathBuf::from(source_path);
    if !source.is_file() {
        return Err("所选文件不存在".into());
    }
    let target_dir = match category.as_str() {
        "character" => "assets/characters",
        "location" => "assets/locations",
        "prop" => "assets/props",
        "keyframe" => "keyframes",
        "reference" => "references",
        _ => return Err("不支持的导入分类".into()),
    };
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let filename = format!("{}.{}", new_id(), extension);
    let relative = PathBuf::from(target_dir).join(filename);
    let target = project.join(&relative);
    fs::copy(&source, &target).map_err(|e| format!("复制导入文件失败：{e}"))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

#[tauri::command]
pub fn read_project_media(project_path: String, relative_path: String) -> AppResult<MediaData> {
    let project = validate_project_path(&project_path)?;
    let project_canonical = project.canonicalize().map_err(|e| e.to_string())?;
    let media = project.join(relative_path);
    let media_canonical = media
        .canonicalize()
        .map_err(|e| format!("媒体文件不存在：{e}"))?;
    if !media_canonical.starts_with(&project_canonical) {
        return Err("拒绝读取项目目录之外的文件".into());
    }
    let bytes = fs::read(&media_canonical).map_err(|e| format!("读取媒体失败：{e}"))?;
    let mime_type = mime_from_path(&media_canonical);
    Ok(MediaData {
        mime_type,
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

#[tauri::command]
pub fn cleanup_project_media(project_path: String) -> AppResult<usize> {
    let project = validate_project_path(&project_path)?;
    let conn = open_database(&project)?;
    let mut referenced = HashSet::new();
    for sql in [
        "SELECT file_path FROM asset_media WHERE file_path IS NOT NULL AND file_path <> ''",
        "SELECT file_path FROM keyframes WHERE file_path IS NOT NULL AND file_path <> ''",
        "SELECT file_path FROM image_generation_results WHERE selection_state <> 'deleted'",
    ] {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let paths = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for path in paths {
            referenced.insert(path.map_err(|e| e.to_string())?.replace('\\', "/"));
        }
    }
    let mut snapshot_stmt = conn
        .prepare("SELECT snapshot_json FROM snapshots")
        .map_err(|e| e.to_string())?;
    let snapshots = snapshot_stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    for snapshot in snapshots {
        let raw = snapshot.map_err(|e| e.to_string())?;
        let value: Value =
            serde_json::from_str(&raw).map_err(|e| format!("快照数据损坏，已停止媒体清理：{e}"))?;
        collect_file_paths(&value, &mut referenced);
    }
    drop(snapshot_stmt);

    let mut history_stmt = conn
        .prepare(
            "SELECT changes.field_name, changes.old_value FROM changes JOIN change_sets ON change_sets.id=changes.change_set_id WHERE change_sets.status<>'undone' AND changes.old_value IS NOT NULL",
        )
        .map_err(|e| e.to_string())?;
    let history = history_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    for row in history {
        let (field_name, raw) = row.map_err(|e| e.to_string())?;
        let value: Value =
            serde_json::from_str(&raw).map_err(|e| format!("变更历史损坏，已停止媒体清理：{e}"))?;
        if field_name == "file_path" {
            if let Some(path) = value.as_str() {
                protect_file_path(path, &mut referenced);
            }
        } else if field_name == "__deleted__" {
            collect_file_paths(&value, &mut referenced);
        }
    }
    let mut removed = 0;
    for directory in [
        "assets/characters",
        "assets/locations",
        "assets/props",
        "keyframes",
        "candidates/images",
    ] {
        removed += remove_unreferenced_files(&project, &project.join(directory), &referenced)?;
    }
    Ok(removed)
}

fn protect_file_path(path: &str, referenced: &mut HashSet<String>) {
    if !path.is_empty() {
        referenced.insert(path.replace('\\', "/"));
    }
}

fn collect_file_paths(value: &Value, referenced: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(path) = object.get("file_path").and_then(Value::as_str) {
                protect_file_path(path, referenced);
            }
            for child in object.values() {
                collect_file_paths(child, referenced);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_file_paths(child, referenced);
            }
        }
        _ => {}
    }
}

fn remove_unreferenced_files(
    project: &Path,
    directory: &Path,
    referenced: &HashSet<String>,
) -> AppResult<usize> {
    if !directory.is_dir() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            removed += remove_unreferenced_files(project, &path, referenced)?;
        } else {
            let relative = path
                .strip_prefix(project)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if !referenced.contains(&relative) {
                fs::remove_file(&path).map_err(|e| format!("清理孤立媒体失败：{e}"))?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

fn validate_project_path(value: &str) -> AppResult<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_dir() || !path.join("project.db").is_file() {
        return Err("目标不是有效的工作台项目目录".into());
    }
    path.canonicalize().map_err(|e| e.to_string())
}

fn sanitize_name(name: &str) -> String {
    name.trim()
        .chars()
        .map(|ch| if r#"<>:"/\|?*"#.contains(ch) { '_' } else { ch })
        .collect::<String>()
        .trim_matches(['.', ' '])
        .to_string()
}

fn unique_project_path(root: &Path, name: &str) -> PathBuf {
    let first = root.join(name);
    if !first.exists() {
        return first;
    }
    for index in 2..10_000 {
        let candidate = root.join(format!("{name} ({index})"));
        if !candidate.exists() {
            return candidate;
        }
    }
    root.join(format!("{name}-{}", new_id()))
}

fn copy_directory(source: &Path, target: &Path) -> AppResult<()> {
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &target_path)?;
        } else if !matches!(
            source_path.extension().and_then(|v| v.to_str()),
            Some("wal") | Some("shm")
        ) {
            fs::copy(&source_path, &target_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn mime_from_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_lifecycle_creates_required_directories() {
        let temp = tempfile::tempdir().unwrap();
        let project = create_project(
            temp.path().to_string_lossy().to_string(),
            "智斗游戏".into(),
            "series".into(),
        )
        .unwrap();
        let path = PathBuf::from(project.path.clone());
        assert!(path.join("project.db").is_file());
        assert!(path.join("assets/characters").is_dir());
        assert!(path.join("keyframes").is_dir());
        assert!(path.join("backups").is_dir());
        assert_eq!(
            list_projects(temp.path().to_string_lossy().to_string())
                .unwrap()
                .len(),
            1
        );
        crate::mutation::create_snapshot(project.path.clone(), "复制前快照".into(), "".into())
            .unwrap();
        {
            let conn = open_database(&path).unwrap();
            let timestamp = now();
            conn.execute(
                "INSERT INTO agent_sessions (id, project_id, title, created_at, updated_at) VALUES ('session', ?1, '原项目会话', ?2, ?2)",
                params![project.id, timestamp],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO agent_messages (id, session_id, role, content, created_at) VALUES ('message', 'session', 'user', '测试', ?1)",
                [timestamp.clone()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO agent_tasks (id, session_id, task_type, agent_type, created_at) VALUES ('task', 'session', 'analyze', 'director', ?1)",
                [timestamp.clone()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO context_packages (id, task_id, project_revision, center_ref_json, checksum, created_at) VALUES ('context', 'task', 0, '{}', 'checksum', ?1)",
                [timestamp.clone()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO patch_proposals (id, task_id, base_revision, title, created_at, updated_at) VALUES ('proposal', 'task', 0, '建议', ?1, ?1)",
                [timestamp.clone()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO patch_items (id, proposal_id, object_type, object_id, field_name) VALUES ('patch', 'proposal', 'scene', 'scene', 'summary')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ai_cards (id, task_id, card_type, title, created_at) VALUES ('card', 'task', 'decision', '待确认', ?1)",
                [timestamp.clone()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO project_expert_overrides (id, project_id, expert_type, created_at, updated_at) VALUES ('override', ?1, 'director', ?2, ?2)",
                params![project.id, timestamp],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO story_elements (id, project_id, type, name, created_at, updated_at) VALUES ('mainline', ?1, 'mainline', '主线', ?2, ?2)",
                params![project.id, timestamp],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO graph_layouts (id, scope_type, scope_id, view_type, updated_at) VALUES ('layout', 'project', ?1, 'graph', ?2)",
                params![project.id, timestamp],
            )
            .unwrap();
        }
        let original_id = project.id;
        let copy = copy_project(project.path, "智斗游戏 副本".into()).unwrap();
        assert!(Path::new(&copy.path).join("project.db").is_file());
        assert_ne!(copy.id, original_id);
        let copy_conn = open_database(Path::new(&copy.path)).unwrap();
        let snapshot_count: i64 = copy_conn
            .query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(snapshot_count, 0);
        let copied_story_project: String = copy_conn
            .query_row(
                "SELECT project_id FROM story_elements WHERE id='mainline'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let copied_layout_scope: String = copy_conn
            .query_row(
                "SELECT scope_id FROM graph_layouts WHERE id='layout'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(copied_story_project, copy.id);
        assert_eq!(copied_layout_scope, copy.id);
        for table in crate::database::AGENT_TABLES {
            let count: i64 = copy_conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "copied Agent data from {table}");
        }
        drop(copy_conn);
        assert_eq!(
            list_projects(temp.path().to_string_lossy().to_string())
                .unwrap()
                .len(),
            2
        );
        delete_project(copy.path).unwrap();
        assert_eq!(
            list_projects(temp.path().to_string_lossy().to_string())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn cleanup_removes_only_unreferenced_managed_media() {
        let temp = tempfile::tempdir().unwrap();
        let project = create_project(
            temp.path().to_string_lossy().to_string(),
            "媒体清理".into(),
            "short".into(),
        )
        .unwrap();
        let root = PathBuf::from(&project.path);
        let orphan = root.join("assets/characters/orphan.png");
        let reference = root.join("references/keep.txt");
        fs::write(&orphan, b"orphan").unwrap();
        fs::write(&reference, b"keep").unwrap();
        let removed = cleanup_project_media(project.path).unwrap();
        assert_eq!(removed, 1);
        assert!(!orphan.exists());
        assert!(reference.exists());
    }

    #[test]
    fn cleanup_preserves_media_referenced_by_snapshots_and_undo_history() {
        let temp = tempfile::tempdir().unwrap();
        let project = create_project(
            temp.path().to_string_lossy().to_string(),
            "历史媒体保护".into(),
            "short".into(),
        )
        .unwrap();
        let root = PathBuf::from(&project.path);
        let snapshot_path = "assets/characters/from-snapshot.png";
        let history_path = "keyframes/from-history.png";
        let undone_path = "assets/props/from-undone-history.png";
        let orphan_path = "assets/locations/orphan.png";
        for path in [snapshot_path, history_path, undone_path, orphan_path] {
            fs::write(root.join(path), path.as_bytes()).unwrap();
        }

        let conn = open_database(&root).unwrap();
        let timestamp = now();
        conn.execute(
            "INSERT INTO snapshots (id, project_id, scope_type, name, description, revision, snapshot_json, created_at) VALUES (?1, ?2, 'project', '保护快照', '', 0, ?3, ?4)",
            params![new_id(), project.id, format!(r#"{{"asset_media":[{{"file_path":"{snapshot_path}"}}]}}"#), timestamp],
        )
        .unwrap();
        for (status, path) in [("closed", history_path), ("undone", undone_path)] {
            let set_id = new_id();
            conn.execute(
                "INSERT INTO change_sets (id, project_id, name, status, created_at) VALUES (?1, ?2, '删除媒体', ?3, ?4)",
                params![set_id, project.id, status, timestamp],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO changes (id, change_set_id, object_type, object_id, field_name, old_value, new_value, created_at) VALUES (?1, ?2, 'assetMedia', ?3, '__deleted__', ?4, 'null', ?5)",
                params![new_id(), set_id, new_id(), format!(r#"{{"file_path":"{path}"}}"#), timestamp],
            )
            .unwrap();
        }
        drop(conn);

        let removed = cleanup_project_media(project.path).unwrap();
        assert_eq!(removed, 2);
        assert!(root.join(snapshot_path).exists());
        assert!(root.join(history_path).exists());
        assert!(!root.join(undone_path).exists());
        assert!(!root.join(orphan_path).exists());
    }
}
