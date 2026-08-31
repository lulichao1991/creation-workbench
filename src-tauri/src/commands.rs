use crate::database::{
    descriptor_from_conn, init_database, new_id, now, open_database, project_state, AppResult,
    ProjectDescriptor,
};
use base64::Engine;
use rusqlite::params;
use serde::Serialize;
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
    conn.execute(
        "UPDATE projects SET name=?1, revision=0, updated_at=?2",
        params![new_name.trim(), now()],
    )
    .map_err(|e| format!("更新项目副本失败：{e}"))?;
    conn.execute("DELETE FROM changes", [])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM change_sets", [])
        .map_err(|e| e.to_string())?;
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
        assert_eq!(
            list_projects(temp.path().to_string_lossy().to_string())
                .unwrap()
                .len(),
            1
        );
        let copy = copy_project(project.path, "智斗游戏 副本".into()).unwrap();
        assert!(Path::new(&copy.path).join("project.db").is_file());
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
}
