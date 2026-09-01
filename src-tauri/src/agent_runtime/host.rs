use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::runtime::{
    AgentRuntime, RuntimeDiagnostics, RuntimeEvent, RuntimeEventSink, RuntimeTaskHandle,
    RuntimeTaskInput, RuntimeTaskState,
};
use crate::database::{new_id, AppResult};

struct HostCommand {
    program: PathBuf,
    args: Vec<String>,
    display: String,
}

struct HostTask {
    session_id: String,
    state: RuntimeTaskState,
    sink: RuntimeEventSink,
}

type PendingResponse = mpsc::Sender<AppResult<Value>>;

struct HostProcess {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Arc<Mutex<Child>>,
    pending: Arc<Mutex<HashMap<String, PendingResponse>>>,
    tasks: Arc<Mutex<HashMap<String, HostTask>>>,
    request_id: AtomicU64,
    reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl HostProcess {
    fn spawn(command: &HostCommand) -> AppResult<Self> {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("creation-workbench")
            .join("agent-host");
        let mut child = Command::new(&command.program)
            .args(&command.args)
            .env("WORKBENCH_AGENT_DATA_DIR", data_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                format!("无法启动 Pi SDK Agent Host（{}）：{error}", command.display)
            })?;
        let stdin = child.stdin.take().ok_or("Agent Host stdin 不可用")?;
        let stdout = child.stdout.take().ok_or("Agent Host stdout 不可用")?;
        let mut stderr = child.stderr.take().ok_or("Agent Host stderr 不可用")?;
        let child = Arc::new(Mutex::new(child));
        let pending = Arc::new(Mutex::new(HashMap::<String, PendingResponse>::new()));
        let tasks = Arc::new(Mutex::new(HashMap::<String, HostTask>::new()));
        let reader_pending = Arc::clone(&pending);
        let reader_tasks = Arc::clone(&tasks);
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        if let Ok(value) = serde_json::from_str::<Value>(&line) {
                            handle_host_message(value, &reader_pending, &reader_tasks);
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            fail_live_tasks(&reader_tasks, "Pi SDK Agent Host 已停止");
            if let Ok(mut pending) = reader_pending.lock() {
                for (_, sender) in pending.drain() {
                    let _ = sender.send(Err("Pi SDK Agent Host 已停止".into()));
                }
            }
        });
        let stderr_reader = thread::spawn(move || {
            let mut ignored = String::new();
            let _ = stderr.read_to_string(&mut ignored);
        });
        Ok(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            child,
            pending,
            tasks,
            request_id: AtomicU64::new(1),
            reader: Some(reader),
            stderr_reader: Some(stderr_reader),
        })
    }

    fn request(&self, request_type: &str, mut body: Value) -> AppResult<Value> {
        let id = format!("rust-{}", self.request_id.fetch_add(1, Ordering::Relaxed));
        body["id"] = json!(id);
        body["type"] = json!(request_type);
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "Agent Host pending 锁损坏".to_string())?
            .insert(id.clone(), tx);
        let write_result = (|| -> AppResult<()> {
            let mut stdin = self
                .stdin
                .lock()
                .map_err(|_| "Agent Host stdin 锁损坏".to_string())?;
            serde_json::to_writer(&mut *stdin, &body).map_err(|error| error.to_string())?;
            stdin.write_all(b"\n").map_err(|error| error.to_string())?;
            stdin.flush().map_err(|error| error.to_string())
        })();
        if let Err(error) = write_result {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&id);
            }
            return Err(error);
        }
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| format!("Agent Host 请求超时：{request_type}"))?
    }

    fn shutdown(&mut self) {
        let _ = self.request("shutdown", json!({}));
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(stderr_reader) = self.stderr_reader.take() {
            let _ = stderr_reader.join();
        }
    }
}

pub struct PiSdkRuntimeAdapter {
    command: HostCommand,
    process: Option<HostProcess>,
    sessions: HashSet<String>,
}

impl Default for PiSdkRuntimeAdapter {
    fn default() -> Self {
        Self::new(resolve_host_command())
    }
}

impl PiSdkRuntimeAdapter {
    fn new(command: HostCommand) -> Self {
        Self {
            command,
            process: None,
            sessions: HashSet::new(),
        }
    }

    fn process(&mut self) -> AppResult<&HostProcess> {
        if self.process.is_none() {
            self.process = Some(HostProcess::spawn(&self.command)?);
        }
        Ok(self.process.as_ref().expect("process initialized"))
    }
}

impl AgentRuntime for PiSdkRuntimeAdapter {
    fn start_task(
        &mut self,
        input: RuntimeTaskInput,
        event_sink: RuntimeEventSink,
    ) -> AppResult<RuntimeTaskHandle> {
        if input.prompt.trim().is_empty() {
            return Err("Agent 任务内容不能为空".into());
        }
        let task_id = input.task_id.unwrap_or_else(new_id);
        let session_id = input.session_id.unwrap_or_else(|| task_id.clone());
        if !self.sessions.contains(&session_id) {
            let process = self.process()?;
            process.request(
                "create_session",
                json!({
                    "sessionId": session_id,
                    "provider": input.provider,
                    "model": input.model,
                    "systemPrompt": input.system_prompt,
                    "thinkingLevel": input.thinking_level,
                }),
            )?;
            self.sessions.insert(session_id.clone());
        }
        let process = self.process()?;
        {
            let mut tasks = process
                .tasks
                .lock()
                .map_err(|_| "Agent Host task 锁损坏".to_string())?;
            if tasks.contains_key(&task_id) {
                return Err(format!("Agent 任务已存在：{task_id}"));
            }
            tasks.insert(
                task_id.clone(),
                HostTask {
                    session_id: session_id.clone(),
                    state: RuntimeTaskState::Running,
                    sink: event_sink,
                },
            );
        }
        let images = input
            .attachments
            .into_iter()
            .map(|attachment| {
                json!({
                    "type": "image",
                    "data": attachment.data,
                    "mimeType": attachment.mime_type,
                    "name": attachment.name,
                })
            })
            .collect::<Vec<_>>();
        if let Err(error) = process.request(
            "send_message",
            json!({
                "sessionId": session_id,
                "taskId": task_id,
                "message": input.prompt,
                "images": images,
            }),
        ) {
            if let Ok(mut tasks) = process.tasks.lock() {
                tasks.remove(&task_id);
            }
            return Err(error);
        }
        Ok(RuntimeTaskHandle { task_id })
    }

    fn send_user_input(&mut self, task_id: &str, input: String) -> AppResult<()> {
        if input.trim().is_empty() {
            return Err("追加输入不能为空".into());
        }
        let process = self.process()?;
        let session_id = process
            .tasks
            .lock()
            .map_err(|_| "Agent Host task 锁损坏".to_string())?
            .get(task_id)
            .map(|task| task.session_id.clone())
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))?;
        process.request(
            "steer",
            json!({ "sessionId": session_id, "message": input }),
        )?;
        Ok(())
    }

    fn cancel_task(&mut self, task_id: &str) -> AppResult<()> {
        let process = self.process()?;
        let session_id = process
            .tasks
            .lock()
            .map_err(|_| "Agent Host task 锁损坏".to_string())?
            .get(task_id)
            .map(|task| task.session_id.clone())
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))?;
        process.request("cancel", json!({ "sessionId": session_id }))?;
        Ok(())
    }

    fn get_task_state(&self, task_id: &str) -> AppResult<RuntimeTaskState> {
        self.process
            .as_ref()
            .ok_or_else(|| "Agent Host 尚未启动".to_string())?
            .tasks
            .lock()
            .map_err(|_| "Agent Host task 锁损坏".to_string())?
            .get(task_id)
            .map(|task| task.state.clone())
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))
    }

    fn dispose(&mut self) -> AppResult<()> {
        if let Some(mut process) = self.process.take() {
            process.shutdown();
        }
        self.sessions.clear();
        Ok(())
    }
}

impl Drop for PiSdkRuntimeAdapter {
    fn drop(&mut self) {
        let _ = self.dispose();
    }
}

pub fn diagnose_pi_sdk_host() -> RuntimeDiagnostics {
    let command = resolve_host_command();
    match HostProcess::spawn(&command) {
        Ok(mut process) => {
            let result = process.request("doctor", json!({}));
            process.shutdown();
            match result {
                Ok(value) => RuntimeDiagnostics {
                    found: true,
                    executable_path: Some(command.display),
                    version: value
                        .get("sdkVersion")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    rpc_handshake: value.get("healthy").and_then(Value::as_bool) == Some(true),
                    current_provider: None,
                    current_model: None,
                    supports_vision: None,
                    error: None,
                },
                Err(error) => RuntimeDiagnostics {
                    found: true,
                    executable_path: Some(command.display),
                    version: None,
                    rpc_handshake: false,
                    current_provider: None,
                    current_model: None,
                    supports_vision: None,
                    error: Some(error),
                },
            }
        }
        Err(error) => RuntimeDiagnostics {
            found: false,
            executable_path: Some(command.display),
            version: None,
            rpc_handshake: false,
            current_provider: None,
            current_model: None,
            supports_vision: None,
            error: Some(error),
        },
    }
}

fn handle_host_message(
    value: Value,
    pending: &Mutex<HashMap<String, PendingResponse>>,
    tasks: &Mutex<HashMap<String, HostTask>>,
) {
    if value.get("type").and_then(Value::as_str) == Some("response") {
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            return;
        };
        let sender = pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(id));
        if let Some(sender) = sender {
            let result = if value.get("success").and_then(Value::as_bool) == Some(true) {
                Ok(value.get("result").cloned().unwrap_or(Value::Null))
            } else {
                Err(value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Agent Host 请求失败")
                    .to_string())
            };
            let _ = sender.send(result);
        }
        return;
    }
    if value.get("type").and_then(Value::as_str) != Some("event") {
        return;
    }
    let Some(task_id) = value.get("taskId").and_then(Value::as_str) else {
        return;
    };
    let Some(event_name) = value.get("event").and_then(Value::as_str) else {
        return;
    };
    let Ok(mut task_map) = tasks.lock() else {
        return;
    };
    let Some(task) = task_map.get_mut(task_id) else {
        return;
    };
    let event = match event_name {
        "task_started" => RuntimeEvent::TaskStarted {
            task_id: task_id.into(),
        },
        "message_delta" => RuntimeEvent::TextDelta {
            task_id: task_id.into(),
            delta: value
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        },
        "tool_call_requested" => RuntimeEvent::ToolCallRequested {
            task_id: task_id.into(),
            tool_name: value
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            arguments: value.get("arguments").cloned().unwrap_or(Value::Null),
        },
        "tool_call_completed" => RuntimeEvent::ToolCallCompleted {
            task_id: task_id.into(),
            tool_name: value
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            result: value.get("result").cloned().unwrap_or(Value::Null),
        },
        "task_completed" => {
            task.state = RuntimeTaskState::Completed;
            RuntimeEvent::TaskCompleted {
                task_id: task_id.into(),
            }
        }
        "task_cancelled" => {
            task.state = RuntimeTaskState::Cancelled;
            RuntimeEvent::TaskCancelled {
                task_id: task_id.into(),
            }
        }
        "task_failed" => {
            task.state = RuntimeTaskState::Failed;
            RuntimeEvent::TaskFailed {
                task_id: task_id.into(),
                error: value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Agent Host 任务失败")
                    .into(),
            }
        }
        _ => return,
    };
    let sink = Arc::clone(&task.sink);
    drop(task_map);
    sink(event);
}

fn fail_live_tasks(tasks: &Mutex<HashMap<String, HostTask>>, error: &str) {
    let Ok(mut tasks) = tasks.lock() else {
        return;
    };
    let notifications = tasks
        .iter_mut()
        .filter(|(_, task)| !task.state.is_terminal())
        .map(|(task_id, task)| {
            task.state = RuntimeTaskState::Failed;
            (task_id.clone(), Arc::clone(&task.sink))
        })
        .collect::<Vec<_>>();
    drop(tasks);
    for (task_id, sink) in notifications {
        sink(RuntimeEvent::TaskFailed {
            task_id,
            error: error.into(),
        });
    }
}

fn resolve_host_command() -> HostCommand {
    if let Some(configured) = std::env::var_os("WORKBENCH_AGENT_HOST").map(PathBuf::from) {
        return command_for_path(configured);
    }
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("agent-host")
        .join("dist")
        .join("index.js");
    command_for_path(script)
}

fn command_for_path(path: PathBuf) -> HostCommand {
    if matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("js" | "mjs" | "cjs")
    ) {
        HostCommand {
            program: PathBuf::from("node"),
            args: vec![path.to_string_lossy().into_owned()],
            display: format!("node {}", path.display()),
        }
    } else if cfg!(windows)
        && matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("cmd" | "bat")
        )
    {
        HostCommand {
            program: PathBuf::from("cmd.exe"),
            args: vec![
                "/d".into(),
                "/s".into(),
                "/c".into(),
                path.to_string_lossy().into_owned(),
            ],
            display: path.display().to_string(),
        }
    } else {
        HostCommand {
            program: path.clone(),
            args: Vec::new(),
            display: path.display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::runtime::{RuntimeEvent, RuntimeTaskInput};
    use std::sync::mpsc;

    #[test]
    fn javascript_host_uses_node_without_shell_string_interpolation() {
        let command = command_for_path(PathBuf::from(r"C:\含中文 空格\host.js"));
        assert_eq!(command.program, PathBuf::from("node"));
        assert_eq!(command.args, vec![r"C:\含中文 空格\host.js"]);
    }

    #[test]
    fn sdk_adapter_bridges_persistent_host_events() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-data")
            .join("含中文 空格")
            .join("mock_agent_host.mjs");
        let mut runtime = PiSdkRuntimeAdapter::new(command_for_path(fixture));
        let (tx, rx) = mpsc::channel();
        let sink: RuntimeEventSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        runtime
            .start_task(
                RuntimeTaskInput {
                    task_id: Some("sdk-task".into()),
                    session_id: Some("sdk-session".into()),
                    prompt: "继续讨论".into(),
                    provider: None,
                    model: None,
                    system_prompt: None,
                    thinking_level: None,
                    attachments: Vec::new(),
                },
                sink,
            )
            .unwrap();
        let events = (0..4)
            .map(|_| rx.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect::<Vec<_>>();
        assert!(matches!(events[0], RuntimeEvent::TaskStarted { .. }));
        let text = events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::TextDelta { delta, .. } => Some(delta.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "Pi SDK 原生会话");
        assert!(matches!(events[3], RuntimeEvent::TaskCompleted { .. }));
        assert_eq!(
            runtime.get_task_state("sdk-task").unwrap(),
            RuntimeTaskState::Completed
        );
        runtime.dispose().unwrap();
    }
}
