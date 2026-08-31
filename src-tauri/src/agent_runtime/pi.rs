use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::runtime::{
    AgentRuntime, RuntimeEvent, RuntimeEventSink, RuntimeTaskHandle, RuntimeTaskInput,
    RuntimeTaskState,
};
use crate::database::{new_id, AppResult};

struct PiTaskProcess {
    command_tx: mpsc::Sender<Value>,
    state: Arc<Mutex<RuntimeTaskState>>,
    child: Arc<Mutex<Child>>,
    event_sink: RuntimeEventSink,
    exited: Arc<AtomicBool>,
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
}

pub struct PiRuntimeAdapter {
    executable: PathBuf,
    prefix_args: Vec<String>,
    tasks: HashMap<String, PiTaskProcess>,
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
        if input.prompt.trim().is_empty() {
            return Err("Agent 任务内容不能为空".into());
        }
        let task_id = input.task_id.unwrap_or_else(new_id);
        if self.tasks.contains_key(&task_id) {
            return Err(format!("Agent 任务已存在：{task_id}"));
        }
        if let Some(provider) = input.provider.as_deref() {
            validate_cli_selector("Provider", provider)?;
        }
        if let Some(model) = input.model.as_deref() {
            validate_cli_selector("模型", model)?;
        }

        let mut command = Command::new(&self.executable);
        command
            .args(&self.prefix_args)
            .args(["--mode", "rpc", "--no-session", "--no-tools"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
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
        if let Err(error) = write_jsonl(
            &mut stdin,
            &json!({ "id": task_id, "type": "prompt", "message": input.prompt }),
        ) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }

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

        event_sink(RuntimeEvent::TaskStarted {
            task_id: task_id.clone(),
        });
        let reader_task_id = task_id.clone();
        let reader_state = Arc::clone(&state);
        let reader_child = Arc::clone(&child);
        let reader_exited = Arc::clone(&exited);
        let reader_sink = Arc::clone(&event_sink);
        let reader = thread::spawn(move || {
            read_pi_events(stdout, &reader_task_id, &reader_state, &reader_sink);
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
            },
        );
        Ok(RuntimeTaskHandle { task_id })
    }

    fn send_user_input(&mut self, task_id: &str, input: String) -> AppResult<()> {
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

    fn cancel_task(&mut self, task_id: &str) -> AppResult<()> {
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

    fn get_task_state(&self, task_id: &str) -> AppResult<RuntimeTaskState> {
        self.tasks
            .get(task_id)
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))?
            .state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "任务状态锁损坏".into())
    }

    fn dispose(&mut self) -> AppResult<()> {
        for (_, task) in self.tasks.drain() {
            terminate_child(&task.child);
            drop(task.command_tx);
            let _ = task.writer.join();
            let _ = task.reader.join();
        }
        Ok(())
    }
}

impl Drop for PiRuntimeAdapter {
    fn drop(&mut self) {
        let _ = self.dispose();
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
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                fail_if_running(task_id, state, event_sink, "Pi Runtime 意外退出");
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
        if handle_pi_event(task_id, value, state, event_sink) {
            break;
        }
    }
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
