import type { ObjectRef, SelectionSnapshot } from "../context";

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
