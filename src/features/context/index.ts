export type ContextObjectType =
  | "project"
  | "contentUnit"
  | "script"
  | "scene"
  | "shot"
  | "asset"
  | "keyframe"
  | "generationTask";

export interface ObjectRef {
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
