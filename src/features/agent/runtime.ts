export interface RuntimeTaskInput {
  taskId?: string;
  prompt: string;
  provider?: string;
  model?: string;
}

export interface RuntimeTaskHandle {
  taskId: string;
}

export type RuntimeTaskState = "running" | "completed" | "cancelled" | "failed";

export type RuntimeEvent =
  | { type: "task_started"; task_id: string }
  | { type: "text_delta"; task_id: string; delta: string }
  | { type: "tool_call_requested"; task_id: string; tool_name: string; arguments: unknown }
  | { type: "tool_call_completed"; task_id: string; tool_name: string; result: unknown }
  | { type: "usage_updated"; task_id: string; usage: unknown }
  | { type: "task_completed"; task_id: string }
  | { type: "task_failed"; task_id: string; error: string }
  | { type: "task_cancelled"; task_id: string };

export interface AgentRuntime {
  startTask(input: RuntimeTaskInput): Promise<RuntimeTaskHandle>;
  sendUserInput(taskId: string, input: string): Promise<void>;
  cancelTask(taskId: string): Promise<void>;
  getTaskState(taskId: string): Promise<RuntimeTaskState>;
  dispose(): Promise<void>;
}

export const runtimeEventName = "agent-runtime-event";
