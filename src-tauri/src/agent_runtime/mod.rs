mod host;
mod runtime;

#[cfg(test)]
pub(crate) mod mock;

use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

use crate::app_database::load_feature_flags;
use crate::database::AppResult;
use host::PiSdkRuntimeAdapter;
use runtime::AgentRuntime;

pub use runtime::{
    AgentModelCatalog, ProviderConnectionTest, RuntimeAttachment, RuntimeDiagnostics,
    RuntimeTaskHandle, RuntimeTaskInput, RuntimeTaskState,
};
pub(crate) use runtime::{RuntimeEvent, RuntimeEventSink};

pub const RUNTIME_EVENT_NAME: &str = "agent-runtime-event";

#[derive(Clone)]
pub struct RuntimeState {
    runtime: Arc<Mutex<Box<dyn AgentRuntime>>>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            runtime: Arc::new(Mutex::new(Box::new(PiSdkRuntimeAdapter::default()))),
        }
    }
}

impl RuntimeState {
    pub(crate) fn for_resource_dir(resource_dir: std::path::PathBuf) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(Box::new(PiSdkRuntimeAdapter::for_resource_dir(
                resource_dir,
            )))),
        }
    }

    pub(crate) fn start_task(
        &self,
        input: RuntimeTaskInput,
        event_sink: RuntimeEventSink,
    ) -> AppResult<RuntimeTaskHandle> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .start_task(input, event_sink)
    }

    pub(crate) fn cancel_task(&self, task_id: &str) -> AppResult<()> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .cancel_task(task_id)
    }

    pub(crate) fn close_session(&self, session_id: &str) -> AppResult<()> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .close_session(session_id)
    }

    pub(crate) fn get_models(&self) -> AppResult<AgentModelCatalog> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .get_models()
    }

    pub(crate) fn login_provider(&self, provider_id: &str, api_key: &str) -> AppResult<()> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .login_provider(provider_id, api_key)
    }

    pub(crate) fn start_provider_auth(
        &self,
        provider_id: &str,
        auth_type: &str,
    ) -> AppResult<serde_json::Value> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .start_provider_auth(provider_id, auth_type)
    }

    pub(crate) fn get_provider_auth_flow(&self, flow_id: &str) -> AppResult<serde_json::Value> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .get_provider_auth_flow(flow_id)
    }

    pub(crate) fn respond_provider_auth(
        &self,
        flow_id: &str,
        prompt_id: &str,
        value: &str,
    ) -> AppResult<()> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .respond_provider_auth(flow_id, prompt_id, value)
    }

    pub(crate) fn cancel_provider_auth(&self, flow_id: &str) -> AppResult<serde_json::Value> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .cancel_provider_auth(flow_id)
    }

    pub(crate) fn save_custom_provider(
        &self,
        provider_id: &str,
        previous_provider_id: Option<&str>,
        provider: serde_json::Value,
    ) -> AppResult<()> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .save_custom_provider(provider_id, previous_provider_id, provider)
    }

    pub(crate) fn delete_custom_provider(&self, provider_id: &str) -> AppResult<()> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .delete_custom_provider(provider_id)
    }

    pub(crate) fn refresh_models(&self, provider_id: Option<&str>) -> AppResult<serde_json::Value> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .refresh_models(provider_id)
    }

    pub(crate) fn import_legacy_api_keys(&self, keys: serde_json::Value) -> AppResult<()> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .import_legacy_api_keys(keys)
    }

    pub(crate) fn logout_provider(&self, provider_id: &str) -> AppResult<()> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .logout_provider(provider_id)
    }

    pub(crate) fn test_provider(&self, provider_id: &str) -> AppResult<ProviderConnectionTest> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .test_provider(provider_id)
    }

    pub(crate) fn doctor(&self) -> AppResult<RuntimeDiagnostics> {
        self.runtime
            .lock()
            .map_err(|_| "Runtime 状态锁损坏".to_string())?
            .doctor()
    }

    #[cfg(test)]
    pub(crate) fn with_runtime(runtime: impl AgentRuntime + 'static) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(Box::new(runtime))),
        }
    }
}

#[tauri::command]
pub fn agent_runtime_start_readonly(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    input: RuntimeTaskInput,
) -> AppResult<RuntimeTaskHandle> {
    ensure_agent_core_enabled(&app)?;
    if let Some(app_data_dir) = input.app_data_dir.as_deref() {
        crate::agent_models::restore_agent_credentials(std::path::Path::new(app_data_dir), &state)?;
    }
    let event_app = app.clone();
    let event_sink: RuntimeEventSink = Arc::new(move |event| {
        let _ = event_app.emit(RUNTIME_EVENT_NAME, event);
    });
    state.start_task(input, event_sink)
}

#[tauri::command]
pub fn agent_runtime_send_input(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    task_id: String,
    input: String,
) -> AppResult<()> {
    ensure_agent_core_enabled(&app)?;
    state
        .runtime
        .lock()
        .map_err(|_| "Runtime 状态锁损坏".to_string())?
        .send_user_input(&task_id, input)
}

#[tauri::command]
pub fn agent_runtime_follow_up(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    task_id: String,
    input: String,
) -> AppResult<()> {
    ensure_agent_core_enabled(&app)?;
    state
        .runtime
        .lock()
        .map_err(|_| "Runtime 状态锁损坏".to_string())?
        .send_follow_up(&task_id, input)
}

#[tauri::command]
pub fn agent_cancel_task(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    task_id: String,
) -> AppResult<()> {
    ensure_agent_core_enabled(&app)?;
    state.cancel_task(&task_id)
}

#[tauri::command]
pub fn agent_get_task_state(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    task_id: String,
) -> AppResult<RuntimeTaskState> {
    ensure_agent_core_enabled(&app)?;
    state
        .runtime
        .lock()
        .map_err(|_| "Runtime 状态锁损坏".to_string())?
        .get_task_state(&task_id)
}

#[tauri::command]
pub fn agent_runtime_doctor(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> AppResult<RuntimeDiagnostics> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("读取应用数据目录失败：{error}"))?;
    crate::agent_models::restore_agent_credentials(&app_data_dir, &state)?;
    let mut diagnostics = state.doctor()?;
    diagnostics.local_database_healthy = true;
    Ok(diagnostics)
}

#[tauri::command]
pub fn agent_provider_test(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    provider_id: String,
) -> AppResult<ProviderConnectionTest> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("读取应用数据目录失败：{error}"))?;
    crate::agent_models::restore_agent_credentials(&app_data_dir, &state)?;
    state.test_provider(&provider_id)
}

pub(crate) fn ensure_agent_core_enabled(app: &tauri::AppHandle) -> AppResult<()> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
    if load_feature_flags(&app_data_dir)?.get("agent_core") != Some(&true) {
        return Err("Agent Core 特性尚未启用".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::mock::MockRuntime;
    use crate::agent_runtime::runtime::{AgentRuntime, RuntimeEvent, RuntimeTaskInput};
    use std::sync::mpsc;
    use std::time::Duration;

    fn input(task_id: &str, prompt: &str) -> RuntimeTaskInput {
        RuntimeTaskInput {
            task_id: Some(task_id.into()),
            session_id: None,
            runtime_session_id: None,
            project_path: None,
            app_data_dir: None,
            prompt: prompt.into(),
            provider: None,
            model: None,
            system_prompt: None,
            thinking_level: None,
            allowed_tools: None,
            allow_call_expert: None,
            result_tool_kind: None,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn mock_runtime_streams_readonly_shot_answer_and_cancels() {
        let (tx, rx) = mpsc::channel();
        let sink: RuntimeEventSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let mut runtime = MockRuntime::new(
            vec!["镜头04：".into(), "中景低机位".into()],
            Duration::from_millis(5),
        );
        runtime
            .start_task(input("stream", "分析当前镜头04"), Arc::clone(&sink))
            .unwrap();
        let events: Vec<_> = (0..4)
            .map(|_| rx.recv_timeout(Duration::from_secs(1)).unwrap())
            .collect();
        assert!(matches!(events[0], RuntimeEvent::TaskStarted { .. }));
        assert!(matches!(events[1], RuntimeEvent::TextDelta { .. }));
        assert!(matches!(events[2], RuntimeEvent::TextDelta { .. }));
        assert!(matches!(events[3], RuntimeEvent::TaskCompleted { .. }));
        assert_eq!(
            runtime.get_task_state("stream").unwrap(),
            RuntimeTaskState::Completed
        );

        runtime
            .start_task(input("cancel", "继续分析"), Arc::clone(&sink))
            .unwrap();
        runtime.cancel_task("cancel").unwrap();
        assert_eq!(
            runtime.get_task_state("cancel").unwrap(),
            RuntimeTaskState::Cancelled
        );
    }
}
