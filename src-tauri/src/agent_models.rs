use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use base64::Engine;
use chrono::Utc;
use keyring::v1::{Entry, Error as KeyringError};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Manager;

use crate::agent_application::expert;
use crate::agent_runtime::{AgentModelCatalog, RuntimeState};
use crate::app_database::open_app_database;
use crate::database::AppResult;

const SETTINGS_KEY: &str = "agent_model_settings";
const CREDENTIAL_PROVIDERS_KEY: &str = "agent_credential_providers";
const CREDENTIAL_SERVICE: &str = "com.lu.workbench.agent-provider";
const CREDENTIAL_MASTER_SERVICE: &str = "com.lu.workbench.agent-credential-master";
const CREDENTIAL_MASTER_ACCOUNT: &str = "master";
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelChoice {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub thinking_level: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelSettings {
    pub default_model: AgentModelChoice,
    #[serde(default)]
    pub professional_overrides: BTreeMap<String, AgentModelChoice>,
}

impl Default for AgentModelSettings {
    fn default() -> Self {
        Self {
            default_model: AgentModelChoice {
                provider: None,
                model: None,
                thinking_level: Some("medium".into()),
            },
            professional_overrides: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelConfiguration {
    pub catalog: AgentModelCatalog,
    pub settings: AgentModelSettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLoginInput {
    pub provider_id: String,
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuthResponseInput {
    pub flow_id: String,
    pub prompt_id: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderSaveInput {
    pub provider_id: String,
    pub previous_provider_id: Option<String>,
    pub provider: Value,
}

pub(crate) fn load_agent_model_settings(app_data_dir: &Path) -> AppResult<AgentModelSettings> {
    let conn = open_app_database(app_data_dir)?;
    let value: Option<String> = conn
        .query_row(
            "SELECT value_json FROM app_settings WHERE key=?1",
            [SETTINGS_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("读取 Agent 模型设置失败：{error}"))?;
    value
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|error| format!("解析 Agent 模型设置失败：{error}"))
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn model_choice_for_role(
    app_data_dir: &Path,
    role: &str,
) -> AppResult<AgentModelChoice> {
    let settings = load_agent_model_settings(app_data_dir)?;
    let override_choice = settings.professional_overrides.get(role);
    Ok(AgentModelChoice {
        provider: override_choice
            .and_then(|choice| choice.provider.clone())
            .or(settings.default_model.provider),
        model: override_choice
            .and_then(|choice| choice.model.clone())
            .or(settings.default_model.model),
        thinking_level: override_choice
            .and_then(|choice| choice.thinking_level.clone())
            .or(settings.default_model.thinking_level),
    })
}

fn credential_entry(provider_id: &str) -> AppResult<Entry> {
    Entry::new(CREDENTIAL_SERVICE, provider_id)
        .map_err(|error| format!("打开系统密钥库失败：{error}"))
}

pub(crate) fn credential_master_key() -> AppResult<String> {
    let entry = Entry::new(CREDENTIAL_MASTER_SERVICE, CREDENTIAL_MASTER_ACCOUNT)
        .map_err(|error| format!("打开 Workbench 凭据主密钥失败：{error}"))?;
    match entry.get_password() {
        Ok(encoded) => {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|error| format!("Workbench 凭据主密钥损坏：{error}"))?;
            if decoded.len() != 32 {
                return Err("Workbench 凭据主密钥长度无效".into());
            }
            return Ok(encoded);
        }
        Err(KeyringError::NoEntry) => {
            let encrypted_credentials = dirs::data_local_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("creation-workbench")
                .join("agent-host")
                .join("credentials.enc");
            if encrypted_credentials.is_file() {
                return Err("Workbench 凭据主密钥缺失，无法解密已有凭据".into());
            }
        }
        Err(error) => return Err(format!("读取 Workbench 凭据主密钥失败：{error}")),
    }
    let mut key = [0_u8; 32];
    getrandom::fill(&mut key).map_err(|error| format!("生成 Workbench 凭据主密钥失败：{error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(key);
    entry
        .set_password(&encoded)
        .map_err(|error| format!("保存 Workbench 凭据主密钥失败：{error}"))?;
    Ok(encoded)
}

fn credential_provider_ids(app_data_dir: &Path) -> AppResult<BTreeSet<String>> {
    let conn = open_app_database(app_data_dir)?;
    let value: Option<String> = conn
        .query_row(
            "SELECT value_json FROM app_settings WHERE key=?1",
            [CREDENTIAL_PROVIDERS_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("读取 Agent 凭据索引失败：{error}"))?;
    value
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|error| format!("解析 Agent 凭据索引失败：{error}"))
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

fn save_credential_provider_ids(
    app_data_dir: &Path,
    provider_ids: &BTreeSet<String>,
) -> AppResult<()> {
    let conn = open_app_database(app_data_dir)?;
    conn.execute(
        "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
        params![
            CREDENTIAL_PROVIDERS_KEY,
            serde_json::to_string(provider_ids).map_err(|error| error.to_string())?,
            Utc::now().to_rfc3339()
        ],
    )
    .map_err(|error| format!("保存 Agent 凭据索引失败：{error}"))?;
    Ok(())
}

pub(crate) fn restore_agent_credentials(
    app_data_dir: &Path,
    runtime: &RuntimeState,
) -> AppResult<()> {
    let provider_ids = credential_provider_ids(app_data_dir)?;
    let mut keys = Vec::new();
    for provider_id in &provider_ids {
        match credential_entry(provider_id)?.get_password() {
            Ok(api_key) => keys.push(json!({ "providerId": provider_id, "apiKey": api_key })),
            Err(KeyringError::NoEntry) => {}
            Err(error) => return Err(format!("读取旧 Agent 凭据失败：{error}")),
        }
    }
    if !keys.is_empty() {
        runtime.import_legacy_api_keys(Value::Array(keys))?;
    }
    for provider_id in &provider_ids {
        let _ = credential_entry(provider_id)?.delete_credential();
    }
    save_credential_provider_ids(app_data_dir, &BTreeSet::new())?;
    Ok(())
}

#[tauri::command]
pub fn agent_model_settings_get(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeState>,
) -> AppResult<AgentModelConfiguration> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("读取应用数据目录失败：{error}"))?;
    restore_agent_credentials(&app_data_dir, &runtime)?;
    Ok(AgentModelConfiguration {
        catalog: runtime.get_models()?,
        settings: load_agent_model_settings(&app_data_dir)?,
    })
}

#[tauri::command]
pub fn agent_model_settings_save(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeState>,
    settings: AgentModelSettings,
) -> AppResult<AgentModelSettings> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("读取应用数据目录失败：{error}"))?;
    restore_agent_credentials(&app_data_dir, &runtime)?;
    let catalog = runtime.get_models()?;
    validate_settings(&settings, &catalog)?;
    let conn = open_app_database(&app_data_dir)?;
    conn.execute(
        "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
        params![
            SETTINGS_KEY,
            serde_json::to_string(&settings).map_err(|error| error.to_string())?,
            Utc::now().to_rfc3339()
        ],
    )
    .map_err(|error| format!("保存 Agent 模型设置失败：{error}"))?;
    Ok(settings)
}

#[tauri::command]
pub fn agent_provider_login(
    runtime: tauri::State<'_, RuntimeState>,
    input: ProviderLoginInput,
) -> AppResult<()> {
    if input.provider_id.trim().is_empty() || input.api_key.trim().is_empty() {
        return Err("Provider 和 API Key 不能为空".into());
    }
    let provider_id = input.provider_id.trim();
    if !runtime
        .get_models()?
        .providers
        .iter()
        .any(|provider| provider.id == provider_id)
    {
        return Err(format!("Provider 不存在：{provider_id}"));
    }
    runtime.login_provider(provider_id, input.api_key.trim())
}

#[tauri::command]
pub fn agent_provider_auth_start(
    runtime: tauri::State<'_, RuntimeState>,
    provider_id: String,
    auth_type: String,
) -> AppResult<Value> {
    runtime.start_provider_auth(provider_id.trim(), auth_type.trim())
}

#[tauri::command]
pub fn agent_provider_auth_get(
    runtime: tauri::State<'_, RuntimeState>,
    flow_id: String,
) -> AppResult<Value> {
    runtime.get_provider_auth_flow(flow_id.trim())
}

#[tauri::command]
pub fn agent_provider_auth_respond(
    runtime: tauri::State<'_, RuntimeState>,
    input: ProviderAuthResponseInput,
) -> AppResult<()> {
    runtime.respond_provider_auth(
        input.flow_id.trim(),
        input.prompt_id.trim(),
        input.value.trim(),
    )
}

#[tauri::command]
pub fn agent_provider_auth_cancel(
    runtime: tauri::State<'_, RuntimeState>,
    flow_id: String,
) -> AppResult<Value> {
    runtime.cancel_provider_auth(flow_id.trim())
}

#[tauri::command]
pub fn agent_custom_provider_save(
    runtime: tauri::State<'_, RuntimeState>,
    input: CustomProviderSaveInput,
) -> AppResult<()> {
    runtime.save_custom_provider(
        input.provider_id.trim(),
        input.previous_provider_id.as_deref().map(str::trim),
        input.provider,
    )
}

#[tauri::command]
pub fn agent_custom_provider_delete(
    runtime: tauri::State<'_, RuntimeState>,
    provider_id: String,
) -> AppResult<()> {
    runtime.delete_custom_provider(provider_id.trim())
}

#[tauri::command]
pub fn agent_models_refresh(
    runtime: tauri::State<'_, RuntimeState>,
    provider_id: Option<String>,
) -> AppResult<Value> {
    runtime.refresh_models(
        provider_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    )
}

#[tauri::command]
pub fn agent_provider_logout(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeState>,
    provider_id: String,
) -> AppResult<()> {
    if provider_id.trim().is_empty() {
        return Err("Provider 不能为空".into());
    }
    let provider_id = provider_id.trim();
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("读取应用数据目录失败：{error}"))?;
    let _ = credential_entry(provider_id)?.delete_credential();
    let mut provider_ids = credential_provider_ids(&app_data_dir)?;
    provider_ids.remove(provider_id);
    save_credential_provider_ids(&app_data_dir, &provider_ids)?;
    runtime.logout_provider(provider_id)
}

fn validate_settings(settings: &AgentModelSettings, catalog: &AgentModelCatalog) -> AppResult<()> {
    validate_choice("主 Agent", &settings.default_model, catalog, true)?;
    for (role, choice) in &settings.professional_overrides {
        let definition = expert(role).ok_or_else(|| format!("未知专业 Agent：{role}"))?;
        validate_choice(definition.display_name, choice, catalog, false)?;
    }
    Ok(())
}

fn validate_choice(
    label: &str,
    choice: &AgentModelChoice,
    catalog: &AgentModelCatalog,
    required: bool,
) -> AppResult<()> {
    match (choice.provider.as_deref(), choice.model.as_deref()) {
        (None, None) if !required => Ok(()),
        (Some(provider), Some(model)) => {
            let selected = catalog
                .providers
                .iter()
                .find(|candidate| candidate.id == provider)
                .and_then(|candidate| {
                    candidate
                        .models
                        .iter()
                        .find(|candidate| candidate.id == model)
                })
                .ok_or_else(|| format!("{label} 的模型不存在：{provider}/{model}"))?;
            if let Some(level) = choice.thinking_level.as_deref() {
                if !selected
                    .supported_thinking_levels
                    .iter()
                    .any(|supported| supported == level)
                {
                    return Err(format!("{label} 的模型不支持推理强度：{level}"));
                }
            }
            Ok(())
        }
        (None, None) => Err(format!("{label} 必须选择 Provider 和模型")),
        _ => Err(format!("{label} 的 Provider 和模型必须同时设置")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_non_secret_model_choices_and_resolves_professional_override() {
        let temp = tempfile::tempdir().unwrap();
        let settings = AgentModelSettings {
            default_model: AgentModelChoice {
                provider: Some("provider-a".into()),
                model: Some("main-model".into()),
                thinking_level: Some("medium".into()),
            },
            professional_overrides: BTreeMap::from([(
                "cinematography".into(),
                AgentModelChoice {
                    provider: Some("provider-b".into()),
                    model: Some("vision-model".into()),
                    thinking_level: Some("high".into()),
                },
            )]),
        };
        let conn = open_app_database(temp.path()).unwrap();
        conn.execute(
            "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)",
            params![
                SETTINGS_KEY,
                serde_json::to_string(&settings).unwrap(),
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
        drop(conn);

        let main = model_choice_for_role(temp.path(), "main").unwrap();
        assert_eq!(main.model.as_deref(), Some("main-model"));
        let cinema = model_choice_for_role(temp.path(), "cinematography").unwrap();
        assert_eq!(cinema.provider.as_deref(), Some("provider-b"));
        assert_eq!(cinema.model.as_deref(), Some("vision-model"));
        assert_eq!(cinema.thinking_level.as_deref(), Some("high"));
        let stored = open_app_database(temp.path())
            .unwrap()
            .query_row(
                "SELECT value_json FROM app_settings WHERE key=?1",
                [SETTINGS_KEY],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert!(!stored.contains("apiKey"));
    }

    #[test]
    fn persists_only_provider_ids_in_the_credential_index() {
        let temp = tempfile::tempdir().unwrap();
        let provider_ids = BTreeSet::from(["openai".to_string(), "anthropic".to_string()]);
        save_credential_provider_ids(temp.path(), &provider_ids).unwrap();
        assert_eq!(credential_provider_ids(temp.path()).unwrap(), provider_ids);
        let stored = open_app_database(temp.path())
            .unwrap()
            .query_row(
                "SELECT value_json FROM app_settings WHERE key=?1",
                [CREDENTIAL_PROVIDERS_KEY],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert!(!stored.to_ascii_lowercase().contains("api_key"));
    }

    #[test]
    fn validates_thinking_level_against_the_selected_pi_model() {
        let catalog: AgentModelCatalog = serde_json::from_value(json!({
            "providers": [{
                "id": "custom-local",
                "name": "Local",
                "authConfigured": true,
                "authSource": "configured API key",
                "authLabel": "API key",
                "authMethods": [{ "type": "api_key", "interactive": true, "label": "API key" }],
                "custom": true,
                "customConfig": { "apiKey": "workbench-local", "authHeader": false },
                "models": [{
                    "id": "local-model",
                    "name": "Local Model",
                    "supportsVision": false,
                    "reasoning": false,
                    "supportedThinkingLevels": ["off"],
                    "contextWindow": 128000,
                    "maxTokens": 16384
                }]
            }]
        }))
        .unwrap();
        let valid = AgentModelChoice {
            provider: Some("custom-local".into()),
            model: Some("local-model".into()),
            thinking_level: Some("off".into()),
        };
        assert!(validate_choice("主 Agent", &valid, &catalog, true).is_ok());
        let invalid = AgentModelChoice {
            thinking_level: Some("high".into()),
            ..valid
        };
        assert!(validate_choice("主 Agent", &invalid, &catalog, true)
            .unwrap_err()
            .contains("不支持推理强度"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_credential_manager_round_trips_agent_secret() {
        let provider_id = format!(
            "workbench-test-{}-{}",
            std::process::id(),
            Utc::now().timestamp_millis()
        );
        let entry = credential_entry(&provider_id).unwrap();
        entry.set_password("temporary-test-secret").unwrap();
        assert_eq!(entry.get_password().unwrap(), "temporary-test-secret");
        entry.delete_credential().unwrap();
    }
}
