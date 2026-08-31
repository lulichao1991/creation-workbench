use chrono::Utc;
use rusqlite::{params, types::ValueRef, Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub type AppResult<T> = Result<T, String>;
pub const CURRENT_SCHEMA_VERSION: i64 = 7;

pub const AGENT_TABLES: &[&str] = &[
    "ai_cards",
    "patch_items",
    "patch_proposals",
    "context_packages",
    "agent_tasks",
    "agent_messages",
    "agent_sessions",
    "project_expert_overrides",
];

pub const BUSINESS_TABLES: &[&str] = &[
    "content_units",
    "scripts",
    "scenes",
    "shots",
    "assets",
    "asset_media",
    "asset_requirements",
    "asset_requirement_sources",
    "asset_media_requirements",
    "shot_assets",
    "keyframes",
    "generation_tasks",
    "generation_task_shots",
    "relations",
    "story_elements",
    "story_element_occurrences",
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
    "asset_requirement_sources",
    "asset_media_requirements",
    "shot_assets",
    "keyframes",
    "generation_tasks",
    "generation_task_shots",
    "relations",
    "story_elements",
    "story_element_occurrences",
    "graph_layouts",
    "project_memories",
    "memory_sources",
    "image_generation_jobs",
    "image_generation_results",
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
    let mut conn = Connection::open(&path).map_err(|e| format!("打开数据库失败：{e}"))?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
        .map_err(|e| format!("初始化数据库连接失败：{e}"))?;
    migrate_database(&mut conn, project_path)?;
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
    conn.execute_batch(MIGRATION_V4)
        .map_err(|e| format!("初始化 Agent 数据结构失败：{e}"))?;
    conn.execute_batch(MIGRATION_V5)
        .map_err(|e| format!("初始化高级作品结构失败：{e}"))?;
    conn.execute_batch(MIGRATION_V6)
        .map_err(|e| format!("初始化项目记忆结构失败：{e}"))?;
    conn.execute_batch(MIGRATION_V7)
        .map_err(|e| format!("初始化静态生图结构失败：{e}"))?;
    verify_database(&conn)?;
    conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
        .map_err(|e| format!("写入数据库版本失败：{e}"))?;

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

fn migrate_database(conn: &mut Connection, project_path: &Path) -> AppResult<()> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| format!("读取数据库版本失败：{e}"))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "项目数据库版本 {version} 高于当前应用支持的版本 {CURRENT_SCHEMA_VERSION}"
        ));
    }
    if version == CURRENT_SCHEMA_VERSION {
        return Ok(());
    }

    let backups = project_path.join("backups");
    fs::create_dir_all(&backups).map_err(|e| format!("创建迁移备份目录失败：{e}"))?;
    conn.execute_batch("PRAGMA wal_checkpoint(FULL);")
        .map_err(|e| format!("迁移前同步数据库失败：{e}"))?;
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let backup = backups.join(format!("project-v{version}-{timestamp}.db"));
    fs::copy(db_path(project_path), &backup).map_err(|e| format!("迁移前备份失败：{e}"))?;

    let tx = conn
        .transaction()
        .map_err(|e| format!("开始迁移失败：{e}"))?;
    if version < 2 {
        tx.execute_batch(MIGRATION_V2)
            .map_err(|e| format!("迁移到数据库版本 2 失败：{e}"))?;
    }
    if version < 3 {
        tx.execute_batch(MIGRATION_V3)
            .map_err(|e| format!("迁移到数据库版本 3 失败：{e}"))?;
    }
    if version < 4 {
        tx.execute_batch(MIGRATION_V4)
            .map_err(|e| format!("迁移到数据库版本 4 失败：{e}"))?;
    }
    if version < 5 {
        tx.execute_batch(MIGRATION_V5)
            .map_err(|e| format!("迁移到数据库版本 5 失败：{e}"))?;
    }
    if version < 6 {
        tx.execute_batch(MIGRATION_V6)
            .map_err(|e| format!("迁移到数据库版本 6 失败：{e}"))?;
    }
    if version < 7 {
        tx.execute_batch(MIGRATION_V7)
            .map_err(|e| format!("迁移到数据库版本 7 失败：{e}"))?;
    }
    verify_database(&tx)?;
    tx.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
        .map_err(|e| format!("更新数据库版本失败：{e}"))?;
    tx.commit()
        .map_err(|e| format!("提交数据库迁移失败：{e}"))?;
    Ok(())
}

fn verify_database(conn: &Connection) -> AppResult<()> {
    let foreign_key_violations: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|e| format!("检查数据库外键失败：{e}"))?;
    if foreign_key_violations != 0 {
        return Err(format!("数据库存在 {foreign_key_violations} 个外键错误"));
    }
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| format!("检查数据库完整性失败：{e}"))?;
    if integrity != "ok" {
        return Err(format!("数据库完整性检查失败：{integrity}"));
    }
    Ok(())
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

pub(crate) fn row_to_json(row: &Row<'_>, column_names: &[String]) -> rusqlite::Result<Value> {
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

CREATE TABLE IF NOT EXISTS asset_requirement_sources (
  id TEXT PRIMARY KEY,
  asset_requirement_id TEXT NOT NULL,
  source_type TEXT NOT NULL,
  source_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(asset_requirement_id) REFERENCES asset_requirements(id) ON DELETE CASCADE,
  UNIQUE(asset_requirement_id, source_type, source_id)
);

CREATE TABLE IF NOT EXISTS asset_media_requirements (
  id TEXT PRIMARY KEY,
  asset_media_id TEXT NOT NULL,
  asset_requirement_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(asset_media_id) REFERENCES asset_media(id) ON DELETE CASCADE,
  FOREIGN KEY(asset_requirement_id) REFERENCES asset_requirements(id) ON DELETE CASCADE,
  UNIQUE(asset_media_id, asset_requirement_id)
);

CREATE TABLE IF NOT EXISTS shot_assets (
  id TEXT PRIMARY KEY,
  shot_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'subject',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(shot_id) REFERENCES shots(id) ON DELETE CASCADE,
  FOREIGN KEY(asset_id) REFERENCES assets(id) ON DELETE CASCADE,
  UNIQUE(shot_id, asset_id, role)
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
CREATE INDEX IF NOT EXISTS idx_asset_requirement_sources_requirement ON asset_requirement_sources(asset_requirement_id);
CREATE INDEX IF NOT EXISTS idx_asset_media_requirements_media ON asset_media_requirements(asset_media_id);
CREATE INDEX IF NOT EXISTS idx_shot_assets_shot ON shot_assets(shot_id);
CREATE INDEX IF NOT EXISTS idx_changes_set ON changes(change_set_id, created_at);
CREATE INDEX IF NOT EXISTS idx_relations_source ON relations(source_type, source_id);
"#;

const MIGRATION_V2: &str = r#"
CREATE TABLE IF NOT EXISTS asset_requirement_sources (
  id TEXT PRIMARY KEY,
  asset_requirement_id TEXT NOT NULL,
  source_type TEXT NOT NULL,
  source_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(asset_requirement_id) REFERENCES asset_requirements(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS asset_media_requirements (
  id TEXT PRIMARY KEY,
  asset_media_id TEXT NOT NULL,
  asset_requirement_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(asset_media_id) REFERENCES asset_media(id) ON DELETE CASCADE,
  FOREIGN KEY(asset_requirement_id) REFERENCES asset_requirements(id) ON DELETE CASCADE,
  UNIQUE(asset_media_id, asset_requirement_id)
);
CREATE TABLE IF NOT EXISTS shot_assets (
  id TEXT PRIMARY KEY,
  shot_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'subject',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(shot_id) REFERENCES shots(id) ON DELETE CASCADE,
  FOREIGN KEY(asset_id) REFERENCES assets(id) ON DELETE CASCADE,
  UNIQUE(shot_id, asset_id, role)
);
CREATE INDEX IF NOT EXISTS idx_asset_requirement_sources_requirement ON asset_requirement_sources(asset_requirement_id);
CREATE INDEX IF NOT EXISTS idx_asset_media_requirements_media ON asset_media_requirements(asset_media_id);
CREATE INDEX IF NOT EXISTS idx_shot_assets_shot ON shot_assets(shot_id);
"#;

const MIGRATION_V3: &str = r#"
INSERT INTO asset_requirement_sources (
  id, asset_requirement_id, source_type, source_id, created_at, updated_at
)
SELECT
  lower(hex(randomblob(16))),
  requirement.id,
  COALESCE(NULLIF(requirement.created_from_type, ''), 'shot'),
  requirement.created_from_id,
  requirement.created_at,
  requirement.updated_at
FROM asset_requirements AS requirement
WHERE requirement.created_from_id IS NOT NULL
  AND requirement.created_from_id <> ''
  AND NOT EXISTS (
    SELECT 1
    FROM asset_requirement_sources AS source
    WHERE source.asset_requirement_id = requirement.id
      AND source.source_type = COALESCE(NULLIF(requirement.created_from_type, ''), 'shot')
      AND source.source_id = requirement.created_from_id
  );
DELETE FROM asset_requirement_sources
WHERE rowid NOT IN (
  SELECT MIN(rowid)
  FROM asset_requirement_sources
  GROUP BY asset_requirement_id, source_type, source_id
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_asset_requirement_sources_unique
ON asset_requirement_sources(asset_requirement_id, source_type, source_id);
"#;

const MIGRATION_V4: &str = r#"
CREATE TABLE IF NOT EXISTS agent_sessions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  scope_type TEXT NOT NULL DEFAULT 'project',
  scope_id TEXT,
  title TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS agent_messages (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  role TEXT NOT NULL,
  agent_type TEXT,
  content TEXT NOT NULL DEFAULT '',
  structured_json TEXT,
  model_provider TEXT,
  model_name TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES agent_sessions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS agent_tasks (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  task_type TEXT NOT NULL,
  agent_type TEXT NOT NULL,
  selection_json TEXT NOT NULL DEFAULT '{}',
  read_scope_json TEXT NOT NULL DEFAULT '{}',
  write_scope_json TEXT NOT NULL DEFAULT '{}',
  context_revision INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'created',
  model_provider TEXT,
  model_name TEXT,
  result_json TEXT,
  usage_json TEXT,
  error_json TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  FOREIGN KEY(session_id) REFERENCES agent_sessions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS context_packages (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL UNIQUE,
  project_revision INTEGER NOT NULL,
  center_ref_json TEXT NOT NULL,
  items_json TEXT NOT NULL DEFAULT '[]',
  memory_ids_json TEXT NOT NULL DEFAULT '[]',
  token_estimate INTEGER NOT NULL DEFAULT 0,
  checksum TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES agent_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS patch_proposals (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  base_revision INTEGER NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'draft',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES agent_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS patch_items (
  id TEXT PRIMARY KEY,
  proposal_id TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT NOT NULL,
  field_name TEXT NOT NULL,
  old_value_json TEXT,
  new_value_json TEXT,
  reason TEXT NOT NULL DEFAULT '',
  permission_state TEXT NOT NULL DEFAULT 'requires_confirmation',
  apply_state TEXT NOT NULL DEFAULT 'pending',
  sort_order INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(proposal_id) REFERENCES patch_proposals(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS ai_cards (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  card_type TEXT NOT NULL,
  related_ref_json TEXT,
  title TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL DEFAULT '',
  options_json TEXT,
  status TEXT NOT NULL DEFAULT 'pending',
  resolution_json TEXT,
  created_at TEXT NOT NULL,
  resolved_at TEXT,
  FOREIGN KEY(task_id) REFERENCES agent_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS project_expert_overrides (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  expert_type TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  model_provider TEXT,
  model_name TEXT,
  config_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
  UNIQUE(project_id, expert_type)
);

CREATE INDEX IF NOT EXISTS idx_agent_sessions_project ON agent_sessions(project_id, updated_at);
CREATE INDEX IF NOT EXISTS idx_agent_messages_session ON agent_messages(session_id, created_at);
CREATE INDEX IF NOT EXISTS idx_agent_tasks_session ON agent_tasks(session_id, created_at);
CREATE INDEX IF NOT EXISTS idx_agent_tasks_status ON agent_tasks(status, created_at);
CREATE INDEX IF NOT EXISTS idx_patch_proposals_task ON patch_proposals(task_id);
CREATE INDEX IF NOT EXISTS idx_patch_items_proposal ON patch_items(proposal_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_ai_cards_task_status ON ai_cards(task_id, status);
CREATE INDEX IF NOT EXISTS idx_project_expert_overrides_project ON project_expert_overrides(project_id);
"#;

const MIGRATION_V5: &str = r#"
CREATE TABLE IF NOT EXISTS story_elements (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  type TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  scope_unit_id TEXT,
  maturity TEXT NOT NULL DEFAULT 'exploring',
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
  FOREIGN KEY(scope_unit_id) REFERENCES content_units(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS story_element_occurrences (
  id TEXT PRIMARY KEY,
  story_element_id TEXT NOT NULL,
  content_unit_id TEXT NOT NULL,
  occurrence_type TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(story_element_id) REFERENCES story_elements(id) ON DELETE CASCADE,
  FOREIGN KEY(content_unit_id) REFERENCES content_units(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS graph_layouts (
  id TEXT PRIMARY KEY,
  scope_type TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  view_type TEXT NOT NULL,
  filter_json TEXT NOT NULL DEFAULT '{}',
  layout_json TEXT NOT NULL DEFAULT '{}',
  updated_at TEXT NOT NULL,
  UNIQUE(scope_type, scope_id, view_type)
);

CREATE INDEX IF NOT EXISTS idx_story_elements_project_type ON story_elements(project_id, type);
CREATE INDEX IF NOT EXISTS idx_story_elements_scope ON story_elements(scope_unit_id, status);
CREATE INDEX IF NOT EXISTS idx_story_occurrences_unit ON story_element_occurrences(content_unit_id);
CREATE INDEX IF NOT EXISTS idx_story_occurrences_element ON story_element_occurrences(story_element_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_graph_layouts_scope ON graph_layouts(scope_type, scope_id, view_type);
"#;

const MIGRATION_V6: &str = r#"
CREATE TABLE IF NOT EXISTS project_memories (
  id TEXT PRIMARY KEY,
  scope_type TEXT NOT NULL DEFAULT 'project',
  scope_id TEXT,
  category TEXT NOT NULL DEFAULT 'decision',
  content TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'candidate' CHECK(status IN ('candidate', 'active', 'superseded', 'invalidated')),
  confidence REAL NOT NULL DEFAULT 1.0,
  priority INTEGER NOT NULL DEFAULT 0,
  source_type TEXT NOT NULL DEFAULT 'user',
  source_id TEXT,
  supersedes_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(supersedes_id) REFERENCES project_memories(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS memory_sources (
  id TEXT PRIMARY KEY,
  memory_id TEXT NOT NULL,
  source_type TEXT NOT NULL,
  source_id TEXT,
  excerpt TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  FOREIGN KEY(memory_id) REFERENCES project_memories(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_project_memories_scope_status
ON project_memories(scope_type, scope_id, status, priority, updated_at);
CREATE INDEX IF NOT EXISTS idx_memory_sources_memory ON memory_sources(memory_id, created_at);
"#;

const MIGRATION_V7: &str = r#"
CREATE TABLE IF NOT EXISTS image_generation_jobs (
  id TEXT PRIMARY KEY,
  target_type TEXT NOT NULL CHECK(target_type IN ('assetRequirement', 'keyframe')),
  target_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  prompt TEXT NOT NULL,
  prompt_revision INTEGER NOT NULL,
  reference_images_json TEXT NOT NULL DEFAULT '[]',
  options_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL DEFAULT 'created' CHECK(status IN ('created', 'queued', 'running', 'completed', 'partial', 'cancelled', 'failed', 'interrupted')),
  request_json TEXT NOT NULL DEFAULT '{}',
  usage_json TEXT NOT NULL DEFAULT '{}',
  error_json TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT
);

CREATE TABLE IF NOT EXISTS image_generation_results (
  id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL,
  file_path TEXT NOT NULL,
  preview_path TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  sort_order INTEGER NOT NULL DEFAULT 0,
  selection_state TEXT NOT NULL DEFAULT 'available' CHECK(selection_state IN ('available', 'rejected', 'selected', 'archived', 'deleted')),
  created_at TEXT NOT NULL,
  FOREIGN KEY(job_id) REFERENCES image_generation_jobs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_image_generation_jobs_target
ON image_generation_jobs(target_type, target_id, created_at);
CREATE INDEX IF NOT EXISTS idx_image_generation_results_job
ON image_generation_results(job_id, sort_order);
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
        for table in AGENT_TABLES {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing Agent table {table}");
        }
        for index in [
            "idx_agent_tasks_session",
            "idx_patch_items_proposal",
            "idx_ai_cards_task_status",
            "idx_story_occurrences_unit",
            "idx_graph_layouts_scope",
            "idx_project_memories_scope_status",
            "idx_image_generation_jobs_target",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    [index],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing Agent index {index}");
        }
        verify_database(&conn).unwrap();
    }

    #[test]
    fn migrates_old_projects_with_backup_and_versioning() {
        let temp = tempfile::tempdir().unwrap();
        init_database(temp.path(), "旧项目", "short").unwrap();
        {
            let conn = Connection::open(db_path(temp.path())).unwrap();
            conn.execute(
                "INSERT INTO asset_requirements (id, asset_type, requirement_type, created_from_type, created_from_id, created_at, updated_at) VALUES ('legacy-requirement', 'character', '背面', 'shot', 'legacy-shot', ?1, ?1)",
                [now()],
            )
            .unwrap();
            conn.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 DROP TABLE ai_cards;
                 DROP TABLE patch_items;
                 DROP TABLE patch_proposals;
                 DROP TABLE context_packages;
                 DROP TABLE agent_tasks;
                 DROP TABLE agent_messages;
                 DROP TABLE agent_sessions;
                 DROP TABLE project_expert_overrides;
                 DROP TABLE shot_assets;
                 DROP TABLE asset_media_requirements;
                 DROP TABLE asset_requirement_sources;
                 DROP TABLE story_element_occurrences;
                 DROP TABLE story_elements;
                 DROP TABLE graph_layouts;
                 DROP TABLE memory_sources;
                 DROP TABLE project_memories;
                 DROP TABLE image_generation_results;
                 DROP TABLE image_generation_jobs;
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }

        let conn = open_database(temp.path()).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        for table in [
            "shot_assets",
            "asset_media_requirements",
            "asset_requirement_sources",
            "story_elements",
            "story_element_occurrences",
            "graph_layouts",
            "project_memories",
            "memory_sources",
            "image_generation_jobs",
            "image_generation_results",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "missing migrated table {table}");
        }
        for table in AGENT_TABLES {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "missing migrated Agent table {table}");
        }
        let migrated_source: (String, String, String) = conn
            .query_row(
                "SELECT asset_requirement_id, source_type, source_id FROM asset_requirement_sources WHERE asset_requirement_id='legacy-requirement'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            migrated_source,
            (
                "legacy-requirement".into(),
                "shot".into(),
                "legacy-shot".into()
            )
        );
        let duplicate = conn.execute(
            "INSERT INTO asset_requirement_sources (id, asset_requirement_id, source_type, source_id, created_at, updated_at) VALUES ('duplicate-source', 'legacy-requirement', 'shot', 'legacy-shot', ?1, ?1)",
            [now()],
        );
        assert!(duplicate.is_err());
        let backups = fs::read_dir(temp.path().join("backups")).unwrap().count();
        assert_eq!(backups, 1);
    }
}
