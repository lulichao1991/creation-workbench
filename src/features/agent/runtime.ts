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
  healthy: boolean;
  agentHostHealthy: boolean;
  sdkVersion: string | null;
  modelRuntimeHealthy: boolean;
  modelRuntimeError: string | null;
  providerCount: number;
  modelCount: number;
  providerAuth: Array<{ providerId: string; configured: boolean; source: string | null; label: string | null }>;
  sessionHealth: { active: number; busy: number };
  toolGatewayHealthy: boolean;
  error: string | null;
}

export interface AgentModel {
  id: string;
  name: string;
  supportsVision: boolean;
  reasoning: boolean;
  contextWindow: number;
  maxTokens: number;
}

export interface AgentModelProvider {
  id: string;
  name: string;
  authConfigured: boolean;
  authSource: string | null;
  authLabel: string | null;
  models: AgentModel[];
}

export interface AgentModelChoice {
  provider: string | null;
  model: string | null;
  thinkingLevel: string | null;
}

export interface AgentModelSettings {
  defaultModel: AgentModelChoice;
  professionalOverrides: Record<string, AgentModelChoice>;
}

export interface AgentModelConfiguration {
  catalog: { providers: AgentModelProvider[] };
  settings: AgentModelSettings;
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
