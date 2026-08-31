export type ContextObjectType =
  | "project"
  | "contentUnit"
  | "script"
  | "scene"
  | "shot"
  | "asset"
  | "assetRequirement"
  | "keyframe"
  | "generationTask"
  | "relation"
  | "changeSet";

export interface ObjectRef {
  projectId: string;
  objectType: ContextObjectType;
  objectId: string;
  field?: string;
}

export interface SelectionSnapshot {
  projectId: string;
  center: ObjectRef | null;
  selected: ObjectRef[];
  projectRevision: number;
}

export interface BuildContextInput {
  taskId: string;
  selection: SelectionSnapshot;
  taskIntent: string;
  expertType: string;
  tokenBudget: number;
}

export interface ContextItem {
  reference: ObjectRef;
  source: "center" | "selection" | "affected" | "parent" | "neighbor" | "relation";
  data: unknown;
  tokenEstimate: number;
}

export interface ContextPackage {
  id: string;
  taskId: string;
  projectRevision: number;
  policyVersion: string;
  centerRef: ObjectRef;
  includedItems: ContextItem[];
  includedMemoryIds: string[];
  omittedSummary: string[];
  tokenEstimate: number;
  checksum: string;
  createdAt: string;
}

export interface ContextSearchResult {
  reference: ObjectRef;
  title: string;
  snippet: string;
  rank: number;
}
