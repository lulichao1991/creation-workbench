use chrono::Utc;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

use crate::database::AppResult;

pub const APP_SCHEMA_VERSION: i64 = 1;
pub const FEATURE_FLAG_KEYS: &[&str] = &[
    "agent_core",
    "expert_agents",
    "change_analysis",
    "story_graph",
    "memory",
    "image_generation",
    "prompt_compiler",
    "expert_team",
];

fn app_db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("app.db")
}

pub fn open_app_database(app_data_dir: &Path) -> AppResult<Connection> {
    fs::create_dir_all(app_data_dir).map_err(|e| format!("创建应用数据目录失败：{e}"))?;
    let path = app_db_path(app_data_dir);
    let existed = path.is_file();
    let mut conn = Connection::open(&path).map_err(|e| format!("打开 app.db 失败：{e}"))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000; PRAGMA journal_mode = WAL;",
    )
    .map_err(|e| format!("初始化 app.db 连接失败：{e}"))?;
    migrate_app_database(&mut conn, app_data_dir, existed)?;
    Ok(conn)
}

pub fn initialize_app_database(app_data_dir: &Path) -> AppResult<()> {
    open_app_database(app_data_dir).map(|_| ())
}

pub fn load_feature_flags(app_data_dir: &Path) -> AppResult<BTreeMap<String, bool>> {
    let conn = open_app_database(app_data_dir)?;
    let mut stmt = conn
        .prepare("SELECT key, enabled FROM feature_flags ORDER BY key")
        .map_err(|e| format!("读取功能开关失败：{e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
        })
        .map_err(|e| format!("读取功能开关失败：{e}"))?;
    rows.collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|e| format!("读取功能开关失败：{e}"))
}

#[tauri::command]
pub fn get_feature_flags(app: tauri::AppHandle) -> AppResult<BTreeMap<String, bool>> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    load_feature_flags(&app_data_dir)
}

fn migrate_app_database(
    conn: &mut Connection,
    app_data_dir: &Path,
    database_existed: bool,
) -> AppResult<()> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| format!("读取 app.db 版本失败：{e}"))?;
    if version > APP_SCHEMA_VERSION {
        return Err(format!(
            "app.db 版本 {version} 高于当前应用支持的版本 {APP_SCHEMA_VERSION}"
        ));
    }
    if version == APP_SCHEMA_VERSION {
        return Ok(());
    }

    if database_existed {
        let backups = app_data_dir.join("backups");
        fs::create_dir_all(&backups).map_err(|e| format!("创建 app.db 备份目录失败：{e}"))?;
        conn.execute_batch("PRAGMA wal_checkpoint(FULL);")
            .map_err(|e| format!("迁移前同步 app.db 失败：{e}"))?;
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
        let backup = backups.join(format!("app-v{version}-{timestamp}.db"));
        fs::copy(app_db_path(app_data_dir), backup)
            .map_err(|e| format!("迁移前备份 app.db 失败：{e}"))?;
    }

    let tx = conn
        .transaction()
        .map_err(|e| format!("开始迁移 app.db 失败：{e}"))?;
    tx.execute_batch(APP_SCHEMA)
        .map_err(|e| format!("迁移 app.db 到版本 1 失败：{e}"))?;
    let timestamp = Utc::now().to_rfc3339();
    for key in FEATURE_FLAG_KEYS {
        tx.execute(
            "INSERT OR IGNORE INTO feature_flags (key, enabled, updated_at) VALUES (?1, 0, ?2)",
            params![key, timestamp],
        )
        .map_err(|e| format!("写入默认功能开关失败：{e}"))?;
    }
    verify_app_database(&tx)?;
    tx.pragma_update(None, "user_version", APP_SCHEMA_VERSION)
        .map_err(|e| format!("更新 app.db 版本失败：{e}"))?;
    tx.commit()
        .map_err(|e| format!("提交 app.db 迁移失败：{e}"))
}

fn verify_app_database(conn: &Connection) -> AppResult<()> {
    let foreign_key_violations: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|e| format!("检查 app.db 外键失败：{e}"))?;
    if foreign_key_violations != 0 {
        return Err(format!("app.db 存在 {foreign_key_violations} 个外键错误"));
    }
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| format!("检查 app.db 完整性失败：{e}"))?;
    if integrity != "ok" {
        return Err(format!("app.db 完整性检查失败：{integrity}"));
    }
    Ok(())
}

const APP_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS feature_flags (
  key TEXT PRIMARY KEY,
  enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0, 1)),
  updated_at TEXT NOT NULL
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_app_database_with_all_features_off() {
        let temp = tempfile::tempdir().unwrap();
        let conn = open_app_database(temp.path()).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, APP_SCHEMA_VERSION);
        drop(conn);

        let flags = load_feature_flags(temp.path()).unwrap();
        assert_eq!(flags.len(), FEATURE_FLAG_KEYS.len());
        for key in FEATURE_FLAG_KEYS {
            assert_eq!(flags.get(*key), Some(&false), "feature {key} was enabled");
        }
    }

    #[test]
    fn migrates_existing_app_database_with_backup_and_preserves_values() {
        let temp = tempfile::tempdir().unwrap();
        {
            let conn = Connection::open(app_db_path(temp.path())).unwrap();
            conn.execute_batch(
                "CREATE TABLE feature_flags (key TEXT PRIMARY KEY, enabled INTEGER NOT NULL, updated_at TEXT NOT NULL); PRAGMA user_version = 0;",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO feature_flags (key, enabled, updated_at) VALUES ('agent_core', 1, ?1)",
                [Utc::now().to_rfc3339()],
            )
            .unwrap();
        }

        let flags = load_feature_flags(temp.path()).unwrap();
        assert_eq!(flags.get("agent_core"), Some(&true));
        assert_eq!(flags.get("memory"), Some(&false));
        assert_eq!(flags.len(), FEATURE_FLAG_KEYS.len());
        let backups = fs::read_dir(temp.path().join("backups")).unwrap().count();
        assert_eq!(backups, 1);
    }
}
