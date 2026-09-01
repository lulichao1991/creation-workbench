use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::database::AppResult;

pub type RuntimeEventSink = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTaskInput {
    pub task_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub runtime_session_id: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub app_data_dir: Option<String>,
    pub prompt: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub thinking_level: Option<String>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub allow_call_expert: Option<bool>,
    #[serde(default)]
    pub result_tool_kind: Option<String>,
    #[serde(default)]
    pub attachments: Vec<RuntimeAttachment>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAttachment {
    pub name: String,
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTaskHandle {
    pub task_id: String,
    pub runtime_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostics {
    pub healthy: bool,
    pub agent_host_healthy: bool,
    pub sdk_version: Option<String>,
    pub model_runtime_healthy: bool,
    pub model_runtime_error: Option<String>,
    pub provider_count: usize,
    pub model_count: usize,
    pub provider_auth: Vec<ProviderAuthDiagnostic>,
    pub session_health: SessionHealthDiagnostic,
    pub tool_gateway_healthy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectionTest {
    pub healthy: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuthDiagnostic {
    pub provider_id: String,
    pub configured: bool,
    pub source: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHealthDiagnostic {
    pub active: usize,
    pub busy: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelCatalog {
    pub providers: Vec<AgentModelProvider>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelProvider {
    pub id: String,
    pub name: String,
    pub auth_configured: bool,
    pub auth_source: Option<String>,
    pub auth_label: Option<String>,
    pub models: Vec<AgentModel>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModel {
    pub id: String,
    pub name: String,
    pub supports_vision: bool,
    pub reasoning: bool,
    pub context_window: usize,
    pub max_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTaskState {
    Running,
    Completed,
    Cancelled,
    Failed,
}

impl RuntimeTaskState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    TaskStarted {
        task_id: String,
    },
    TextDelta {
        task_id: String,
        delta: String,
    },
    ToolCallRequested {
        task_id: String,
        tool_name: String,
        arguments: Value,
    },
    ToolCallCompleted {
        task_id: String,
        tool_name: String,
        result: Value,
    },
    UsageUpdated {
        task_id: String,
        usage: Value,
    },
    StructuredResult {
        task_id: String,
        result: Value,
    },
    TaskCompleted {
        task_id: String,
    },
    TaskFailed {
        task_id: String,
        error: String,
    },
    TaskCancelled {
        task_id: String,
    },
}

pub trait AgentRuntime: Send {
    fn start_task(
        &mut self,
        input: RuntimeTaskInput,
        event_sink: RuntimeEventSink,
    ) -> AppResult<RuntimeTaskHandle>;
    fn send_user_input(&mut self, task_id: &str, input: String) -> AppResult<()>;
    fn send_follow_up(&mut self, task_id: &str, input: String) -> AppResult<()>;
    fn cancel_task(&mut self, task_id: &str) -> AppResult<()>;
    fn close_session(&mut self, session_id: &str) -> AppResult<()>;
    fn get_task_state(&self, task_id: &str) -> AppResult<RuntimeTaskState>;
    fn get_models(&mut self) -> AppResult<AgentModelCatalog> {
        Err("当前 Runtime 不支持应用内模型配置".into())
    }
    fn login_provider(&mut self, _provider_id: &str, _api_key: &str) -> AppResult<()> {
        Err("当前 Runtime 不支持应用内 Provider 登录".into())
    }
    fn logout_provider(&mut self, _provider_id: &str) -> AppResult<()> {
        Err("当前 Runtime 不支持应用内 Provider 注销".into())
    }
    fn test_provider(&mut self, _provider_id: &str) -> AppResult<ProviderConnectionTest> {
        Err("当前 Runtime 不支持 Provider 连接测试".into())
    }
    fn doctor(&mut self) -> AppResult<RuntimeDiagnostics> {
        Err("当前 Runtime 不支持 Agent Host Doctor".into())
    }
    fn dispose(&mut self) -> AppResult<()>;
}
