use chrono::Utc;
use rusqlite::{params, types::ValueRef, Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub type AppResult<T> = Result<T, String>;

pub const BUSINESS_TABLES: &[&str] = &[
    "content_units",
    "scripts",
    "scenes",
    "shots",
    "assets",
    "asset_media",
    "asset_requirements",
    "keyframes",
    "generation_tasks",
    "generation_task_shots",
    "relations",
];

pub const STATE_TABLES: &[&str] = &[
    "projects",
    "content_units",
    "scripts",
    "scenes",
    "shots",
    "assets",
    "asset_media",
    "asset_requirements",
    "keyframes",
    "generation_tasks",
    "generation_task_shots",
    "relations",
    "change_sets",
    "changes",
    "snapshots",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub structure_type: String,
    pub maturity: String,
    pub sync_status: String,
    pub revision: i64,
    pub path: String,
    pub updated_at: String,
}

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn db_path(project_path: &Path) -> PathBuf {
    project_path.join("project.db")
}

pub fn open_database(project_path: &Path) -> AppResult<Connection> {
    let path = db_path(project_path);
    if !path.is_file() {
        return Err(format!("项目数据库不存在：{}", path.display()));
    }
    let conn = Connection::open(&path).map_err(|e| format!("打开数据库失败：{e}"))?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
        .map_err(|e| format!("初始化数据库连接失败：{e}"))?;
    Ok(conn)
}

pub fn init_database(
    project_path: &Path,
    name: &str,
    structure_type: &str,
) -> AppResult<ProjectDescriptor> {
    let db = db_path(project_path);
    let conn = Connection::open(&db).map_err(|e| format!("创建数据库失败：{e}"))?;
    conn.execute_batch(SCHEMA)
        .map_err(|e| format!("初始化数据库结构失败：{e}"))?;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if count == 0 {
        let id = new_id();
        let timestamp = now();
        conn.execute(
            "INSERT INTO projects (id, name, description, structure_type, maturity, sync_status, revision, created_at, updated_at) VALUES (?1, ?2, '', ?3, 'exploring', 'normal', 0, ?4, ?4)",
            params![id, name, structure_type, timestamp],
        )
        .map_err(|e| format!("创建项目记录失败：{e}"))?;
    }
    descriptor_from_conn(&conn, project_path)
}

pub fn descriptor_from_conn(
    conn: &Connection,
    project_path: &Path,
) -> AppResult<ProjectDescriptor> {
    conn.query_row(
        "SELECT id, name, description, structure_type, maturity, sync_status, revision, updated_at FROM projects LIMIT 1",
        [],
        |row| {
            Ok(ProjectDescriptor {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                structure_type: row.get(3)?,
                maturity: row.get(4)?,
                sync_status: row.get(5)?,
                revision: row.get(6)?,
                path: project_path.to_string_lossy().to_string(),
                updated_at: row.get(7)?,
            })
        },
    )
    .map_err(|e| format!("读取项目失败：{e}"))
}

pub fn query_table_json(conn: &Connection, table: &str) -> AppResult<Vec<Value>> {
    if !STATE_TABLES.contains(&table) {
        return Err(format!("不允许读取数据表：{table}"));
    }
    let sql = format!("SELECT * FROM {table}");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let rows = stmt
        .query_map([], |row| row_to_json(row, &column_names))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn row_by_id(conn: &Connection, table: &str, id: &str) -> AppResult<Option<Value>> {
    if !STATE_TABLES.contains(&table) {
        return Err(format!("不允许读取数据表：{table}"));
    }
    let id_column = if table == "generation_task_shots" {
        "generation_task_id"
    } else {
        "id"
    };
    let sql = format!("SELECT * FROM {table} WHERE {id_column} = ?1 LIMIT 1");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    match stmt.query_row([id], |row| row_to_json(row, &column_names)) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn row_to_json(row: &Row<'_>, column_names: &[String]) -> rusqlite::Result<Value> {
    let mut object = Map::new();
    for (index, name) in column_names.iter().enumerate() {
        let value = match row.get_ref(index)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(v) => Value::from(v),
            ValueRef::Real(v) => Value::from(v),
            ValueRef::Text(v) => Value::String(String::from_utf8_lossy(v).to_string()),
            ValueRef::Blob(v) => Value::String(base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                v,
            )),
        };
        object.insert(name.clone(), value);
    }
    Ok(Value::Object(object))
}

pub fn project_state(conn: &Connection) -> AppResult<Value> {
    let mut state = Map::new();
    for table in STATE_TABLES {
        let key = snake_to_camel(table);
        state.insert(key, Value::Array(query_table_json(conn, table)?));
    }
    Ok(Value::Object(state))
}

fn snake_to_camel(value: &str) -> String {
    let mut result = String::new();
    let mut uppercase = false;
    for ch in value.chars() {
        if ch == '_' {
            uppercase = true;
        } else if uppercase {
            result.extend(ch.to_uppercase());
            uppercase = false;
        } else {
            result.push(ch);
        }
    }
    result
}

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  structure_type TEXT NOT NULL DEFAULT 'series',
  maturity TEXT NOT NULL DEFAULT 'exploring',
  sync_status TEXT NOT NULL DEFAULT 'normal',
  revision INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS content_units (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  parent_id TEXT,
  type TEXT NOT NULL,
  name TEXT NOT NULL,
  summary TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0,
  maturity TEXT NOT NULL DEFAULT 'exploring',
  sync_status TEXT NOT NULL DEFAULT 'normal',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id),
  FOREIGN KEY(parent_id) REFERENCES content_units(id)
);

CREATE TABLE IF NOT EXISTS scripts (
  id TEXT PRIMARY KEY,
  content_unit_id TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL DEFAULT '',
  summary TEXT NOT NULL DEFAULT '',
  maturity TEXT NOT NULL DEFAULT 'exploring',
  sync_status TEXT NOT NULL DEFAULT 'normal',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(content_unit_id) REFERENCES content_units(id)
);

CREATE TABLE IF NOT EXISTS scenes (
  id TEXT PRIMARY KEY,
  script_id TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0,
  location_text TEXT NOT NULL DEFAULT '',
  time_text TEXT NOT NULL DEFAULT '',
  summary TEXT NOT NULL DEFAULT '',
  content TEXT NOT NULL DEFAULT '',
  maturity TEXT NOT NULL DEFAULT 'exploring',
  sync_status TEXT NOT NULL DEFAULT 'normal',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(script_id) REFERENCES scripts(id)
);

CREATE TABLE IF NOT EXISTS shots (
  id TEXT PRIMARY KEY,
  scene_id TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  title TEXT NOT NULL DEFAULT '',
  duration REAL NOT NULL DEFAULT 0,
  narrative_purpose TEXT NOT NULL DEFAULT '',
  new_information TEXT NOT NULL DEFAULT '',
  shot_size TEXT NOT NULL DEFAULT '',
  camera_height TEXT NOT NULL DEFAULT '',
  camera_direction TEXT NOT NULL DEFAULT '',
  composition TEXT NOT NULL DEFAULT '',
  camera_movement TEXT NOT NULL DEFAULT '',
  subjects TEXT NOT NULL DEFAULT '',
  action TEXT NOT NULL DEFAULT '',
  dialogue TEXT NOT NULL DEFAULT '',
  environment TEXT NOT NULL DEFAULT '',
  start_state TEXT NOT NULL DEFAULT '',
  end_state TEXT NOT NULL DEFAULT '',
  maturity TEXT NOT NULL DEFAULT 'exploring',
  sync_status TEXT NOT NULL DEFAULT 'normal',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(scene_id) REFERENCES scenes(id)
);

CREATE TABLE IF NOT EXISTS assets (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  type TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  scope_unit_id TEXT,
  maturity TEXT NOT NULL DEFAULT 'exploring',
  sync_status TEXT NOT NULL DEFAULT 'normal',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id),
  FOREIGN KEY(scope_unit_id) REFERENCES content_units(id)
);

CREATE TABLE IF NOT EXISTS asset_media (
  id TEXT PRIMARY KEY,
  asset_id TEXT NOT NULL,
  media_type TEXT NOT NULL DEFAULT 'image',
  file_path TEXT NOT NULL,
  label TEXT NOT NULL DEFAULT '',
  description TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0,
  is_primary INTEGER NOT NULL DEFAULT 0,
  source_type TEXT NOT NULL DEFAULT 'manual',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(asset_id) REFERENCES assets(id)
);

CREATE TABLE IF NOT EXISTS asset_requirements (
  id TEXT PRIMARY KEY,
  content_unit_id TEXT,
  asset_id TEXT,
  asset_type TEXT NOT NULL,
  requirement_type TEXT NOT NULL DEFAULT 'standard',
  description TEXT NOT NULL DEFAULT '',
  prompt_draft TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'planned',
  created_from_type TEXT,
  created_from_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(content_unit_id) REFERENCES content_units(id),
  FOREIGN KEY(asset_id) REFERENCES assets(id)
);

CREATE TABLE IF NOT EXISTS keyframes (
  id TEXT PRIMARY KEY,
  shot_id TEXT NOT NULL,
  type TEXT NOT NULL DEFAULT 'single',
  file_path TEXT,
  description TEXT NOT NULL DEFAULT '',
  prompt_draft TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'planned',
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(shot_id) REFERENCES shots(id)
);

CREATE TABLE IF NOT EXISTS generation_tasks (
  id TEXT PRIMARY KEY,
  content_unit_id TEXT NOT NULL,
  name TEXT NOT NULL,
  target_model TEXT NOT NULL DEFAULT '',
  duration REAL NOT NULL DEFAULT 0,
  prompt TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'draft',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(content_unit_id) REFERENCES content_units(id)
);

CREATE TABLE IF NOT EXISTS generation_task_shots (
  generation_task_id TEXT NOT NULL,
  shot_id TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(generation_task_id, shot_id),
  FOREIGN KEY(generation_task_id) REFERENCES generation_tasks(id),
  FOREIGN KEY(shot_id) REFERENCES shots(id)
);

CREATE TABLE IF NOT EXISTS relations (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  source_type TEXT NOT NULL,
  source_id TEXT NOT NULL,
  relation_type TEXT NOT NULL,
  target_type TEXT NOT NULL,
  target_id TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  importance INTEGER NOT NULL DEFAULT 1,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS change_sets (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  name TEXT NOT NULL,
  source_type TEXT NOT NULL DEFAULT 'user',
  source_id TEXT,
  status TEXT NOT NULL DEFAULT 'closed',
  created_at TEXT NOT NULL,
  closed_at TEXT,
  FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS changes (
  id TEXT PRIMARY KEY,
  change_set_id TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT NOT NULL,
  field_name TEXT NOT NULL,
  old_value TEXT,
  new_value TEXT,
  source_type TEXT NOT NULL DEFAULT 'user',
  source_id TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(change_set_id) REFERENCES change_sets(id)
);

CREATE TABLE IF NOT EXISTS snapshots (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  scope_type TEXT NOT NULL DEFAULT 'project',
  scope_id TEXT,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  revision INTEGER NOT NULL,
  snapshot_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE INDEX IF NOT EXISTS idx_content_units_parent ON content_units(parent_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_scenes_script ON scenes(script_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_shots_scene ON shots(scene_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_assets_project ON assets(project_id, type);
CREATE INDEX IF NOT EXISTS idx_changes_set ON changes(change_set_id, created_at);
CREATE INDEX IF NOT EXISTS idx_relations_source ON relations(source_type, source_id);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_complete_project_database() {
        let temp = tempfile::tempdir().unwrap();
        let descriptor = init_database(temp.path(), "测试项目", "series").unwrap();
        assert_eq!(descriptor.name, "测试项目");
        let conn = open_database(temp.path()).unwrap();
        for table in STATE_TABLES {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing table {table}");
        }
    }
}
