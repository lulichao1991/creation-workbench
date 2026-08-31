use crate::app_database::load_feature_flags;
use crate::database::{new_id, now, open_database, AppResult};
use rusqlite::params;
use serde::Deserialize;
use std::path::Path;
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveGraphLayoutInput {
    scope_type: String,
    scope_id: String,
    view_type: String,
    filter_json: String,
    layout_json: String,
}

fn validate_layout(input: &SaveGraphLayoutInput) -> AppResult<()> {
    if !["project", "contentUnit"].contains(&input.scope_type.as_str()) {
        return Err("不支持的关系图范围".into());
    }
    if !["timeline", "graph", "episodes"].contains(&input.view_type.as_str()) {
        return Err("不支持的关系图视图".into());
    }
    if input.scope_id.trim().is_empty() {
        return Err("关系图范围不能为空".into());
    }
    serde_json::from_str::<serde_json::Value>(&input.filter_json)
        .map_err(|_| "关系图筛选条件不是有效 JSON".to_string())?;
    serde_json::from_str::<serde_json::Value>(&input.layout_json)
        .map_err(|_| "关系图布局不是有效 JSON".to_string())?;
    Ok(())
}

fn save_layout(project_path: &Path, input: SaveGraphLayoutInput) -> AppResult<String> {
    validate_layout(&input)?;
    let conn = open_database(project_path)?;
    let id = new_id();
    conn.execute(
        "INSERT INTO graph_layouts (id, scope_type, scope_id, view_type, filter_json, layout_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(scope_type, scope_id, view_type) DO UPDATE SET
           filter_json=excluded.filter_json, layout_json=excluded.layout_json, updated_at=excluded.updated_at",
        params![id, input.scope_type, input.scope_id, input.view_type, input.filter_json, input.layout_json, now()],
    )
    .map_err(|e| format!("保存关系图布局失败：{e}"))?;
    conn.query_row(
        "SELECT id FROM graph_layouts WHERE scope_type=?1 AND scope_id=?2 AND view_type=?3",
        params![input.scope_type, input.scope_id, input.view_type],
        |row| row.get(0),
    )
    .map_err(|e| format!("读取关系图布局失败：{e}"))
}

fn reset_layout(
    project_path: &Path,
    scope_type: &str,
    scope_id: &str,
    view_type: &str,
) -> AppResult<()> {
    let input = SaveGraphLayoutInput {
        scope_type: scope_type.into(),
        scope_id: scope_id.into(),
        view_type: view_type.into(),
        filter_json: "{}".into(),
        layout_json: "{}".into(),
    };
    validate_layout(&input)?;
    let conn = open_database(project_path)?;
    conn.execute(
        "DELETE FROM graph_layouts WHERE scope_type=?1 AND scope_id=?2 AND view_type=?3",
        params![scope_type, scope_id, view_type],
    )
    .map_err(|e| format!("重置关系图布局失败：{e}"))?;
    Ok(())
}

fn ensure_enabled(app: &tauri::AppHandle) -> AppResult<()> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    if load_feature_flags(&app_data_dir)?.get("story_graph") == Some(&true) {
        Ok(())
    } else {
        Err("高级作品结构尚未启用".into())
    }
}

#[tauri::command]
pub fn graph_layout_save(
    app: tauri::AppHandle,
    project_path: String,
    input: SaveGraphLayoutInput,
) -> AppResult<String> {
    ensure_enabled(&app)?;
    save_layout(Path::new(&project_path), input)
}

#[tauri::command]
pub fn graph_layout_reset(
    app: tauri::AppHandle,
    project_path: String,
    scope_type: String,
    scope_id: String,
    view_type: String,
) -> AppResult<()> {
    ensure_enabled(&app)?;
    reset_layout(Path::new(&project_path), &scope_type, &scope_id, &view_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::init_database;

    fn input(layout: &str) -> SaveGraphLayoutInput {
        SaveGraphLayoutInput {
            scope_type: "project".into(),
            scope_id: "project".into(),
            view_type: "graph".into(),
            filter_json: r#"{"focus":"foreshadow"}"#.into(),
            layout_json: layout.into(),
        }
    }

    #[test]
    fn layout_upsert_does_not_change_project_revision() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "测试", "series").unwrap();
        let first = save_layout(temp.path(), input(r#"{"zoom":1}"#)).unwrap();
        let second = save_layout(temp.path(), input(r#"{"zoom":2}"#)).unwrap();
        assert_eq!(first, second);
        let conn = open_database(temp.path()).unwrap();
        let revision: i64 = conn
            .query_row(
                "SELECT revision FROM projects WHERE id=?1",
                [project.id],
                |row| row.get(0),
            )
            .unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM graph_layouts", [], |row| row.get(0))
            .unwrap();
        let layout: String = conn
            .query_row("SELECT layout_json FROM graph_layouts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(revision, 0);
        assert_eq!(count, 1);
        assert_eq!(layout, r#"{"zoom":2}"#);
    }

    #[test]
    fn layout_rejects_unknown_view_and_reset_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        init_database(temp.path(), "测试", "series").unwrap();
        let mut bad = input("{}");
        bad.view_type = "unbounded".into();
        assert!(save_layout(temp.path(), bad).is_err());
        reset_layout(temp.path(), "project", "project", "graph").unwrap();
        reset_layout(temp.path(), "project", "project", "graph").unwrap();
    }
}
