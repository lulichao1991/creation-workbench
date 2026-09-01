import { mkdir } from "node:fs/promises";
import path from "node:path";

import {
  createAgentSession,
  createExtensionRuntime,
  ModelRuntime,
  type AgentSession,
  type AgentSessionEvent,
  type ResourceLoader,
  SessionManager,
  SettingsManager,
  VERSION,
} from "@earendil-works/pi-coding-agent";

import type { HostEvent, HostRequest } from "./protocol.js";
import { createWorkbenchTools, type CallExpertInput, type ToolGateway } from "./tools.js";

type Emit = (event: HostEvent) => void;
type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";

interface SessionEntry {
  session: AgentSession;
  unsubscribe: () => void;
  activeTaskId?: string;
  activeImages?: ImageInput[];
}

interface ImageInput {
  type: "image";
  data: string;
  mimeType: string;
}

const MAIN_SYSTEM_PROMPT = `你是创作工作台的主创作 Agent，定位接近制片人和创作总协调者。
你不拥有项目事实，也不能直接修改项目。项目事实只能通过工作台工具读取。
先根据用户问题调用必要的读取工具；信息不足时继续调用工具，不要猜测项目事实。
需要专业判断时自行调用 call_expert；不得用关键词假装已经咨询专家。专业 Agent 的结果只是意见，你必须结合项目事实综合回答。
如果核对事实后确认问题横跨至少两个专业方向、并行独立意见比单专家更合适，只能返回 expertTeamSuggestion（reason、question、members），由用户确认专家和高成本后启动；不得自行启动专家团。
所有修改只能在最终结构化结果中提出 patchProposal，由工作台决定是否应用。`;

const unavailableGateway: ToolGateway = async () => {
  throw new Error("工作台 Tool Gateway 不可用");
};

export class WorkbenchAgentHost {
  readonly sdkVersion = VERSION;
  private readonly sessions = new Map<string, SessionEntry>();

  private constructor(
    private readonly dataDir: string,
    private readonly sessionDir: string,
    private readonly modelRuntime: ModelRuntime,
    private readonly emit: Emit,
    private readonly gateway: ToolGateway,
    private readonly toolGatewayHealthy: boolean,
  ) {}

  static async create(
    dataDir: string,
    emit: Emit,
    configureRuntime?: (runtime: ModelRuntime) => void,
    gateway: ToolGateway = unavailableGateway,
  ): Promise<WorkbenchAgentHost> {
    await mkdir(dataDir, { recursive: true });
    const sessionDir = path.join(dataDir, "sessions");
    await mkdir(sessionDir, { recursive: true });
    const modelRuntime = await ModelRuntime.create({
      authPath: path.join(dataDir, "auth.json"),
      modelsPath: null,
      allowModelNetwork: false,
      refreshOnCreate: false,
    });
    configureRuntime?.(modelRuntime);
    return new WorkbenchAgentHost(dataDir, sessionDir, modelRuntime, emit, gateway, gateway !== unavailableGateway);
  }

  doctor(): Record<string, unknown> {
    const providers = this.modelRuntime.getProviders();
    const models = this.modelRuntime.getModels();
    return {
      healthy: true,
      agentHostHealthy: true,
      sdkVersion: this.sdkVersion,
      modelRuntimeHealthy: !this.modelRuntime.getError() && providers.length > 0 && models.length > 0,
      modelRuntimeError: this.modelRuntime.getError(),
      providerCount: providers.length,
      modelCount: models.length,
      providerAuth: providers.map((provider) => ({
        providerId: provider.id,
        ...this.modelRuntime.getProviderAuthStatus(provider.id),
      })),
      sessionHealth: {
        active: this.sessions.size,
        busy: [...this.sessions.values()].filter((entry) => entry.activeTaskId).length,
      },
      toolGatewayHealthy: this.toolGatewayHealthy,
    };
  }

  async handle(request: HostRequest): Promise<unknown> {
    switch (request.type) {
      case "doctor":
        return this.doctor();
      case "get_models":
        return this.getModels();
      case "login_provider":
        return this.loginProvider(request);
      case "logout_provider":
        return this.logoutProvider(request);
      case "create_session":
        return this.createSession(request);
      case "send_message":
        return this.sendMessage(request);
      case "steer":
        return this.steer(request);
      case "follow_up":
        return this.followUp(request);
      case "cancel":
        return this.cancel(request);
      case "dispose_session":
        return this.disposeSession(requiredString(request, "sessionId"));
      case "shutdown":
        this.dispose();
        return { stopped: true };
      default:
        throw new Error(`不支持的 Agent Host 请求：${request.type}`);
    }
  }

  dispose(): void {
    for (const entry of this.sessions.values()) {
      entry.unsubscribe();
      entry.session.dispose();
    }
    this.sessions.clear();
  }

  private getModels(): Record<string, unknown> {
    return {
      providers: this.modelRuntime.getProviders().map((provider) => {
        const auth = this.modelRuntime.getProviderAuthStatus(provider.id);
        return {
          id: provider.id,
          name: provider.name,
          authConfigured: auth.configured,
          authSource: auth.source,
          authLabel: auth.label,
          models: this.modelRuntime.getModels(provider.id).map((model) => ({
            id: model.id,
            name: model.name,
            supportsVision: model.input.includes("image"),
            reasoning: model.reasoning,
            contextWindow: model.contextWindow,
            maxTokens: model.maxTokens,
          })),
        };
      }),
    };
  }

  private async loginProvider(request: HostRequest): Promise<Record<string, unknown>> {
    const providerId = requiredString(request, "providerId");
    if (!this.modelRuntime.getProvider(providerId)) throw new Error(`Provider 不存在：${providerId}`);
    await this.modelRuntime.setRuntimeApiKey(providerId, requiredString(request, "apiKey"));
    return { providerId, authConfigured: true };
  }

  private async logoutProvider(request: HostRequest): Promise<Record<string, unknown>> {
    const providerId = requiredString(request, "providerId");
    if (!this.modelRuntime.getProvider(providerId)) throw new Error(`Provider 不存在：${providerId}`);
    await this.modelRuntime.logout(providerId);
    return { providerId, authConfigured: false };
  }

  private async createSession(request: HostRequest): Promise<Record<string, unknown>> {
    const sessionId = requiredString(request, "sessionId");
    if (this.sessions.has(sessionId)) {
      const existing = this.sessions.get(sessionId)!;
      return { sessionId, runtimeSessionId: existing.session.sessionId, resumed: true };
    }

    const runtimeSessionId = optionalString(request, "runtimeSessionId") ?? sessionId;
    const prior = (await SessionManager.list(this.dataDir, this.sessionDir))
      .find((candidate) => candidate.id === runtimeSessionId);
    const model = selectModel(this.modelRuntime, optionalString(request, "provider"), optionalString(request, "model"));
    const cwd = this.dataDir;
    const resourceLoader = isolatedResourceLoader(optionalString(request, "systemPrompt") ?? MAIN_SYSTEM_PROMPT);
    const allowedToolNames = optionalStringArray(request, "allowedTools");
    const allowCallExpert = optionalBoolean(request, "allowCallExpert") ?? true;
    const settingsManager = SettingsManager.inMemory({
      compaction: { enabled: true },
      retry: { enabled: true, maxRetries: 2 },
    });
    let entry: SessionEntry | undefined;
    const { session } = await createAgentSession({
      cwd,
      agentDir: this.dataDir,
      modelRuntime: this.modelRuntime,
      model,
      thinkingLevel: optionalThinkingLevel(request),
      noTools: "builtin",
      customTools: createWorkbenchTools(sessionId, () => entry?.activeTaskId, this.gateway, {
        allowedToolNames,
        callExpert: allowCallExpert ? (input) => this.callExpert(sessionId, entry, input) : undefined,
      }),
      resourceLoader,
      settingsManager,
      sessionManager: prior
        ? SessionManager.open(prior.path, this.sessionDir, cwd)
        : SessionManager.create(cwd, this.sessionDir, { id: runtimeSessionId }),
    });
    entry = { session, unsubscribe: () => {} };
    entry.unsubscribe = session.subscribe((event) => this.forwardSessionEvent(sessionId, entry, event));
    this.sessions.set(sessionId, entry);
    return { sessionId, runtimeSessionId: session.sessionId, resumed: Boolean(prior) };
  }

  private async callExpert(
    parentSessionId: string,
    parentEntry: SessionEntry | undefined,
    input: CallExpertInput,
  ): Promise<unknown> {
    const parentTaskId = parentEntry?.activeTaskId;
    if (!parentTaskId) throw new Error("当前没有可审计的主 Agent 任务");
    const launch = expertLaunch(gatewayData(await this.gateway({
      toolCallId: input.toolCallId,
      sessionId: parentSessionId,
      taskId: parentTaskId,
      toolName: "call_expert",
      arguments: {
        expertType: input.expertType,
        task: input.task,
        focusRefs: input.focusRefs,
      },
    }, input.signal)));
    let session: AgentSession | undefined;
    let unsubscribe = () => {};
    const abort = () => void session?.abort();
    input.signal?.addEventListener("abort", abort, { once: true });
    try {
      const model = selectModel(this.modelRuntime, launch.provider, launch.model) ?? parentEntry.session.model;
      const settingsManager = SettingsManager.inMemory({
        compaction: { enabled: true },
        retry: { enabled: true, maxRetries: 2 },
      });
      ({ session } = await createAgentSession({
        cwd: this.dataDir,
        agentDir: this.dataDir,
        modelRuntime: this.modelRuntime,
        model,
        thinkingLevel: launch.thinkingLevel,
        noTools: "builtin",
        customTools: createWorkbenchTools(
          launch.expertSessionId,
          () => launch.expertTaskId,
          this.gateway,
          { allowedToolNames: launch.allowedTools, parentTaskId },
        ),
        resourceLoader: isolatedResourceLoader(launch.systemPrompt),
        settingsManager,
        sessionManager: SessionManager.create(this.dataDir, this.sessionDir, { id: launch.runtimeSessionId }),
      }));
      unsubscribe = session.subscribe((event) => {
        if (event.type === "tool_execution_start") {
          this.emit({
            type: "event",
            event: "tool_call_requested",
            sessionId: parentSessionId,
            taskId: parentTaskId,
            expertType: launch.expertType,
            toolCallId: event.toolCallId,
            toolName: event.toolName,
            arguments: event.args,
          });
        } else if (event.type === "tool_execution_end") {
          this.emit({
            type: "event",
            event: "tool_call_completed",
            sessionId: parentSessionId,
            taskId: parentTaskId,
            expertType: launch.expertType,
            toolCallId: event.toolCallId,
            toolName: event.toolName,
            result: event.result,
            isError: event.isError,
          });
        }
      });
      const prompt = `专业任务：${input.task}\n焦点对象：${JSON.stringify(input.focusRefs)}\n先调用必要的工作台读取工具核对事实，再返回专业意见。最终只返回 JSON，键为 summary、findings、patchProposal、questions、risks。`;
      await session.prompt(prompt, { images: launch.allowImages ? parentEntry.activeImages : undefined });
      const output = session.getLastAssistantText()?.trim();
      if (!output) throw new Error(`${launch.expertType} Agent 没有返回结果`);
      const completed = gatewayData(await this.gateway({
        toolCallId: `${input.toolCallId}:complete`,
        sessionId: launch.expertSessionId,
        taskId: launch.expertTaskId,
        parentTaskId,
        toolName: "complete_expert",
        arguments: { runtimeSessionId: session.sessionId, result: output },
      }));
      return {
        expertType: launch.expertType,
        expertSessionId: launch.expertSessionId,
        result: completed && typeof completed === "object" && !Array.isArray(completed) && "result" in completed
          ? (completed as Record<string, unknown>).result
          : parseJson(output),
      };
    } catch (error) {
      await this.gateway({
        toolCallId: `${input.toolCallId}:failed`,
        sessionId: launch.expertSessionId,
        taskId: launch.expertTaskId,
        parentTaskId,
        toolName: "fail_expert",
        arguments: { error: error instanceof Error ? error.message : String(error) },
      }).catch(() => {});
      throw error;
    } finally {
      input.signal?.removeEventListener("abort", abort);
      unsubscribe();
      session?.dispose();
    }
  }

  private sendMessage(request: HostRequest): Record<string, unknown> {
    const sessionId = requiredString(request, "sessionId");
    const taskId = requiredString(request, "taskId");
    const message = requiredString(request, "message");
    const entry = this.requiredSession(sessionId);
    if (entry.activeTaskId) throw new Error(`会话正在执行任务：${entry.activeTaskId}`);
    entry.activeTaskId = taskId;
    this.emit({ type: "event", event: "task_started", sessionId, taskId });
    const images = imageInputs(request.images);
    entry.activeImages = images;
    void entry.session.prompt(message, { images }).then(
      () => {
        if (entry.activeTaskId === taskId) this.emit({ type: "event", event: "task_completed", sessionId, taskId });
      },
      (error: unknown) => {
        if (entry.activeTaskId === taskId) {
          this.emit({
            type: "event",
            event: "task_failed",
            sessionId,
            taskId,
            error: error instanceof Error ? error.message : String(error),
          });
        }
      },
    ).finally(() => {
      if (entry.activeTaskId === taskId) {
        entry.activeTaskId = undefined;
        entry.activeImages = undefined;
      }
    });
    return { accepted: true, sessionId, taskId };
  }

  private async steer(request: HostRequest): Promise<Record<string, unknown>> {
    const sessionId = requiredString(request, "sessionId");
    await this.requiredSession(sessionId).session.steer(requiredString(request, "message"), imageInputs(request.images));
    return { accepted: true };
  }

  private async followUp(request: HostRequest): Promise<Record<string, unknown>> {
    const sessionId = requiredString(request, "sessionId");
    await this.requiredSession(sessionId).session.followUp(requiredString(request, "message"), imageInputs(request.images));
    return { accepted: true };
  }

  private async cancel(request: HostRequest): Promise<Record<string, unknown>> {
    const sessionId = requiredString(request, "sessionId");
    const entry = this.requiredSession(sessionId);
    const taskId = entry.activeTaskId;
    await entry.session.abort();
    if (taskId) this.emit({ type: "event", event: "task_cancelled", sessionId, taskId });
    entry.activeTaskId = undefined;
    entry.activeImages = undefined;
    return { cancelled: Boolean(taskId) };
  }

  private disposeSession(sessionId: string): Record<string, unknown> {
    const entry = this.sessions.get(sessionId);
    if (!entry) return { disposed: false };
    entry.unsubscribe();
    entry.session.dispose();
    this.sessions.delete(sessionId);
    return { disposed: true };
  }

  private requiredSession(sessionId: string): SessionEntry {
    const entry = this.sessions.get(sessionId);
    if (!entry) throw new Error(`Agent Session 不存在：${sessionId}`);
    return entry;
  }

  private forwardSessionEvent(sessionId: string, entry: SessionEntry, event: AgentSessionEvent): void {
    const taskId = entry.activeTaskId;
    if (!taskId) return;
    if (event.type === "message_update" && event.assistantMessageEvent.type === "text_delta") {
      this.emit({ type: "event", event: "message_delta", sessionId, taskId, delta: event.assistantMessageEvent.delta });
    } else if (event.type === "tool_execution_start") {
      this.emit({ type: "event", event: "tool_call_requested", sessionId, taskId, toolCallId: event.toolCallId, toolName: event.toolName, arguments: event.args });
    } else if (event.type === "tool_execution_end") {
      this.emit({ type: "event", event: "tool_call_completed", sessionId, taskId, toolCallId: event.toolCallId, toolName: event.toolName, result: event.result, isError: event.isError });
    }
  }
}

interface ExpertLaunch {
  expertType: string;
  expertSessionId: string;
  expertTaskId: string;
  runtimeSessionId: string;
  systemPrompt: string;
  allowedTools: string[];
  provider?: string;
  model?: string;
  thinkingLevel?: ThinkingLevel;
  allowImages: boolean;
}

function expertLaunch(value: unknown): ExpertLaunch {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("工作台未返回专业 Agent 配置");
  const record = value as Record<string, unknown>;
  const allowedTools = record.allowedTools;
  return {
    expertType: requiredRecordString(record, "expertType"),
    expertSessionId: requiredRecordString(record, "expertSessionId"),
    expertTaskId: requiredRecordString(record, "expertTaskId"),
    runtimeSessionId: requiredRecordString(record, "runtimeSessionId"),
    systemPrompt: requiredRecordString(record, "systemPrompt"),
    allowedTools: Array.isArray(allowedTools) && allowedTools.every((item) => typeof item === "string")
      ? allowedTools
      : (() => { throw new Error("专业 Agent 工具白名单无效"); })(),
    provider: record.provider === null ? undefined : optionalRecordString(record, "provider"),
    model: record.model === null ? undefined : optionalRecordString(record, "model"),
    thinkingLevel: optionalRecordString(record, "thinkingLevel") as ThinkingLevel | undefined,
    allowImages: record.allowImages === true,
  };
}

function gatewayData(value: unknown): unknown {
  return value && typeof value === "object" && !Array.isArray(value) && "data" in value
    ? (value as Record<string, unknown>).data
    : value;
}

function requiredRecordString(value: Record<string, unknown>, key: string): string {
  const result = optionalRecordString(value, key);
  if (!result) throw new Error(`专业 Agent 配置缺少 ${key}`);
  return result;
}

function optionalRecordString(value: Record<string, unknown>, key: string): string | undefined {
  const result = value[key];
  if (result === undefined) return undefined;
  if (typeof result !== "string") throw new Error(`专业 Agent 配置 ${key} 必须是字符串`);
  return result;
}

function parseJson(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function isolatedResourceLoader(systemPrompt: string): ResourceLoader {
  return {
    getExtensions: () => ({ extensions: [], errors: [], runtime: createExtensionRuntime() }),
    getSkills: () => ({ skills: [], diagnostics: [] }),
    getPrompts: () => ({ prompts: [], diagnostics: [] }),
    getThemes: () => ({ themes: [], diagnostics: [] }),
    getAgentsFiles: () => ({ agentsFiles: [] }),
    getSystemPrompt: () => systemPrompt,
    getSystemPromptSource: () => undefined,
    getAppendSystemPrompt: () => [],
    getAppendSystemPromptSources: () => [],
    extendResources: () => {},
    reload: async () => {},
  };
}

function selectModel(runtime: ModelRuntime, provider?: string, model?: string) {
  if (provider && model) {
    const selected = runtime.getModel(provider, model);
    if (!selected) throw new Error(`模型不存在：${provider}/${model}`);
    return selected;
  }
  if (provider) return runtime.getModels(provider)[0];
  if (model) return runtime.getModels().find((candidate) => candidate.id === model);
  return undefined;
}

function requiredString(value: HostRequest, key: string): string {
  const field = value[key];
  if (typeof field !== "string" || !field.trim()) throw new Error(`${key} 不能为空`);
  return field;
}

function optionalString(value: HostRequest, key: string): string | undefined {
  const field = value[key];
  if (field === undefined || field === null || field === "") return undefined;
  if (typeof field !== "string") throw new Error(`${key} 必须是字符串`);
  return field;
}

function optionalStringArray(value: HostRequest, key: string): string[] | undefined {
  const field = value[key];
  if (field === undefined || field === null) return undefined;
  if (!Array.isArray(field) || !field.every((item) => typeof item === "string")) {
    throw new Error(`${key} 必须是字符串数组`);
  }
  return field;
}

function optionalBoolean(value: HostRequest, key: string): boolean | undefined {
  const field = value[key];
  if (field === undefined || field === null) return undefined;
  if (typeof field !== "boolean") throw new Error(`${key} 必须是布尔值`);
  return field;
}

function optionalThinkingLevel(value: HostRequest): ThinkingLevel | undefined {
  const level = optionalString(value, "thinkingLevel");
  if (!level) return undefined;
  const levels: ThinkingLevel[] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
  if (!levels.includes(level as ThinkingLevel)) throw new Error(`无效 thinkingLevel：${level}`);
  return level as ThinkingLevel;
}

function imageInputs(value: unknown): ImageInput[] | undefined {
  if (value === undefined) return undefined;
  if (!Array.isArray(value)) throw new Error("images 必须是数组");
  return value.map((item) => {
    if (!item || typeof item !== "object") throw new Error("图片参数无效");
    const image = item as Record<string, unknown>;
    if (typeof image.data !== "string" || typeof image.mimeType !== "string") throw new Error("图片缺少 data 或 mimeType");
    return { type: "image", data: image.data, mimeType: image.mimeType };
  });
}
