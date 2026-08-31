import type { ObjectRef, SelectionSnapshot, ContextObjectType } from "../context";
import type { WriteScope } from "../permission";
import type { AgentMode } from ".";

const objectTypes = new Set<ContextObjectType>([
  "project",
  "contentUnit",
  "script",
  "scene",
  "shot",
  "asset",
  "assetRequirement",
  "keyframe",
  "generationTask",
  "relation",
  "storyElement",
  "storyElementOccurrence",
  "changeSet",
]);

const photographyFields = [
  "shot_size",
  "camera_height",
  "camera_direction",
  "composition",
  "camera_movement",
];

const protectedShotFields = [
  "duration",
  "narrative_purpose",
  "new_information",
  "action",
  "dialogue",
  "end_state",
];

export interface PanelSelectionInput {
  projectId: string;
  revision: number;
  objectType: string | null;
  objectId: string | null;
  field: string | null;
  selectedIds: string[];
  currentUnitId: string | null;
}

export function buildAgentSelection(input: PanelSelectionInput): SelectionSnapshot {
  const objectType = input.objectType && objectTypes.has(input.objectType as ContextObjectType)
    ? input.objectType as ContextObjectType
    : input.currentUnitId ? "contentUnit" : "project";
  const objectId = input.objectId ?? input.currentUnitId ?? input.projectId;
  const center: ObjectRef = {
    projectId: input.projectId,
    objectType,
    objectId,
    ...(input.field ? { field: input.field } : {}),
  };
  const selected = input.selectedIds.length
    ? input.selectedIds.map((id) => ({ projectId: input.projectId, objectType, objectId: id }))
    : [center];
  return {
    projectId: input.projectId,
    center,
    selected,
    projectRevision: input.revision,
  };
}

export function buildWriteScope(selection: SelectionSnapshot, mode: AgentMode): WriteScope {
  if (mode !== "edit") return { refs: [], protectedRefs: [] };
  const selected = selection.selected.length ? selection.selected : selection.center ? [selection.center] : [];
  if (selected.length > 1 && selected.every((reference) => reference.objectType === "shot")) {
    return {
      refs: selected.flatMap((reference) => photographyFields.map((field) => ({ ...reference, field }))),
      protectedRefs: selected.flatMap((reference) => protectedShotFields.map((field) => ({ ...reference, field }))),
    };
  }
  return { refs: selected, protectedRefs: [] };
}

export function buildChangeAnalysisSelection(
  projectId: string,
  changeSetId: string,
  revision: number,
): SelectionSnapshot {
  return {
    projectId,
    center: { projectId, objectType: "changeSet", objectId: changeSetId },
    selected: [],
    projectRevision: revision,
  };
}

export function displayRef(reference: ObjectRef | null): string {
  if (!reference) return "尚未建立选区";
  return `${reference.objectType}:${reference.objectId}${reference.field ? `.${reference.field}` : ""}`;
}
