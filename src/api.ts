import { invoke } from "@tauri-apps/api/core";
import type { FeatureFlags } from "./features/featureFlags";
import type {
  AgentDispatch,
  AgentMessage,
  AgentSession,
  AgentTask,
  CreateSessionInput,
  ExpertDefinition,
  ResolveIntentInput,
  ResolvedIntent,
  SendAgentMessageInput,
} from "./features/agent";
import type {
  RuntimeTaskHandle,
  RuntimeTaskInput,
  RuntimeTaskState,
} from "./features/agent/runtime";
import type {
  BuildContextInput,
  ContextPackage,
  ContextSearchResult,
} from "./features/context";
import type {
  AICard,
  ApplyPatchInput,
  ApplyPatchResponse,
  CreateCardInput,
  PatchProposal,
  ProposePatchInput,
  ResolveCardInput,
} from "./features/permission";
import type {
  MutationRequest,
  MutationResponse,
  BatchMutationRequest,
  BatchMutationResponse,
  ProjectDescriptor,
  ProjectState,
  SaveGraphLayoutInput,
} from "./types";

export const api = {
  getFeatureFlags: () => invoke<FeatureFlags>("get_feature_flags"),
  setFeatureFlag: (key: keyof FeatureFlags, enabled: boolean) =>
    invoke<FeatureFlags>("set_feature_flag", { key, enabled }),
  agentListExperts: () => invoke<ExpertDefinition[]>("agent_list_experts"),
  agentResolveIntent: (input: ResolveIntentInput) =>
    invoke<ResolvedIntent>("agent_resolve_intent", { input }),
  agentCreateSession: (projectPath: string, input: CreateSessionInput) =>
    invoke<AgentSession>("agent_create_session", { projectPath, input }),
  agentSendMessage: (projectPath: string, input: SendAgentMessageInput) =>
    invoke<AgentDispatch>("agent_send_message", { projectPath, input }),
  agentGetTask: (projectPath: string, taskId: string) =>
    invoke<AgentTask>("agent_get_task", { projectPath, taskId }),
  agentListMessages: (projectPath: string, sessionId: string) =>
    invoke<AgentMessage[]>("agent_list_messages", { projectPath, sessionId }),
  agentStartReadonlyTask: (input: RuntimeTaskInput) =>
    invoke<RuntimeTaskHandle>("agent_runtime_start_readonly", { input }),
  agentSendRuntimeInput: (taskId: string, input: string) =>
    invoke<void>("agent_runtime_send_input", { taskId, input }),
  agentCancelTask: (taskId: string) =>
    invoke<void>("agent_cancel_task", { taskId }),
  agentGetTaskState: (taskId: string) =>
    invoke<RuntimeTaskState>("agent_get_task_state", { taskId }),
  contextBuild: (projectPath: string, input: BuildContextInput) =>
    invoke<ContextPackage>("context_build", { projectPath, input }),
  contextSearch: (projectPath: string, query: string, limit = 20) =>
    invoke<ContextSearchResult[]>("context_search", { projectPath, query, limit }),
  patchPropose: (projectPath: string, input: ProposePatchInput) =>
    invoke<PatchProposal>("patch_propose", { projectPath, input }),
  patchGet: (projectPath: string, proposalId: string) =>
    invoke<PatchProposal>("patch_get", { projectPath, proposalId }),
  patchApply: (projectPath: string, input: ApplyPatchInput) =>
    invoke<ApplyPatchResponse>("patch_apply", { projectPath, input }),
  patchReject: (projectPath: string, proposalId: string) =>
    invoke<PatchProposal>("patch_reject", { projectPath, proposalId }),
  cardCreate: (projectPath: string, input: CreateCardInput) =>
    invoke<AICard>("card_create", { projectPath, input }),
  cardGet: (projectPath: string, cardId: string) =>
    invoke<AICard>("card_get", { projectPath, cardId }),
  cardList: (projectPath: string, taskId: string) =>
    invoke<AICard[]>("card_list", { projectPath, taskId }),
  cardResolve: (projectPath: string, input: ResolveCardInput) =>
    invoke<AICard>("card_resolve", { projectPath, input }),
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
  mutateBatch: (projectPath: string, request: BatchMutationRequest) =>
    invoke<BatchMutationResponse>("apply_batch_mutation", { projectPath, request }),
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
  cleanupProjectMedia: (projectPath: string) =>
    invoke<number>("cleanup_project_media", { projectPath }),
  saveGraphLayout: (projectPath: string, input: SaveGraphLayoutInput) =>
    invoke<string>("graph_layout_save", { projectPath, input }),
  resetGraphLayout: (projectPath: string, scopeType: SaveGraphLayoutInput["scopeType"], scopeId: string, viewType: SaveGraphLayoutInput["viewType"]) =>
    invoke<void>("graph_layout_reset", { projectPath, scopeType, scopeId, viewType }),
};
