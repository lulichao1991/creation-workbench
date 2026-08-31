import { invoke } from "@tauri-apps/api/core";
import type {
  MutationRequest,
  MutationResponse,
  ProjectDescriptor,
  ProjectState,
} from "./types";

export const api = {
  getDefaultWorkspace: () =>
    invoke<{ defaultPath: string }>("get_default_workspace"),
  listProjects: (rootPath: string) =>
    invoke<ProjectDescriptor[]>("list_projects", { rootPath }),
  createProject: (rootPath: string, name: string, structureType: string) =>
    invoke<ProjectDescriptor>("create_project", {
      rootPath,
      name,
      structureType,
    }),
  openProject: (projectPath: string) =>
    invoke<ProjectDescriptor>("open_project", { projectPath }),
  copyProject: (projectPath: string, newName: string) =>
    invoke<ProjectDescriptor>("copy_project", { projectPath, newName }),
  deleteProject: (projectPath: string) =>
    invoke<void>("delete_project", { projectPath }),
  loadProjectState: (projectPath: string) =>
    invoke<ProjectState>("load_project_state", { projectPath }),
  mutate: (projectPath: string, request: MutationRequest) =>
    invoke<MutationResponse>("apply_mutation", { projectPath, request }),
  listHistory: (projectPath: string) =>
    invoke<Pick<ProjectState, "changeSets" | "changes" | "snapshots">>(
      "list_history",
      { projectPath },
    ),
  undoChangeSet: (projectPath: string, changeSetId: string) =>
    invoke<number>("undo_change_set", { projectPath, changeSetId }),
  createSnapshot: (projectPath: string, name: string, description: string) =>
    invoke<{ id: string; revision: number }>("create_snapshot", {
      projectPath,
      name,
      description,
    }),
  restoreSnapshot: (projectPath: string, snapshotId: string) =>
    invoke<number>("restore_snapshot", { projectPath, snapshotId }),
  importProjectFile: (
    projectPath: string,
    sourcePath: string,
    category: string,
  ) =>
    invoke<string>("import_project_file", {
      projectPath,
      sourcePath,
      category,
    }),
  readProjectMedia: (projectPath: string, relativePath: string) =>
    invoke<{ mimeType: string; data: string }>("read_project_media", {
      projectPath,
      relativePath,
    }),
};
