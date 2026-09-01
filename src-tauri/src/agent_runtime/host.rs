use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::runtime::{
    AgentModelCatalog, AgentRuntime, RuntimeDiagnostics, RuntimeEvent, RuntimeEventSink,
    RuntimeTaskHandle, RuntimeTaskInput, RuntimeTaskState,
};
use crate::agent_gateway::{execute_tool, ToolGatewayRequest};
use crate::database::{new_id, AppResult};

const MAX_TERMINAL_TASKS: usize = 256;

struct HostCommand {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    display: String,
}

struct HostTask {
    session_id: String,
    project_path: Option<PathBuf>,
    app_data_dir: Option<PathBuf>,
    state: RuntimeTaskState,
    sink: RuntimeEventSink,
}

type PendingResponse = mpsc::Sender<AppResult<Value>>;

struct HostProcess {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Arc<Mutex<Child>>,
    pending: Arc<Mutex<HashMap<String, PendingResponse>>>,
    tasks: Arc<Mutex<HashMap<String, HostTask>>>,
    terminal_tasks: Arc<Mutex<HashMap<String, RuntimeTaskState>>>,
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
        let mut process_command = Command::new(&command.program);
        process_command
            .args(&command.args)
            .env("WORKBENCH_AGENT_DATA_DIR", data_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(current_dir) = &command.current_dir {
            process_command.current_dir(current_dir);
        }
        let mut child = process_command.spawn().map_err(|error| {
            format!("无法启动 Pi SDK Agent Host（{}）：{error}", command.display)
        })?;
        let stdin = Arc::new(Mutex::new(
            child.stdin.take().ok_or("Agent Host stdin 不可用")?,
        ));
        let stdout = child.stdout.take().ok_or("Agent Host stdout 不可用")?;
        let mut stderr = child.stderr.take().ok_or("Agent Host stderr 不可用")?;
        let child = Arc::new(Mutex::new(child));
        let pending = Arc::new(Mutex::new(HashMap::<String, PendingResponse>::new()));
        let tasks = Arc::new(Mutex::new(HashMap::<String, HostTask>::new()));
        let terminal_tasks = Arc::new(Mutex::new(HashMap::<String, RuntimeTaskState>::new()));
        let reader_pending = Arc::clone(&pending);
        let reader_tasks = Arc::clone(&tasks);
        let reader_terminal_tasks = Arc::clone(&terminal_tasks);
        let reader_stdin = Arc::clone(&stdin);
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let reader_stderr_tail = Arc::clone(&stderr_tail);
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        if let Ok(value) = serde_json::from_str::<Value>(&line) {
                            handle_host_message(
                                value,
                                &reader_pending,
                                &reader_tasks,
                                &reader_terminal_tasks,
                                &reader_stdin,
                            );
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let stopped_error = host_stopped_error(&reader_stderr_tail);
            fail_live_tasks(&reader_tasks, &reader_terminal_tasks, &stopped_error);
            if let Ok(mut pending) = reader_pending.lock() {
                for (_, sender) in pending.drain() {
                    let _ = sender.send(Err(stopped_error.clone()));
                }
            }
        });
        let stderr_capture = Arc::clone(&stderr_tail);
        let stderr_reader = thread::spawn(move || {
            for line in BufReader::new(&mut stderr).lines().map_while(Result::ok) {
                if let Ok(mut tail) = stderr_capture.lock() {
                    if tail.len() + line.len() > 2_000 {
                        let trim = (tail.len() + line.len()) - 2_000;
                        let current_len = tail.len();
                        tail.drain(..trim.min(current_len));
                    }
                    if !tail.is_empty() {
                        tail.push(' ');
                    }
                    tail.push_str(&line);
                }
            }
        });
        Ok(Self {
            stdin,
            child,
            pending,
            tasks,
            terminal_tasks,
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
            let encoded = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
            let mut stdin = self
                .stdin
                .lock()
                .map_err(|_| "Agent Host stdin 锁损坏".to_string())?;
            stdin
                .write_all(&encoded)
                .and_then(|_| stdin.write_all(b"\n"))
                .and_then(|_| stdin.flush())
                .map_err(host_write_error)
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

    fn is_running(&self) -> AppResult<bool> {
        self.child
            .lock()
            .map_err(|_| "Agent Host 进程锁损坏".to_string())?
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|error| format!("检查 Agent Host 状态失败：{error}"))
    }

    fn shutdown(&mut self) {
        if self.is_running().unwrap_or(false) {
            let _ = self.request("shutdown", json!({}));
        }
        self.terminate();
    }

    fn terminate(&mut self) {
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
    sessions: HashMap<String, String>,
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
            sessions: HashMap::new(),
        }
    }

    pub(crate) fn for_resource_dir(resource_dir: PathBuf) -> Self {
        let bundled = bundled_host_command(&resource_dir);
        if bundled.program.is_file()
            && bundled
                .args
                .first()
                .is_some_and(|script| PathBuf::from(script).is_file())
        {
            Self::new(bundled)
        } else {
            Self::default()
        }
    }

    fn process(&mut self) -> AppResult<&HostProcess> {
        let stopped = match self.process.as_ref() {
            Some(process) => !process.is_running()?,
            None => false,
        };
        if stopped {
            if let Some(mut process) = self.process.take() {
                process.terminate();
            }
            self.sessions.clear();
        }
        if self.process.is_none() {
            self.process = Some(HostProcess::spawn(&self.command)?);
        }
        Ok(self.process.as_ref().expect("process initialized"))
    }

    fn management_request(&mut self, request_type: &str, body: Value) -> AppResult<Value> {
        let first = self.process()?.request(request_type, body.clone());
        let Err(first_error) = first else {
            return first;
        };
        if !is_restartable_host_error(&first_error) {
            return Err(first_error);
        }
        #[cfg(debug_assertions)]
        eprintln!("Agent Host 请求 {request_type} 中断，正在重启：{first_error}");
        if let Some(mut process) = self.process.take() {
            process.terminate();
        }
        self.sessions.clear();
        let retry = self
            .process()?
            .request(request_type, body)
            .map_err(|retry_error| format!("Agent Host 自动恢复失败：{retry_error}"));
        #[cfg(debug_assertions)]
        if let Err(retry_error) = &retry {
            eprintln!("Agent Host 请求 {request_type} 自动恢复失败：{retry_error}");
        }
        retry
    }
}

fn host_stopped_error(stderr_tail: &Mutex<String>) -> String {
    let detail = stderr_tail
        .lock()
        .ok()
        .map(|tail| tail.trim().replace(['\r', '\n'], " "))
        .filter(|tail| !tail.is_empty());
    match detail {
        Some(detail) => format!("Pi SDK Agent Host 已停止：{detail}"),
        None => "Pi SDK Agent Host 已停止".into(),
    }
}

fn is_restartable_host_error(error: &str) -> bool {
    error.contains("Agent Host 已停止")
        || error.contains("Agent Host stopped")
        || error.contains("broken pipe")
        || error.contains("管道")
}

fn host_write_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::BrokenPipe || error.raw_os_error() == Some(232) {
        "Pi SDK Agent Host 已停止，请重试当前操作".into()
    } else {
        format!("写入 Pi SDK Agent Host 失败：{error}")
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
        let runtime_session_id = if let Some(runtime_session_id) = self.sessions.get(&session_id) {
            runtime_session_id.clone()
        } else {
            let result = self.process()?.request(
                "create_session",
                json!({
                    "sessionId": session_id,
                    "runtimeSessionId": input.runtime_session_id,
                    "provider": input.provider,
                    "model": input.model,
                    "systemPrompt": input.system_prompt,
                    "thinkingLevel": input.thinking_level,
                    "allowedTools": input.allowed_tools,
                    "allowCallExpert": input.allow_call_expert,
                    "resultToolKind": input.result_tool_kind,
                }),
            )?;
            let runtime_session_id = result
                .get("runtimeSessionId")
                .and_then(Value::as_str)
                .ok_or_else(|| "Agent Host 未返回 runtimeSessionId".to_string())?
                .to_string();
            self.sessions
                .insert(session_id.clone(), runtime_session_id.clone());
            runtime_session_id
        };
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
                    project_path: input.project_path.map(PathBuf::from),
                    app_data_dir: input.app_data_dir.map(PathBuf::from),
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
        Ok(RuntimeTaskHandle {
            task_id,
            runtime_session_id: Some(runtime_session_id),
        })
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

    fn send_follow_up(&mut self, task_id: &str, input: String) -> AppResult<()> {
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
            "follow_up",
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

    fn close_session(&mut self, session_id: &str) -> AppResult<()> {
        if self.sessions.remove(session_id).is_none() {
            return Ok(());
        }
        if let Some(process) = self.process.as_ref() {
            process.request("dispose_session", json!({ "sessionId": session_id }))?;
        }
        Ok(())
    }

    fn get_task_state(&self, task_id: &str) -> AppResult<RuntimeTaskState> {
        let process = self
            .process
            .as_ref()
            .ok_or_else(|| "Agent Host 尚未启动".to_string())?;
        if let Some(state) = process
            .tasks
            .lock()
            .map_err(|_| "Agent Host task 锁损坏".to_string())?
            .get(task_id)
            .map(|task| task.state.clone())
        {
            return Ok(state);
        }
        process
            .terminal_tasks
            .lock()
            .map_err(|_| "Agent Host terminal task 锁损坏".to_string())?
            .get(task_id)
            .cloned()
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))
    }

    fn get_models(&mut self) -> AppResult<AgentModelCatalog> {
        let value = self.management_request("get_models", json!({}))?;
        serde_json::from_value(value).map_err(|error| format!("解析模型目录失败：{error}"))
    }

    fn login_provider(&mut self, provider_id: &str, api_key: &str) -> AppResult<()> {
        self.management_request(
            "login_provider",
            json!({ "providerId": provider_id, "apiKey": api_key }),
        )?;
        Ok(())
    }

    fn logout_provider(&mut self, provider_id: &str) -> AppResult<()> {
        self.management_request("logout_provider", json!({ "providerId": provider_id }))?;
        Ok(())
    }

    fn test_provider(
        &mut self,
        provider_id: &str,
    ) -> AppResult<crate::agent_runtime::runtime::ProviderConnectionTest> {
        let value =
            self.management_request("test_provider", json!({ "providerId": provider_id }))?;
        serde_json::from_value(value)
            .map_err(|error| format!("解析 Provider 连接测试失败：{error}"))
    }

    fn doctor(&mut self) -> AppResult<RuntimeDiagnostics> {
        let value = self.management_request("doctor", json!({}))?;
        serde_json::from_value(value)
            .map_err(|error| format!("解析 Agent Host Doctor 失败：{error}"))
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

fn handle_host_message(
    value: Value,
    pending: &Mutex<HashMap<String, PendingResponse>>,
    tasks: &Mutex<HashMap<String, HostTask>>,
    terminal_tasks: &Mutex<HashMap<String, RuntimeTaskState>>,
    stdin: &Mutex<ChildStdin>,
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
    if value.get("type").and_then(Value::as_str) == Some("tool_request") {
        handle_tool_request(value, tasks, stdin);
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
        "usage_updated" => RuntimeEvent::UsageUpdated {
            task_id: task_id.into(),
            usage: value.get("usage").cloned().unwrap_or(Value::Null),
        },
        "structured_result" => RuntimeEvent::StructuredResult {
            task_id: task_id.into(),
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
    let terminal_state = task.state.is_terminal().then(|| task.state.clone());
    if terminal_state.is_some() {
        task_map.remove(task_id);
    }
    drop(task_map);
    if let Some(state) = terminal_state {
        remember_terminal_task(terminal_tasks, task_id, state);
    }
    sink(event);
}

fn handle_tool_request(
    value: Value,
    tasks: &Mutex<HashMap<String, HostTask>>,
    stdin: &Mutex<ChildStdin>,
) {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let result = (|| -> AppResult<Value> {
        let task_id = value
            .get("taskId")
            .and_then(Value::as_str)
            .ok_or("Tool Gateway 请求缺少 taskId")?;
        let session_id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or("Tool Gateway 请求缺少 sessionId")?;
        let parent_task_id = value.get("parentTaskId").and_then(Value::as_str);
        let lookup_task_id = parent_task_id.unwrap_or(task_id);
        let (project_path, app_data_dir, expected_session_id) = {
            let tasks = tasks
                .lock()
                .map_err(|_| "Agent Host task 锁损坏".to_string())?;
            let task = tasks
                .get(lookup_task_id)
                .ok_or_else(|| format!("Agent 任务不存在：{lookup_task_id}"))?;
            (
                task.project_path
                    .clone()
                    .ok_or("当前 Agent 任务没有项目作用域")?,
                task.app_data_dir.clone(),
                task.session_id.clone(),
            )
        };
        if parent_task_id.is_none() && expected_session_id != session_id {
            return Err("Tool Gateway 请求的 Session 不匹配".into());
        }
        execute_tool(
            &project_path,
            app_data_dir.as_deref(),
            ToolGatewayRequest {
                tool_call_id: value
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .ok_or("Tool Gateway 请求缺少 toolCallId")?
                    .to_string(),
                task_id: task_id.to_string(),
                session_id: session_id.to_string(),
                tool_name: value
                    .get("toolName")
                    .and_then(Value::as_str)
                    .ok_or("Tool Gateway 请求缺少 toolName")?
                    .to_string(),
                arguments: value.get("arguments").cloned().unwrap_or_else(|| json!({})),
            },
        )
    })();
    let response = match result {
        Ok(result) => {
            json!({ "id": id, "type": "tool_response", "success": true, "result": result })
        }
        Err(error) => {
            json!({ "id": id, "type": "tool_response", "success": false, "error": error })
        }
    };
    if let Ok(mut stdin) = stdin.lock() {
        let _ = serde_json::to_writer(&mut *stdin, &response);
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
    }
}

fn fail_live_tasks(
    tasks: &Mutex<HashMap<String, HostTask>>,
    terminal_tasks: &Mutex<HashMap<String, RuntimeTaskState>>,
    error: &str,
) {
    let Ok(mut tasks) = tasks.lock() else {
        return;
    };
    let notifications = tasks
        .drain()
        .filter(|(_, task)| !task.state.is_terminal())
        .map(|(task_id, task)| {
            remember_terminal_task(terminal_tasks, &task_id, RuntimeTaskState::Failed);
            (task_id, Arc::clone(&task.sink))
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

fn remember_terminal_task(
    terminal_tasks: &Mutex<HashMap<String, RuntimeTaskState>>,
    task_id: &str,
    state: RuntimeTaskState,
) {
    let Ok(mut terminal_tasks) = terminal_tasks.lock() else {
        return;
    };
    if terminal_tasks.len() >= MAX_TERMINAL_TASKS {
        if let Some(oldest) = terminal_tasks.keys().next().cloned() {
            terminal_tasks.remove(&oldest);
        }
    }
    terminal_tasks.insert(task_id.to_string(), state);
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

fn bundled_host_command(resource_dir: &std::path::Path) -> HostCommand {
    let host_dir = resource_dir.join("agent-host");
    let program = host_dir.join(if cfg!(windows) { "node.exe" } else { "node" });
    let script = PathBuf::from("dist").join("index.js");
    HostCommand {
        display: format!("{} {}", program.display(), script.display()),
        program,
        args: vec![script.to_string_lossy().into_owned()],
        current_dir: Some(host_dir),
    }
}

fn command_for_path(path: PathBuf) -> HostCommand {
    if matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("js" | "mjs" | "cjs")
    ) {
        HostCommand {
            program: PathBuf::from("node"),
            args: vec![path.to_string_lossy().into_owned()],
            current_dir: None,
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
            current_dir: None,
            display: path.display().to_string(),
        }
    } else {
        HostCommand {
            program: path.clone(),
            args: Vec::new(),
            current_dir: None,
            display: path.display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::runtime::{RuntimeEvent, RuntimeTaskInput};
    use crate::database::{init_database, now, open_database};
    use std::sync::mpsc;

    #[test]
    fn javascript_host_uses_node_without_shell_string_interpolation() {
        let command = command_for_path(PathBuf::from(r"C:\含中文 空格\host.js"));
        assert_eq!(command.program, PathBuf::from("node"));
        assert_eq!(command.args, vec![r"C:\含中文 空格\host.js"]);
    }

    #[test]
    fn bundled_host_uses_private_node_runtime_and_resource_script() {
        let command = bundled_host_command(std::path::Path::new(r"C:\portable\resources"));
        assert_eq!(
            command.program,
            PathBuf::from(r"C:\portable\resources\agent-host\node.exe")
        );
        assert_eq!(command.args, vec![r"dist\index.js"]);
        assert_eq!(
            command.current_dir,
            Some(PathBuf::from(r"C:\portable\resources\agent-host"))
        );
    }

    #[test]
    fn terminal_task_cache_is_bounded() {
        let tasks = Mutex::new(HashMap::new());
        for index in 0..(MAX_TERMINAL_TASKS + 20) {
            remember_terminal_task(
                &tasks,
                &format!("task-{index}"),
                RuntimeTaskState::Completed,
            );
        }
        assert_eq!(tasks.lock().unwrap().len(), MAX_TERMINAL_TASKS);
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
                    runtime_session_id: None,
                    project_path: None,
                    app_data_dir: None,
                    prompt: "继续讨论".into(),
                    provider: None,
                    model: None,
                    system_prompt: None,
                    thinking_level: None,
                    allowed_tools: None,
                    allow_call_expert: None,
                    result_tool_kind: None,
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

    #[test]
    fn sdk_adapter_restarts_a_stopped_host_before_the_next_request() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-data")
            .join("含中文 空格")
            .join("mock_agent_host.mjs");
        let mut runtime = PiSdkRuntimeAdapter::new(command_for_path(fixture));
        assert_eq!(
            runtime.doctor().unwrap().sdk_version.as_deref(),
            Some("mock-sdk")
        );

        let process = runtime.process.as_ref().unwrap();
        let mut child = process.child.lock().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        drop(child);

        assert_eq!(
            runtime.doctor().unwrap().sdk_version.as_deref(),
            Some("mock-sdk")
        );
        runtime.dispose().unwrap();
    }

    #[test]
    fn sdk_adapter_retries_a_management_request_after_the_host_crashes() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("crashed-once.marker");
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-data")
            .join("含中文 空格")
            .join("mock_flaky_agent_host.mjs");
        let mut runtime = PiSdkRuntimeAdapter::new(HostCommand {
            program: PathBuf::from("node"),
            args: vec![
                fixture.to_string_lossy().into_owned(),
                marker.to_string_lossy().into_owned(),
            ],
            current_dir: None,
            display: "flaky fixture".into(),
        });

        assert_eq!(
            runtime.doctor().unwrap().sdk_version.as_deref(),
            Some("recovered-sdk")
        );
        runtime.dispose().unwrap();
    }

    #[test]
    fn sdk_adapter_round_trips_tool_calls_through_rust_gateway() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "Gateway IPC", "short").unwrap();
        let conn = open_database(temp.path()).unwrap();
        let timestamp = now();
        conn.execute(
            "INSERT INTO agent_sessions
             (id, project_id, scope_type, title, status, session_kind, session_status,
              last_active_at, created_at, updated_at)
             VALUES ('sdk-session', ?1, 'project', 'IPC', 'active', 'main', 'active', ?2, ?2, ?2)",
            rusqlite::params![project.id, timestamp],
        )
        .unwrap();
        let selection = json!({
            "projectId": project.id,
            "center": { "projectId": project.id, "objectType": "project", "objectId": project.id, "field": null },
            "selected": [],
            "projectRevision": 0,
        });
        conn.execute(
            "INSERT INTO agent_tasks
             (id, session_id, task_type, interaction_mode, agent_type, selection_json,
              read_scope_json, write_scope_json, context_revision, status, created_at)
             VALUES ('sdk-tool-task', 'sdk-session', 'discussion', 'discussion', 'main',
                     ?1, '[]', '{\"refs\":[],\"protectedRefs\":[]}', 0, 'queued', ?2)",
            rusqlite::params![selection.to_string(), timestamp],
        )
        .unwrap();
        drop(conn);

        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-data")
            .join("含中文 空格")
            .join("mock_tool_agent_host.mjs");
        let mut runtime = PiSdkRuntimeAdapter::new(command_for_path(fixture));
        let (tx, rx) = mpsc::channel();
        let sink: RuntimeEventSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        runtime
            .start_task(
                RuntimeTaskInput {
                    task_id: Some("sdk-tool-task".into()),
                    session_id: Some("sdk-session".into()),
                    runtime_session_id: None,
                    project_path: Some(temp.path().to_string_lossy().into_owned()),
                    app_data_dir: None,
                    prompt: "读取当前选区".into(),
                    provider: None,
                    model: None,
                    system_prompt: None,
                    thinking_level: None,
                    allowed_tools: None,
                    allow_call_expert: None,
                    result_tool_kind: None,
                    attachments: Vec::new(),
                },
                sink,
            )
            .unwrap();
        let mut events = Vec::new();
        while events.len() < 8 {
            let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let completed = matches!(event, RuntimeEvent::TaskCompleted { .. });
            events.push(event);
            if completed {
                break;
            }
        }
        assert!(events.iter().any(|event| matches!(event, RuntimeEvent::ToolCallRequested { tool_name, .. } if tool_name == "get_selection")));
        assert!(events.iter().any(|event| matches!(event, RuntimeEvent::ToolCallCompleted { tool_name, .. } if tool_name == "get_selection")));
        let text = events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::TextDelta { delta, .. } => Some(delta.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert!(text.contains("projectRevision"));
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM agent_tool_calls WHERE id='fixture-tool' AND status='completed'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        runtime.dispose().unwrap();
    }
}
