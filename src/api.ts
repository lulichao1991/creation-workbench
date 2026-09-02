import { invoke } from "@tauri-apps/api/core";
import type { FeatureFlags } from "./features/featureFlags";
import type {
  AgentDispatch,
  AgentMessage,
  AgentSession,
  AgentTask,
  CreateSessionInput,
  ExpertDefinition,
  ExpertTeamConsultation,
  RequestExpertTeamInput,
  ResolveIntentInput,
  ResolvedIntent,
  SendAgentMessageInput,
} from "./features/agent";
import type {
  RuntimeTaskHandle,
  RuntimeTaskInput,
  RuntimeTaskState,
  RuntimeDiagnostics,
  ProviderConnectionTest,
  AgentModelConfiguration,
  AgentModelSettings,
  AgentAuthFlow,
  SaveAgentCustomProviderInput,
} from "./features/agent/runtime";
import type {
  BuildContextInput,
  ContextPackage,
  ContextSearchResult,
} from "./features/context";
import type { CreateMemoryInput, MemoryRecord, UpdateMemoryInput } from "./features/memory";
import type {
  GenerateImageInput,
  ImageJob,
  ImageResult,
  ImageSelectionState,
  ProviderConfig,
  ProviderTestResult,
  SaveProviderInput,
  SelectImageResult,
} from "./services/imageGeneration";
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
  ModelProfile,
  PromptCompilation,
  PromptTemplate,
  SaveModelProfileInput,
  SavePromptTemplateInput,
} from "./services/promptCompiler";
import type {
  MutationRequest,
  MutationResponse,
  BatchMutationRequest,
  BatchMutationResponse,
  ProjectDescriptor,
  ProjectState,
  SnapshotDetail,
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
  agentListSessions: (projectPath: string) =>
    invoke<AgentSession[]>("agent_list_sessions", { projectPath }),
  agentCloseSession: (projectPath: string, sessionId: string) =>
    invoke<AgentSession>("agent_close_session", { projectPath, sessionId }),
  agentResumeSession: (projectPath: string, sessionId: string) =>
    invoke<AgentSession>("agent_resume_session", { projectPath, sessionId }),
  agentSendMessage: (projectPath: string, input: SendAgentMessageInput) =>
    invoke<AgentDispatch>("agent_send_message", { projectPath, input }),
  agentGetTask: (projectPath: string, taskId: string) =>
    invoke<AgentTask>("agent_get_task", { projectPath, taskId }),
  agentListMessages: (projectPath: string, sessionId: string) =>
    invoke<AgentMessage[]>("agent_list_messages", { projectPath, sessionId }),
  expertTeamRequest: (projectPath: string, input: RequestExpertTeamInput) =>
    invoke<ExpertTeamConsultation>("expert_team_request", { projectPath, input }),
  expertTeamConfirm: (projectPath: string, consultationId: string) =>
    invoke<ExpertTeamConsultation>("expert_team_confirm", { projectPath, input: { consultationId, confirmed: true } }),
  expertTeamGet: (projectPath: string, consultationId: string) =>
    invoke<ExpertTeamConsultation>("expert_team_get", { projectPath, consultationId }),
  expertTeamList: (projectPath: string, sessionId: string) =>
    invoke<ExpertTeamConsultation[]>("expert_team_list", { projectPath, sessionId }),
  expertTeamCancel: (projectPath: string, consultationId: string) =>
    invoke<ExpertTeamConsultation>("expert_team_cancel", { projectPath, consultationId }),
  agentStartReadonlyTask: (input: RuntimeTaskInput) =>
    invoke<RuntimeTaskHandle>("agent_runtime_start_readonly", { input }),
  agentSendRuntimeInput: (taskId: string, input: string) =>
    invoke<void>("agent_runtime_send_input", { taskId, input }),
  agentFollowUpRuntimeInput: (taskId: string, input: string) =>
    invoke<void>("agent_runtime_follow_up", { taskId, input }),
  agentCancelTask: (taskId: string) =>
    invoke<void>("agent_cancel_task", { taskId }),
  agentGetTaskState: (taskId: string) =>
    invoke<RuntimeTaskState>("agent_get_task_state", { taskId }),
  agentRuntimeDoctor: () => invoke<RuntimeDiagnostics>("agent_runtime_doctor"),
  agentProviderTest: (providerId: string) => invoke<ProviderConnectionTest>("agent_provider_test", { providerId }),
  agentModelSettingsGet: () => invoke<AgentModelConfiguration>("agent_model_settings_get"),
  agentModelSettingsSave: (settings: AgentModelSettings) =>
    invoke<AgentModelSettings>("agent_model_settings_save", { settings }),
  agentProviderLogin: (providerId: string, apiKey: string) =>
    invoke<void>("agent_provider_login", { input: { providerId, apiKey } }),
  agentProviderLogout: (providerId: string) =>
    invoke<void>("agent_provider_logout", { providerId }),
  agentProviderAuthStart: (providerId: string, authType: "oauth" | "api_key") =>
    invoke<{ flowId: string }>("agent_provider_auth_start", { providerId, authType }),
  agentProviderAuthGet: (flowId: string) =>
    invoke<AgentAuthFlow>("agent_provider_auth_get", { flowId }),
  agentProviderAuthRespond: (flowId: string, promptId: string, value: string) =>
    invoke<void>("agent_provider_auth_respond", { input: { flowId, promptId, value } }),
  agentProviderAuthCancel: (flowId: string) =>
    invoke<AgentAuthFlow>("agent_provider_auth_cancel", { flowId }),
  agentCustomProviderSave: (input: SaveAgentCustomProviderInput) =>
    invoke<void>("agent_custom_provider_save", { input }),
  agentCustomProviderDelete: (providerId: string) =>
    invoke<void>("agent_custom_provider_delete", { providerId }),
  agentModelsRefresh: (providerId?: string) =>
    invoke<{ refreshed: boolean; errors: Array<{ providerId: string; message: string }> }>("agent_models_refresh", { providerId }),
  contextBuild: (projectPath: string, input: BuildContextInput) =>
    invoke<ContextPackage>("context_build", { projectPath, input }),
  contextSearch: (projectPath: string, query: string, limit = 20) =>
    invoke<ContextSearchResult[]>("context_search", { projectPath, query, limit }),
  memoryList: (projectPath: string, query?: string) =>
    invoke<MemoryRecord[]>("memory_list", { projectPath, query }),
  memoryCreate: (projectPath: string, input: CreateMemoryInput) =>
    invoke<MemoryRecord>("memory_create", { projectPath, input }),
  memoryUpdate: (projectPath: string, input: UpdateMemoryInput) =>
    invoke<MemoryRecord>("memory_update", { projectPath, input }),
  memoryInvalidate: (projectPath: string, storage: MemoryRecord["storage"], memoryId: string) =>
    invoke<MemoryRecord>("memory_invalidate", { projectPath, storage, memoryId }),
  providerList: () => invoke<ProviderConfig[]>("provider_list"),
  providerSave: (input: SaveProviderInput) => invoke<ProviderConfig>("provider_save", { input }),
  providerDelete: (providerId: string) => invoke<void>("provider_delete", { providerId }),
  providerTest: (providerId: string) => invoke<ProviderTestResult>("provider_test", { providerId }),
  imageGenerate: (projectPath: string, input: GenerateImageInput) =>
    invoke<ImageJob>("image_generate", { projectPath, input }),
  imagePreviewPrompt: (projectPath: string, targetType: GenerateImageInput["targetType"], targetId: string, prompt: string) =>
    invoke<string>("image_preview_prompt", { projectPath, targetType, targetId, prompt }),
  imageGetJob: (projectPath: string, jobId: string) =>
    invoke<ImageJob>("image_get_job", { projectPath, jobId }),
  imageListJobs: (projectPath: string, targetType: GenerateImageInput["targetType"], targetId: string) =>
    invoke<ImageJob[]>("image_list_jobs", { projectPath, targetType, targetId }),
  imageListRecentJobs: (projectPath: string, limit = 40) =>
    invoke<ImageJob[]>("image_list_recent_jobs", { projectPath, limit }),
  imageCancel: (projectPath: string, jobId: string) =>
    invoke<ImageJob>("image_cancel", { projectPath, jobId }),
  imageSelectResult: (projectPath: string, resultId: string) =>
    invoke<SelectImageResult>("image_select_result", { projectPath, resultId }),
  imageUpdateResultState: (projectPath: string, resultId: string, selectionState: Exclude<ImageSelectionState, "available" | "selected">) =>
    invoke<ImageResult>("image_update_result_state", { projectPath, resultId, selectionState }),
  promptListProfiles: () => invoke<ModelProfile[]>("prompt_list_profiles"),
  promptSaveProfile: (input: SaveModelProfileInput) => invoke<ModelProfile>("prompt_save_profile", { input }),
  promptListTemplates: (projectId?: string) => invoke<PromptTemplate[]>("prompt_list_templates", { projectId }),
  promptSaveTemplate: (input: SavePromptTemplateInput) => invoke<PromptTemplate>("prompt_save_template", { input }),
  promptCompile: (projectPath: string, input: { requestId: string; generationTaskId: string; modelProfileKey: string; templateId: string }) =>
    invoke<PromptCompilation>("prompt_compile", { projectPath, input }),
  promptListCompilations: (projectPath: string, generationTaskId: string) =>
    invoke<PromptCompilation[]>("prompt_list_compilations", { projectPath, generationTaskId }),
  promptSetCurrent: (projectPath: string, input: { compilationId: string; prompt: string; expectedRevision: number }) =>
    invoke<PromptCompilation>("prompt_set_current", { projectPath, input }),
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
  exportProductionPackage: (projectPath: string, generationTaskId?: string) =>
    invoke<{ directory: string; fileCount: number; warnings: string[] }>("export_production_package", { projectPath, generationTaskId }),
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
  getSnapshot: (projectPath: string, snapshotId: string) =>
    invoke<SnapshotDetail>("get_snapshot", { projectPath, snapshotId }),
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
