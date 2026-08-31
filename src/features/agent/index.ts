import type { ObjectRef, SelectionSnapshot } from "../context";
import type { WriteScope } from "../permission";

export * from "./runtime";

export const agentTaskStatuses = [
  "created",
  "context_building",
  "queued",
  "running",
  "waiting_for_user",
  "completed",
  "cancelled",
  "failed",
  "stale",
  "interrupted",
] as const;

export type AgentTaskStatus = (typeof agentTaskStatuses)[number];

export interface AgentTaskEnvelope {
  taskType: string;
  agentType: string;
  selection: SelectionSnapshot;
  readScope: ObjectRef[];
  writeScope: ObjectRef[];
}

export type ExpertType =
  | "writer"
  | "director"
  | "cinematography"
  | "art"
  | "keyframe"
  | "prompt";

export interface ExpertDefinition {
  expertType: ExpertType;
  displayName: string;
  responsibilities: string[];
  defaultRead: string[];
  defaultWrite: string[];
  prohibited: string[];
  systemInstruction: string;
}

export interface ResolveIntentInput {
  message: string;
  workspace?: string | null;
  selection: SelectionSnapshot;
}

export interface ResolvedIntent {
  taskType: string;
  expertType: ExpertType | null;
  confidence: number;
  reason: string;
  clarificationQuestion: string | null;
}

export interface CreateSessionInput {
  requestId: string;
  projectId: string;
  scopeType: string;
  scopeId?: string | null;
  title: string;
}

export interface AgentSession {
  id: string;
  projectId: string;
  scopeType: string;
  scopeId: string | null;
  title: string;
  status: string;
  createdAt: string;
  updatedAt: string;
}

export interface SendAgentMessageInput {
  requestId: string;
  sessionId: string;
  message: string;
  workspace?: string | null;
  selection: SelectionSnapshot;
  writeScope: WriteScope;
  tokenBudget?: number;
  provider?: string | null;
  model?: string | null;
}

export interface AgentTask {
  id: string;
  sessionId: string;
  taskType: string;
  agentType: ExpertType | "main";
  selection: SelectionSnapshot;
  readScope: ObjectRef[];
  writeScope: WriteScope;
  contextRevision: number;
  status: AgentTaskStatus;
  modelProvider: string | null;
  modelName: string | null;
  result: unknown | null;
  error: unknown | null;
  createdAt: string;
  startedAt: string | null;
  completedAt: string | null;
}

export interface AgentDispatch {
  sessionId: string;
  taskId: string;
  route: ResolvedIntent;
  runtimeStarted: boolean;
  status: AgentTaskStatus;
}
