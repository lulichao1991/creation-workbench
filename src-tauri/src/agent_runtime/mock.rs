use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::runtime::{
    AgentRuntime, RuntimeEvent, RuntimeEventSink, RuntimeTaskHandle, RuntimeTaskInput,
    RuntimeTaskState,
};
use crate::database::{new_id, AppResult};

struct MockTask {
    cancelled: Arc<AtomicBool>,
    state: Arc<Mutex<RuntimeTaskState>>,
    event_sink: RuntimeEventSink,
}

pub struct MockRuntime {
    chunks: Vec<String>,
    delay: Duration,
    tasks: HashMap<String, MockTask>,
}

impl MockRuntime {
    pub fn new(chunks: Vec<String>, delay: Duration) -> Self {
        Self {
            chunks,
            delay,
            tasks: HashMap::new(),
        }
    }
}

impl AgentRuntime for MockRuntime {
    fn start_task(
        &mut self,
        input: RuntimeTaskInput,
        event_sink: RuntimeEventSink,
    ) -> AppResult<RuntimeTaskHandle> {
        let task_id = input.task_id.unwrap_or_else(new_id);
        let cancelled = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(RuntimeTaskState::Running));
        let worker_id = task_id.clone();
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_state = Arc::clone(&state);
        let worker_sink = Arc::clone(&event_sink);
        let chunks = self.chunks.clone();
        let delay = self.delay;
        event_sink(RuntimeEvent::TaskStarted {
            task_id: task_id.clone(),
        });
        thread::spawn(move || {
            for delta in chunks {
                thread::sleep(delay);
                if worker_cancelled.load(Ordering::Acquire) {
                    return;
                }
                worker_sink(RuntimeEvent::TextDelta {
                    task_id: worker_id.clone(),
                    delta,
                });
            }
            if let Ok(mut current) = worker_state.lock() {
                *current = RuntimeTaskState::Completed;
            }
            worker_sink(RuntimeEvent::TaskCompleted { task_id: worker_id });
        });
        self.tasks.insert(
            task_id.clone(),
            MockTask {
                cancelled,
                state,
                event_sink,
            },
        );
        Ok(RuntimeTaskHandle { task_id })
    }

    fn send_user_input(&mut self, task_id: &str, input: String) -> AppResult<()> {
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))?;
        (task.event_sink)(RuntimeEvent::TextDelta {
            task_id: task_id.to_string(),
            delta: input,
        });
        Ok(())
    }

    fn cancel_task(&mut self, task_id: &str) -> AppResult<()> {
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Agent 任务不存在：{task_id}"))?;
        task.cancelled.store(true, Ordering::Release);
        *task.state.lock().map_err(|_| "任务状态锁损坏")? = RuntimeTaskState::Cancelled;
        (task.event_sink)(RuntimeEvent::TaskCancelled {
            task_id: task_id.to_string(),
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
        for task in self.tasks.values() {
            task.cancelled.store(true, Ordering::Release);
        }
        self.tasks.clear();
        Ok(())
    }
}
