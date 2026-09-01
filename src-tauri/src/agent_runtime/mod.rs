mod host;
mod pi;
mod runtime;

#[cfg(test)]
pub(crate) mod mock;

use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

use crate::app_database::load_feature_flags;
use crate::database::AppResult;
use host::{diagnose_pi_sdk_host, PiSdkRuntimeAdapter};
use pi::{diagnose_pi_runtime, PiRuntimeAdapter};
use runtime::AgentRuntime;

pub use runtime::{
    RuntimeAttachment, RuntimeDiagnostics, RuntimeTaskHandle, RuntimeTaskInput, RuntimeTaskState,
};
pub(crate) use runtime::{RuntimeEvent, RuntimeEventSink};

pub const RUNTIME_EVENT_NAME: &str = "agent-runtime-event";

#[derive(Clone)]
pub struct RuntimeState {
    runtime: Arc<Mutex<Box<dyn AgentRuntime>>>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        let runtime: Box<dyn AgentRuntime> = if use_pi_sdk_runtime() {
            Box::new(PiSdkRuntimeAdapter::default())
        } else {
            Box::new(PiRuntimeAdapter::default())
        };
        Self {
            runtime: Arc::new(Mutex::new(runtime)),
        }
    }
}

impl RuntimeState {
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
pub fn agent_runtime_doctor() -> RuntimeDiagnostics {
    if use_pi_sdk_runtime() {
        diagnose_pi_sdk_host()
    } else {
        diagnose_pi_runtime()
    }
}

fn use_pi_sdk_runtime() -> bool {
    std::env::var("WORKBENCH_AGENT_RUNTIME")
        .map(|value| value.eq_ignore_ascii_case("pi_sdk"))
        .unwrap_or(false)
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
    use crate::agent_runtime::pi::PiRuntimeAdapter;
    use crate::agent_runtime::runtime::{AgentRuntime, RuntimeEvent, RuntimeTaskInput};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn input(task_id: &str, prompt: &str) -> RuntimeTaskInput {
        RuntimeTaskInput {
            task_id: Some(task_id.into()),
            session_id: None,
            prompt: prompt.into(),
            provider: None,
            model: None,
            system_prompt: None,
            thinking_level: None,
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

    #[test]
    fn pi_adapter_rejects_unsafe_command_selectors_before_spawn() {
        let mut runtime = PiRuntimeAdapter::new(PathBuf::from("missing-pi"));
        let sink: RuntimeEventSink = Arc::new(|_| {});
        let mut unsafe_input = input("unsafe", "test");
        unsafe_input.model = Some("model & calc.exe".into());
        let error = runtime.start_task(unsafe_input, sink).unwrap_err();
        assert!(error.contains("不允许的字符"));
    }

    #[test]
    #[cfg(windows)]
    fn pi_adapter_checks_vision_capability_and_sends_images() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-data")
            .join("含中文 空格")
            .join("mock_pi.cmd");
        let (tx, rx) = mpsc::channel();
        let sink: RuntimeEventSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let mut runtime = PiRuntimeAdapter::new(fixture);
        let mut visual = input("pi-vision", "检查构图");
        visual.attachments.push(RuntimeAttachment {
            name: "构图参考.png".into(),
            mime_type: "image/png".into(),
            data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".into(),
        });
        runtime.start_task(visual, sink).unwrap();
        let mut text = String::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeEvent::TextDelta { delta, .. } => text.push_str(&delta),
                RuntimeEvent::TaskCompleted { .. } => break,
                RuntimeEvent::TaskFailed { error, .. } => panic!("{error}"),
                _ => {}
            }
        }
        assert!(text.contains("视觉附件1张"));
    }

    #[test]
    #[cfg(windows)]
    fn pi_adapter_streams_and_cancels_sidecar_from_unicode_space_path() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-data")
            .join("含中文 空格")
            .join("mock_pi.cmd");
        let (tx, rx) = mpsc::channel();
        let sink: RuntimeEventSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let mut runtime = PiRuntimeAdapter::new(fixture);
        runtime
            .start_task(input("pi-stream", "镜头04"), Arc::clone(&sink))
            .unwrap();
        let mut text = String::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeEvent::TextDelta { delta, .. } => text.push_str(&delta),
                RuntimeEvent::TaskCompleted { .. } => break,
                _ => {}
            }
        }
        assert_eq!(text, "围绕镜头04的只读回答");
        assert_eq!(
            runtime.get_task_state("pi-stream").unwrap(),
            RuntimeTaskState::Completed
        );

        runtime
            .start_task(input("pi-cancel", "slow"), Arc::clone(&sink))
            .unwrap();
        runtime.cancel_task("pi-cancel").unwrap();
        thread::sleep(Duration::from_millis(700));
        assert!(!runtime.has_live_process("pi-cancel"));
        assert_eq!(
            runtime.get_task_state("pi-cancel").unwrap(),
            RuntimeTaskState::Cancelled
        );
    }
}
