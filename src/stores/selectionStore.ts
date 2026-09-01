import { create } from "zustand";
import type { Workspace } from "../types";

export type SelectionMode = "edit" | "readonly" | "discussion" | "change-analysis";
export type SaveState = "saved" | "dirty" | "saving" | "error";

export interface SelectionState {
  projectId: string | null;
  contentUnitId: string | null;
  workspace: Workspace;
  objectType: string | null;
  objectId: string | null;
  field: string | null;
  selectedIds: string[];
  selectionScope: string | null;
  writeScope: string | null;
  mode: SelectionMode;
  saveState: SaveState;
  dirtyFields: string[];
  savingFields: string[];
  errorFields: string[];
  select: (selection: Partial<Omit<SelectionState, "select" | "clear" | "setSaveState" | "markFieldDirty" | "beginFieldSave" | "finishFieldSave">>) => void;
  setSaveState: (saveState: SaveState) => void;
  markFieldDirty: (fieldId: string) => void;
  beginFieldSave: (fieldId: string) => void;
  finishFieldSave: (fieldId: string, succeeded: boolean) => void;
  clear: () => void;
}

const initialState = {
  projectId: null,
  contentUnitId: null,
  workspace: "overview" as Workspace,
  objectType: null,
  objectId: null,
  field: null,
  selectedIds: [] as string[],
  selectionScope: null,
  writeScope: null,
  mode: "discussion" as SelectionMode,
  saveState: "saved" as SaveState,
  dirtyFields: [] as string[],
  savingFields: [] as string[],
  errorFields: [] as string[],
};

export const useSelectionStore = create<SelectionState>((set) => ({
  ...initialState,
  select: (selection) => set(selection),
  setSaveState: (saveState) => set({ saveState }),
  markFieldDirty: (fieldId) => set((state) => ({
    dirtyFields: state.dirtyFields.includes(fieldId) ? state.dirtyFields : [...state.dirtyFields, fieldId],
    errorFields: state.errorFields.filter((id) => id !== fieldId),
    saveState: "dirty",
  })),
  beginFieldSave: (fieldId) => set((state) => {
    const dirtyFields = state.dirtyFields.filter((id) => id !== fieldId);
    const savingFields = state.savingFields.includes(fieldId) ? state.savingFields : [...state.savingFields, fieldId];
    return { dirtyFields, savingFields, errorFields: state.errorFields.filter((id) => id !== fieldId), saveState: dirtyFields.length ? "dirty" : "saving" };
  }),
  finishFieldSave: (fieldId, succeeded) => set((state) => {
    const savingFields = state.savingFields.filter((id) => id !== fieldId);
    const dirtyFields = succeeded ? state.dirtyFields : state.dirtyFields.includes(fieldId) ? state.dirtyFields : [...state.dirtyFields, fieldId];
    const errorFields = succeeded ? state.errorFields.filter((id) => id !== fieldId) : state.errorFields.includes(fieldId) ? state.errorFields : [...state.errorFields, fieldId];
    const saveState: SaveState = errorFields.length ? "error" : dirtyFields.length ? "dirty" : savingFields.length ? "saving" : "saved";
    return { dirtyFields, savingFields, errorFields, saveState };
  }),
  clear: () => set(initialState),
}));
