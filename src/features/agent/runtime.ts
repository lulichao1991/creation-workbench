export interface RuntimeTaskInput {
  taskId?: string;
  sessionId?: string;
  runtimeSessionId?: string;
  projectPath?: string;
  prompt: string;
  provider?: string;
  model?: string;
  systemPrompt?: string;
  thinkingLevel?: string;
  attachments?: RuntimeAttachment[];
}

export interface RuntimeAttachment {
  name: string;
  mimeType: "image/png" | "image/jpeg" | "image/webp";
  data: string;
}

export interface RuntimeTaskHandle {
  taskId: string;
  runtimeSessionId?: string;
}

export interface RuntimeDiagnostics {
  found: boolean;
  executablePath: string | null;
  version: string | null;
  rpcHandshake: boolean;
  currentProvider: string | null;
  currentModel: string | null;
  supportsVision: boolean | null;
  error: string | null;
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
  followUp(taskId: string, input: string): Promise<void>;
  cancelTask(taskId: string): Promise<void>;
  getTaskState(taskId: string): Promise<RuntimeTaskState>;
  dispose(): Promise<void>;
}

export const runtimeEventName = "agent-runtime-event";
