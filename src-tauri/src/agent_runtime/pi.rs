use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::runtime::{
    AgentRuntime, RuntimeAttachment, RuntimeDiagnostics, RuntimeEvent, RuntimeEventSink,
    RuntimeTaskHandle, RuntimeTaskInput, RuntimeTaskState,
};
use crate::database::{new_id, AppResult};
use base64::Engine;

struct PiTaskProcess {
    command_tx: mpsc::Sender<Value>,
    state: Arc<Mutex<RuntimeTaskState>>,
    child: Arc<Mutex<Child>>,
    event_sink: RuntimeEventSink,
    exited: Arc<AtomicBool>,
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
    stderr_reader: JoinHandle<()>,
}

struct PendingVisualPrompt {
    capability_request_id: String,
    prompt_command: Value,
}

pub struct PiRuntimeAdapter {
    executable: PathBuf,
    prefix_args: Vec<String>,
    tasks: HashMap<String, PiTaskProcess>,
    terminal_states: HashMap<String, RuntimeTaskState>,
}

impl Default for PiRuntimeAdapter {
    fn default() -> Self {
        Self::new(
            std::env::var_os("PI_AGENT_CLI")
                .map(PathBuf::from)
                .or_else(find_pi_on_path)
                .unwrap_or_else(|| PathBuf::from("pi")),
        )
    }
}

impl PiRuntimeAdapter {
    pub fn new(executable: PathBuf) -> Self {
        let (executable, prefix_args) = process_command(executable);
        Self {
            executable,
            prefix_args,
            tasks: HashMap::new(),
            terminal_states: HashMap::new(),
        }
    }

    fn cleanup_terminal_tasks(&mut self) {
        let completed = self
            .tasks
            .iter()
            .filter(|(_, task)| task.exited.load(Ordering::Acquire))
            .map(|(task_id, _)| task_id.clone())
            .collect::<Vec<_>>();
        for task_id in completed {
            let Some(task) = self.tasks.remove(&task_id) else {
                continue;
            };
            let state = task
                .state
                .lock()
                .map(|state| state.clone())
                .unwrap_or(RuntimeTaskState::Failed);
            drop(task.command_tx);
            let _ = task.writer.join();
            let _ = task.reader.join();
            let _ = task.stderr_reader.join();
            self.terminal_states.insert(task_id, state);
        }
    }

    #[cfg(test)]
    pub fn has_live_process(&self, task_id: &str) -> bool {
        self.tasks
            .get(task_id)
            .and_then(|task| task.child.lock().ok())
            .and_then(|mut child| child.try_wait().ok())
            .is_some_and(|status| status.is_none())
    }
}

impl AgentRuntime for PiRuntimeAdapter {
    fn start_task(
        &mut self,
        input: RuntimeTaskInput,
        event_sink: RuntimeEventSink,
    ) -> AppResult<RuntimeTaskHandle> {
        self.cleanup_terminal_tasks();
        if input.prompt.trim().is_empty() {
            return Err("Agent 任务内容不能为空".into());
        }
        let task_id = input.task_id.unwrap_or_else(new_id);
        if self.tasks.contains_key(&task_id) || self.terminal_states.contains_key(&task_id) {
            return Err(format!("Agent 任务已存在：{task_id}"));
        }
        if let Some(provider) = input.provider.as_deref() {
            validate_cli_selector("Provider", provider)?;
        }
        if let Some(model) = input.model.as_deref() {
            validate_cli_selector("模型", model)?;
        }
        validate_attachments(&input.attachments)?;
        let prompt_command = prompt_command(&task_id, &input.prompt, &input.attachments);

        let mut command = Command::new(&self.executable);
        command
            .args(&self.prefix_args)
            .args(["--mode", "rpc", "--no-session", "--no-tools"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(provider) = input.provider.as_deref() {
            command.args(["--provider", provider]);
        }
        if let Some(model) = input.model.as_deref() {
            command.args(["--model", model]);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("无法启动 Pi Runtime（{}）：{e}", self.executable.display()))?;
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Pi Runtime stdin 不可用".into());
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Pi Runtime stdout 不可用".into());
        };
        let Some(mut stderr) = child.stderr.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Pi Runtime stderr 不可用".into());
        };
        let stderr_text = Arc::new(Mutex::new(String::new()));
        let stderr_target = Arc::clone(&stderr_text);
        let stderr_reader = thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            if let Ok(mut target) = stderr_target.lock() {
                *target = text;
            }
        });
        let child = Arc::new(Mutex::new(child));
        let state = Arc::new(Mutex::new(RuntimeTaskState::Running));
        let exited = Arc::new(AtomicBool::new(false));
        let (command_tx, command_rx) = mpsc::channel::<Value>();
        let writer = thread::spawn(move || {
            while let Ok(command) = command_rx.recv() {
                if write_jsonl(&mut stdin, &command).is_err() {
                    break;
                }
            }
        });

        let pending_visual_prompt = if input.attachments.is_empty() {
            command_tx
                .send(prompt_command)
                .map_err(|_| "Pi Runtime 已停止".to_string())?;
            None
        } else {
            let capability_request_id = format!("capabilities-{task_id}");
            command_tx
                .send(json!({ "id": capability_request_id, "type": "get_state" }))
                .map_err(|_| "Pi Runtime 已停止".to_string())?;
            Some(PendingVisualPrompt {
                capability_request_id,
                prompt_command,
            })
        };

        event_sink(RuntimeEvent::TaskStarted {
            task_id: task_id.clone(),
        });
        let reader_task_id = task_id.clone();
        let reader_state = Arc::clone(&state);
        let reader_child = Arc::clone(&child);
        let reader_exited = Arc::clone(&exited);
        let reader_sink = Arc::clone(&event_sink);
        let reader_command_tx = command_tx.clone();
        let reader_stderr = Arc::clone(&stderr_text);
        let reader = thread::spawn(move || {
            read_pi_events(
                stdout,
                &reader_task_id,
                &reader_state,
                &reader_sink,
                &reader_command_tx,
                pending_visual_prompt,
                &reader_stderr,
            );
            terminate_child(&reader_child);
            reader_exited.store(true, Ordering::Release);
        });

        self.tasks.insert(
            task_id.clone(),
            PiTaskProcess {
                command_tx,
                state,
                child,
                event_sink,
                exited,
                reader,
                writer,
                stderr_reader,
            },
        );
        Ok(RuntimeTaskHandle {
            task_id,
            runtime_session_id: None,
        })
    }

    fn send_user_input(&mut self, task_id: &str, input: String) -> AppResult<()> {
        self.cleanup_terminal_tasks();
        if input.trim().is_empty() {
            return Err("追加输入不能为空".into());
        }
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))?;
        task.command_tx
            .send(json!({ "type": "prompt", "message": input, "streamingBehavior": "steer" }))
            .map_err(|_| "Pi Runtime 已停止".to_string())
    }

    fn send_follow_up(&mut self, task_id: &str, input: String) -> AppResult<()> {
        self.cleanup_terminal_tasks();
        if input.trim().is_empty() {
            return Err("追加输入不能为空".into());
        }
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))?;
        task.command_tx
            .send(json!({ "type": "prompt", "message": input, "streamingBehavior": "followUp" }))
            .map_err(|_| "Pi Runtime 已停止".to_string())
    }

    fn cancel_task(&mut self, task_id: &str) -> AppResult<()> {
        self.cleanup_terminal_tasks();
        if self.terminal_states.contains_key(task_id) {
            return Ok(());
        }
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))?;
        let mut state = task.state.lock().map_err(|_| "任务状态锁损坏")?;
        if state.is_terminal() {
            return Ok(());
        }
        task.command_tx
            .send(json!({ "id": format!("cancel-{task_id}"), "type": "abort" }))
            .map_err(|_| "Pi Runtime 已停止".to_string())?;
        *state = RuntimeTaskState::Cancelled;
        drop(state);
        (task.event_sink)(RuntimeEvent::TaskCancelled {
            task_id: task_id.to_string(),
        });

        let child = Arc::clone(&task.child);
        let exited = Arc::clone(&task.exited);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            if !exited.load(Ordering::Acquire) {
                terminate_child(&child);
                exited.store(true, Ordering::Release);
            }
        });
        Ok(())
    }

    fn close_session(&mut self, _session_id: &str) -> AppResult<()> {
        Ok(())
    }

    fn get_task_state(&self, task_id: &str) -> AppResult<RuntimeTaskState> {
        if let Some(task) = self.tasks.get(task_id) {
            return task
                .state
                .lock()
                .map(|state| state.clone())
                .map_err(|_| "任务状态锁损坏".into());
        }
        self.terminal_states
            .get(task_id)
            .cloned()
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))
    }

    fn dispose(&mut self) -> AppResult<()> {
        for (_, task) in self.tasks.drain() {
            terminate_child(&task.child);
            drop(task.command_tx);
            let _ = task.writer.join();
            let _ = task.reader.join();
            let _ = task.stderr_reader.join();
        }
        self.terminal_states.clear();
        Ok(())
    }
}

impl Drop for PiRuntimeAdapter {
    fn drop(&mut self) {
        let _ = self.dispose();
    }
}

pub(crate) fn diagnose_pi_runtime() -> RuntimeDiagnostics {
    let configured = std::env::var_os("PI_AGENT_CLI").map(PathBuf::from);
    let path = configured.or_else(find_pi_on_path);
    let Some(path) = path else {
        return RuntimeDiagnostics {
            found: false,
            executable_path: None,
            version: None,
            rpc_handshake: false,
            current_provider: None,
            current_model: None,
            supports_vision: None,
            error: Some("未找到 Pi；请安装 Pi 或设置 PI_AGENT_CLI".into()),
        };
    };
    diagnose_pi_runtime_at(path)
}

fn diagnose_pi_runtime_at(path: PathBuf) -> RuntimeDiagnostics {
    let display_path = path.to_string_lossy().into_owned();
    let (executable, prefix_args) = process_command(path);
    let version = Command::new(&executable)
        .args(&prefix_args)
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| {
            let text = if output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stderr).trim().to_string()
            } else {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            };
            (!text.is_empty()).then_some(text)
        });
    let mut command = Command::new(&executable);
    command
        .args(&prefix_args)
        .args(["--mode", "rpc", "--no-session", "--no-tools"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return RuntimeDiagnostics {
                found: true,
                executable_path: Some(display_path),
                version,
                rpc_handshake: false,
                current_provider: None,
                current_model: None,
                supports_vision: None,
                error: Some(format!("无法启动 Pi RPC：{error}")),
            }
        }
    };
    let result = (|| -> AppResult<Value> {
        let mut stdin = child.stdin.take().ok_or("Pi Runtime stdin 不可用")?;
        let stdout = child.stdout.take().ok_or("Pi Runtime stdout 不可用")?;
        write_jsonl(
            &mut stdin,
            &json!({ "id": "runtime-doctor", "type": "get_state" }),
        )?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "Pi RPC 握手超时".to_string())?;
        serde_json::from_str(line.trim()).map_err(|e| format!("Pi RPC 返回无效 JSON：{e}"))
    })();
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(value) if value.get("success") == Some(&Value::Bool(true)) => {
            let model = value.pointer("/data/model");
            let inputs = value.pointer("/data/model/input").and_then(Value::as_array);
            RuntimeDiagnostics {
                found: true,
                executable_path: Some(display_path),
                version,
                rpc_handshake: true,
                current_provider: model
                    .and_then(|value| value.get("provider"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                current_model: model
                    .and_then(|value| value.get("id").or_else(|| value.get("name")))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                supports_vision: inputs
                    .map(|items| items.iter().any(|input| input.as_str() == Some("image"))),
                error: None,
            }
        }
        Ok(value) => RuntimeDiagnostics {
            found: true,
            executable_path: Some(display_path),
            version,
            rpc_handshake: false,
            current_provider: None,
            current_model: None,
            supports_vision: None,
            error: Some(
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Pi RPC 拒绝握手")
                    .to_string(),
            ),
        },
        Err(error) => RuntimeDiagnostics {
            found: true,
            executable_path: Some(display_path),
            version,
            rpc_handshake: false,
            current_provider: None,
            current_model: None,
            supports_vision: None,
            error: Some(error),
        },
    }
}

fn write_jsonl(writer: &mut impl Write, value: &Value) -> AppResult<()> {
    serde_json::to_writer(&mut *writer, value).map_err(|e| format!("编码 JSONL 失败：{e}"))?;
    writer
        .write_all(b"\n")
        .and_then(|_| writer.flush())
        .map_err(|e| format!("写入 Pi Runtime 失败：{e}"))
}

fn read_pi_events(
    stdout: impl std::io::Read,
    task_id: &str,
    state: &Arc<Mutex<RuntimeTaskState>>,
    event_sink: &RuntimeEventSink,
    command_tx: &mpsc::Sender<Value>,
    mut pending_visual_prompt: Option<PendingVisualPrompt>,
    stderr: &Arc<Mutex<String>>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                let detail = stderr
                    .lock()
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                let message = detail
                    .map(|value| format!("Pi Runtime 意外退出：{value}"))
                    .unwrap_or_else(|| "Pi Runtime 意外退出".into());
                fail_if_running(task_id, state, event_sink, &message);
                break;
            }
            Ok(_) => {}
            Err(error) => {
                fail_if_running(
                    task_id,
                    state,
                    event_sink,
                    &format!("读取 Pi Runtime 失败：{error}"),
                );
                break;
            }
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                fail_if_running(
                    task_id,
                    state,
                    event_sink,
                    &format!("Pi Runtime 返回无效 JSONL：{error}"),
                );
                break;
            }
        };
        if let Some(pending) = pending_visual_prompt.as_ref() {
            if value.get("id").and_then(Value::as_str)
                == Some(pending.capability_request_id.as_str())
            {
                if value.get("success") != Some(&Value::Bool(true)) {
                    fail_if_running(
                        task_id,
                        state,
                        event_sink,
                        "MODEL_CAPABILITY_UNKNOWN: 无法确认当前 Pi 模型的视觉能力",
                    );
                    break;
                }
                let supports_vision = value
                    .pointer("/data/model/input")
                    .and_then(Value::as_array)
                    .is_some_and(|inputs| {
                        inputs.iter().any(|input| input.as_str() == Some("image"))
                    });
                if !supports_vision {
                    fail_if_running(
                        task_id,
                        state,
                        event_sink,
                        "MODEL_CAPABILITY_MISSING: 当前 Pi 模型不支持视觉输入，请更换支持 image 输入的模型",
                    );
                    break;
                }
                let pending = pending_visual_prompt.take().expect("pending visual prompt");
                if command_tx.send(pending.prompt_command).is_err() {
                    fail_if_running(task_id, state, event_sink, "Pi Runtime 已停止");
                    break;
                }
                continue;
            }
        }
        if handle_pi_event(task_id, value, state, event_sink) {
            break;
        }
    }
}

fn prompt_command(task_id: &str, prompt: &str, attachments: &[RuntimeAttachment]) -> Value {
    let mut command = json!({ "id": task_id, "type": "prompt", "message": prompt });
    if !attachments.is_empty() {
        command["images"] = Value::Array(
            attachments
                .iter()
                .map(|attachment| {
                    json!({
                        "type": "image",
                        "data": attachment.data,
                        "mimeType": attachment.mime_type,
                    })
                })
                .collect(),
        );
    }
    command
}

fn validate_attachments(attachments: &[RuntimeAttachment]) -> AppResult<()> {
    if attachments.len() > 8 {
        return Err("TOOL_ARGUMENT_INVALID: Agent 视觉附件最多 8 张".into());
    }
    let mut total_bytes = 0usize;
    for attachment in attachments {
        if !matches!(
            attachment.mime_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp"
        ) {
            return Err(format!(
                "TOOL_ARGUMENT_INVALID: 不支持的 Agent 视觉附件类型：{}",
                attachment.mime_type
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&attachment.data)
            .map_err(|_| "TOOL_ARGUMENT_INVALID: Agent 视觉附件不是有效 Base64".to_string())?;
        if bytes.len() > 20 * 1024 * 1024 {
            return Err("TOOL_ARGUMENT_INVALID: 单张 Agent 视觉附件不得超过 20MB".into());
        }
        total_bytes += bytes.len();
    }
    if total_bytes > 60 * 1024 * 1024 {
        return Err("TOOL_ARGUMENT_INVALID: Agent 视觉附件总大小不得超过 60MB".into());
    }
    Ok(())
}

fn handle_pi_event(
    task_id: &str,
    value: Value,
    state: &Arc<Mutex<RuntimeTaskState>>,
    event_sink: &RuntimeEventSink,
) -> bool {
    match value.get("type").and_then(Value::as_str) {
        Some("response") if value.get("success") == Some(&Value::Bool(false)) => {
            let error = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Pi Runtime 拒绝任务");
            fail_if_running(task_id, state, event_sink, error);
            true
        }
        Some("message_update") => {
            let update = &value["assistantMessageEvent"];
            if update.get("type").and_then(Value::as_str) == Some("text_delta") {
                if let Some(delta) = update.get("delta").and_then(Value::as_str) {
                    event_sink(RuntimeEvent::TextDelta {
                        task_id: task_id.to_string(),
                        delta: delta.to_string(),
                    });
                }
            }
            false
        }
        Some("tool_execution_start") => {
            event_sink(RuntimeEvent::ToolCallRequested {
                task_id: task_id.to_string(),
                tool_name: value
                    .get("toolName")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                arguments: value.get("args").cloned().unwrap_or(Value::Null),
            });
            false
        }
        Some("tool_execution_end") => {
            event_sink(RuntimeEvent::ToolCallCompleted {
                task_id: task_id.to_string(),
                tool_name: value
                    .get("toolName")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                result: value.get("result").cloned().unwrap_or(Value::Null),
            });
            false
        }
        Some("usage_update") => {
            event_sink(RuntimeEvent::UsageUpdated {
                task_id: task_id.to_string(),
                usage: value.get("usage").cloned().unwrap_or(Value::Null),
            });
            false
        }
        Some("agent_end") => {
            if let Ok(mut current) = state.lock() {
                if *current == RuntimeTaskState::Running {
                    *current = RuntimeTaskState::Completed;
                    event_sink(RuntimeEvent::TaskCompleted {
                        task_id: task_id.to_string(),
                    });
                }
            }
            true
        }
        _ => false,
    }
}

fn fail_if_running(
    task_id: &str,
    state: &Arc<Mutex<RuntimeTaskState>>,
    event_sink: &RuntimeEventSink,
    error: &str,
) {
    if let Ok(mut current) = state.lock() {
        if *current == RuntimeTaskState::Running {
            *current = RuntimeTaskState::Failed;
            event_sink(RuntimeEvent::TaskFailed {
                task_id: task_id.to_string(),
                error: error.to_string(),
            });
        }
    }
}

fn terminate_child(child: &Arc<Mutex<Child>>) {
    if let Ok(mut child) = child.lock() {
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

fn validate_cli_selector(label: &str, value: &str) -> AppResult<()> {
    let valid = !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '-' | '_' | '.' | '/' | ':' | '@')
        });
    if !valid {
        return Err(format!("{label} 名称包含不允许的字符"));
    }
    Ok(())
}

#[cfg(windows)]
fn find_pi_on_path() -> Option<PathBuf> {
    let output = Command::new("where.exe").arg("pi").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(windows))]
fn find_pi_on_path() -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn process_command(executable: PathBuf) -> (PathBuf, Vec<String>) {
    let is_script = executable
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("cmd") || value.eq_ignore_ascii_case("bat")
        });
    if is_script {
        let command_interpreter = std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("cmd.exe"));
        return (
            command_interpreter,
            vec![
                "/D".into(),
                "/C".into(),
                "call".into(),
                executable.to_string_lossy().into_owned(),
            ],
        );
    }
    (executable, Vec::new())
}

#[cfg(not(windows))]
fn process_command(executable: PathBuf) -> (PathBuf, Vec<String>) {
    (executable, Vec::new())
}
