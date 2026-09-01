use crate::app_database::{load_feature_flags, open_app_database};
use crate::database::{now, AppResult};
use keyring::v1::Entry;
use reqwest::Url;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use tauri::Manager;

const KEYRING_SERVICE: &str = "com.lu.workbench.image-provider";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub provider_type: String,
    pub display_name: String,
    pub base_url: String,
    pub text_to_image_path: String,
    pub image_edit_path: String,
    pub default_model: String,
    pub capabilities: serde_json::Value,
    pub timeout_seconds: i64,
    pub max_concurrency: i64,
    pub allow_image_upload: bool,
    pub status: String,
    pub has_secret: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderInput {
    pub request_id: String,
    pub provider_type: String,
    pub display_name: String,
    pub base_url: String,
    pub text_to_image_path: Option<String>,
    pub image_edit_path: Option<String>,
    pub default_model: String,
    pub api_key: Option<String>,
    pub timeout_seconds: Option<i64>,
    pub max_concurrency: Option<i64>,
    pub allow_image_upload: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub healthy: bool,
    pub message: String,
}

fn ensure_enabled(app: &tauri::AppHandle) -> AppResult<std::path::PathBuf> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    if load_feature_flags(&app_data_dir)?.get("image_generation") == Some(&true) {
        Ok(app_data_dir)
    } else {
        Err("静态生图系统尚未启用".into())
    }
}

fn validate_base_url(value: &str) -> AppResult<String> {
    let url = Url::parse(value.trim()).map_err(|_| "TOOL_ARGUMENT_INVALID: Base URL 无效")?;
    let local_http =
        url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !local_http {
        return Err(
            "TOOL_ARGUMENT_INVALID: Provider 必须使用 HTTPS；仅本机 localhost 可使用 HTTP".into(),
        );
    }
    Ok(value.trim().trim_end_matches('/').to_string())
}

fn validate_endpoint_path(value: Option<&str>, fallback: &str) -> AppResult<String> {
    let path = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    if !path.starts_with('/') || path.contains("://") || path.contains('?') || path.contains('#') {
        return Err(
            "TOOL_ARGUMENT_INVALID: 接口路径必须是以 / 开头、且不含域名、查询参数或片段的相对路径"
                .into(),
        );
    }
    Ok(path.to_string())
}

fn entry(config_id: &str) -> AppResult<Entry> {
    Entry::new(KEYRING_SERVICE, config_id).map_err(|e| format!("打开系统密钥库失败：{e}"))
}

pub(crate) fn provider_secret(config_id: &str) -> AppResult<String> {
    entry(config_id)?
        .get_password()
        .map_err(|_| "PROVIDER_NOT_CONFIGURED: Provider 密钥不存在，请重新保存 API Key".into())
}

pub(crate) fn load_provider(
    app_data_dir: &std::path::Path,
    config_id: &str,
) -> AppResult<ProviderConfig> {
    let conn = open_app_database(app_data_dir)?;
    conn.query_row(
        "SELECT id, provider_type, display_name, base_url, text_to_image_path, image_edit_path, default_model, capabilities_json, timeout_seconds, max_concurrency, allow_image_upload, status, secret_ref, created_at, updated_at FROM provider_configs WHERE id=?1",
        [config_id],
        |row| {
            let capabilities_raw: String = row.get(7)?;
            let secret_ref: String = row.get(12)?;
            Ok(ProviderConfig {
                id: row.get(0)?,
                provider_type: row.get(1)?,
                display_name: row.get(2)?,
                base_url: row.get(3)?,
                text_to_image_path: row.get(4)?,
                image_edit_path: row.get(5)?,
                default_model: row.get(6)?,
                capabilities: serde_json::from_str(&capabilities_raw).unwrap_or_else(|_| json!({})),
                timeout_seconds: row.get(8)?,
                max_concurrency: row.get(9)?,
                allow_image_upload: row.get::<_, i64>(10)? != 0,
                status: row.get(11)?,
                has_secret: !secret_ref.is_empty(),
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
            })
        },
    )
    .map_err(|_| "PROVIDER_NOT_CONFIGURED: Provider 配置不存在".to_string())
}

#[tauri::command]
pub fn provider_list(app: tauri::AppHandle) -> AppResult<Vec<ProviderConfig>> {
    let app_data_dir = ensure_enabled(&app)?;
    let conn = open_app_database(&app_data_dir)?;
    let mut stmt = conn
        .prepare("SELECT id FROM provider_configs ORDER BY updated_at DESC, id")
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    ids.iter()
        .map(|id| load_provider(&app_data_dir, id))
        .collect()
}

#[tauri::command]
pub fn provider_save(app: tauri::AppHandle, input: SaveProviderInput) -> AppResult<ProviderConfig> {
    let app_data_dir = ensure_enabled(&app)?;
    if input.request_id.trim().is_empty()
        || input.display_name.trim().is_empty()
        || input.default_model.trim().is_empty()
    {
        return Err("TOOL_ARGUMENT_INVALID: Provider ID、名称和默认模型不能为空".into());
    }
    if input.provider_type != "openai_compatible" && input.provider_type != "mock" {
        return Err("TOOL_ARGUMENT_INVALID: 当前仅支持 openai_compatible 或 mock Provider".into());
    }
    let base_url = validate_base_url(&input.base_url)?;
    let text_to_image_path =
        validate_endpoint_path(input.text_to_image_path.as_deref(), "/images/generations")?;
    let image_edit_path =
        validate_endpoint_path(input.image_edit_path.as_deref(), "/images/edits")?;
    let conn = open_app_database(&app_data_dir)?;
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT created_at,secret_ref FROM provider_configs WHERE id=?1",
            [&input.request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let supplied_secret = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if input.provider_type == "openai_compatible"
        && supplied_secret.is_none()
        && existing
            .as_ref()
            .is_none_or(|(_, reference)| reference.is_empty())
    {
        return Err("PROVIDER_NOT_CONFIGURED: 首次配置真实 Provider 时必须填写 API Key".into());
    }
    if let Some(secret) = supplied_secret {
        entry(&input.request_id)?
            .set_password(secret)
            .map_err(|e| format!("写入系统密钥库失败：{e}"))?;
    }
    let secret_ref = if input.provider_type == "mock" {
        String::new()
    } else {
        format!("keyring://image-provider/{}", input.request_id)
    };
    let timestamp = now();
    let created_at = existing.map(|(created_at, _)| created_at);
    let supports_references =
        input.provider_type == "mock" || input.allow_image_upload.unwrap_or(false);
    let capabilities = json!({
        "textToImage": true,
        "imageToImage": supports_references,
        "referenceImages": supports_references,
        "multipleOutputs": true,
        "aspectRatio": true,
        "transparentBackground": true
    });
    conn.execute(
        "INSERT INTO provider_configs (id, provider_type, display_name, base_url, text_to_image_path, image_edit_path, secret_ref, default_model, capabilities_json, timeout_seconds, max_concurrency, allow_image_upload, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'configured', ?13, ?14)
         ON CONFLICT(id) DO UPDATE SET provider_type=excluded.provider_type, display_name=excluded.display_name, base_url=excluded.base_url, text_to_image_path=excluded.text_to_image_path, image_edit_path=excluded.image_edit_path, secret_ref=excluded.secret_ref, default_model=excluded.default_model, capabilities_json=excluded.capabilities_json, timeout_seconds=excluded.timeout_seconds, max_concurrency=excluded.max_concurrency, allow_image_upload=excluded.allow_image_upload, status='configured', updated_at=excluded.updated_at",
        params![
            input.request_id,
            input.provider_type,
            input.display_name.trim(),
            base_url,
            text_to_image_path,
            image_edit_path,
            secret_ref,
            input.default_model.trim(),
            capabilities.to_string(),
            input.timeout_seconds.unwrap_or(120).clamp(10, 600),
            input.max_concurrency.unwrap_or(1).clamp(1, 4),
            i64::from(supports_references || input.allow_image_upload.unwrap_or(false)),
            created_at.unwrap_or_else(|| timestamp.clone()),
            timestamp,
        ],
    )
    .map_err(|e| format!("保存 Provider 配置失败：{e}"))?;
    load_provider(&app_data_dir, &input.request_id)
}

#[tauri::command]
pub fn provider_delete(app: tauri::AppHandle, provider_id: String) -> AppResult<()> {
    let app_data_dir = ensure_enabled(&app)?;
    let conn = open_app_database(&app_data_dir)?;
    let deleted = conn
        .execute("DELETE FROM provider_configs WHERE id=?1", [&provider_id])
        .map_err(|e| e.to_string())?;
    if deleted == 0 {
        return Err("PROVIDER_NOT_CONFIGURED: Provider 配置不存在".into());
    }
    let _ = entry(&provider_id).and_then(|value| {
        value
            .delete_credential()
            .map_err(|e| format!("删除系统密钥失败：{e}"))
    });
    Ok(())
}

#[tauri::command]
pub async fn provider_test(
    app: tauri::AppHandle,
    provider_id: String,
) -> AppResult<ProviderTestResult> {
    let app_data_dir = ensure_enabled(&app)?;
    let provider = load_provider(&app_data_dir, &provider_id)?;
    let result = if provider.provider_type == "mock" {
        ProviderTestResult {
            healthy: true,
            message: "Mock 图片服务可用".into(),
        }
    } else {
        let secret = provider_secret(&provider_id)?;
        match reqwest::Client::new()
            .get(format!("{}/models", provider.base_url))
            .bearer_auth(secret)
            .timeout(Duration::from_secs(15))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => ProviderTestResult {
                healthy: true,
                message: "连接和认证均正常；生成接口路径会在首次生成时验证，以免产生测试费用"
                    .into(),
            },
            Ok(response) if matches!(response.status().as_u16(), 401 | 403) => ProviderTestResult {
                healthy: false,
                message: "服务可连接，但 API Key 未通过认证".into(),
            },
            Ok(response) => ProviderTestResult {
                healthy: false,
                message: format!(
                    "服务返回 HTTP {}，请检查 Base URL 和兼容性",
                    response.status().as_u16()
                ),
            },
            Err(error) if error.is_timeout() => ProviderTestResult {
                healthy: false,
                message: "连接超时，请检查网络或 Base URL".into(),
            },
            Err(_) => ProviderTestResult {
                healthy: false,
                message: "无法连接图片服务，请检查网络或 Base URL".into(),
            },
        }
    };
    let conn = open_app_database(&app_data_dir)?;
    conn.execute(
        "UPDATE provider_configs SET status=?1,updated_at=?2 WHERE id=?3",
        params![
            if result.healthy { "ready" } else { "error" },
            now(),
            provider_id
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_insecure_remote_provider_url() {
        assert!(validate_base_url("http://example.com/v1").is_err());
        assert_eq!(
            validate_base_url("http://127.0.0.1:8080/v1/").unwrap(),
            "http://127.0.0.1:8080/v1"
        );
        assert_eq!(
            validate_base_url("https://api.example.com/v1/").unwrap(),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn accepts_only_relative_provider_paths() {
        assert_eq!(
            validate_endpoint_path(Some("/custom/images"), "/fallback").unwrap(),
            "/custom/images"
        );
        assert_eq!(
            validate_endpoint_path(None, "/fallback").unwrap(),
            "/fallback"
        );
        assert!(validate_endpoint_path(Some("images/generations"), "/fallback").is_err());
        assert!(validate_endpoint_path(Some("https://evil.example/images"), "/fallback").is_err());
        assert!(validate_endpoint_path(Some("/images?model=x"), "/fallback").is_err());
    }
}
