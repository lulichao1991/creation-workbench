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
    pub prompt: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub thinking_level: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTaskHandle {
    pub task_id: String,
    pub runtime_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostics {
    pub found: bool,
    pub executable_path: Option<String>,
    pub version: Option<String>,
    pub rpc_handshake: bool,
    pub current_provider: Option<String>,
    pub current_model: Option<String>,
    pub supports_vision: Option<bool>,
    pub error: Option<String>,
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
    fn dispose(&mut self) -> AppResult<()>;
}
