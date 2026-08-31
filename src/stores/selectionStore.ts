import { create } from "zustand";
import type { Workspace } from "../types";

export type SelectionMode = "edit" | "readonly" | "discussion" | "change-analysis";

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
  select: (selection: Partial<Omit<SelectionState, "select" | "clear">>) => void;
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
  mode: "edit" as SelectionMode,
};

export const useSelectionStore = create<SelectionState>((set) => ({
  ...initialState,
  select: (selection) => set(selection),
  clear: () => set(initialState),
}));
