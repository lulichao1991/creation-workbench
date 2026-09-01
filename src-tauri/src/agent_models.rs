use std::collections::BTreeMap;
use std::path::Path;

use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::agent_application::expert;
use crate::agent_runtime::{AgentModelCatalog, RuntimeState};
use crate::app_database::open_app_database;
use crate::database::AppResult;

const SETTINGS_KEY: &str = "agent_model_settings";
const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

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

#[tauri::command]
pub fn agent_model_settings_get(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeState>,
) -> AppResult<AgentModelConfiguration> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("读取应用数据目录失败：{error}"))?;
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
    let catalog = runtime.get_models()?;
    validate_settings(&settings, &catalog)?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("读取应用数据目录失败：{error}"))?;
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
    runtime.login_provider(input.provider_id.trim(), input.api_key.trim())
}

#[tauri::command]
pub fn agent_provider_logout(
    runtime: tauri::State<'_, RuntimeState>,
    provider_id: String,
) -> AppResult<()> {
    if provider_id.trim().is_empty() {
        return Err("Provider 不能为空".into());
    }
    runtime.logout_provider(provider_id.trim())
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
    if choice
        .thinking_level
        .as_deref()
        .is_some_and(|level| !THINKING_LEVELS.contains(&level))
    {
        return Err(format!("{label} 的 thinking level 无效"));
    }
    match (choice.provider.as_deref(), choice.model.as_deref()) {
        (None, None) if !required => Ok(()),
        (Some(provider), Some(model)) => {
            let exists = catalog
                .providers
                .iter()
                .find(|candidate| candidate.id == provider)
                .is_some_and(|candidate| {
                    candidate
                        .models
                        .iter()
                        .any(|candidate| candidate.id == model)
                });
            if exists {
                Ok(())
            } else {
                Err(format!("{label} 的模型不存在：{provider}/{model}"))
            }
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
}
