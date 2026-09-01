import { mkdir, readFile, rename, unlink, writeFile } from "node:fs/promises";
import path from "node:path";

import {
  getSupportedThinkingLevels,
  InMemoryCredentialStore,
  type AuthEvent,
  type AuthPrompt,
  type AuthType,
  type CredentialStore,
} from "@earendil-works/pi-ai";

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
  submittedResult?: Record<string, unknown>;
}

interface ImageInput {
  type: "image";
  data: string;
  mimeType: string;
}

interface AuthFlow {
  id: string;
  providerId: string;
  authType: AuthType;
  status: "running" | "completed" | "failed" | "cancelled";
  notifications: Array<AuthEvent & { id: string }>;
  cancelledPromptIds: string[];
  prompt?: AuthPrompt & { id: string };
  promptResolve?: (value: string) => void;
  promptReject?: (error: Error) => void;
  abort: AbortController;
  error?: string;
}

interface ModelsJsonConfig {
  providers: Record<string, Record<string, unknown>>;
}

const MAIN_SYSTEM_PROMPT = `你是创作工作台的主创作 Agent，定位接近制片人和创作总协调者。
你不拥有项目事实，也不能直接修改项目。项目事实只能通过工作台工具读取。
先根据用户问题调用必要的读取工具；信息不足时继续调用工具，不要猜测项目事实。
需要专业判断时自行调用 call_expert；不得用关键词假装已经咨询专家。专业 Agent 的结果只是意见，你必须结合项目事实综合回答。
如果核对事实后确认问题横跨至少两个专业方向、并行独立意见比单专家更合适，只能返回 expertTeamSuggestion（reason、question、members），由用户确认专家和高成本后启动；不得自行启动专家团。
所有修改只能在 submit_agent_result 中提出 patchProposal，由工作台决定是否应用。结束前必须调用一次 submit_agent_result；不要依赖自由文本 JSON。`;

const unavailableGateway: ToolGateway = async () => {
  throw new Error("工作台 Tool Gateway 不可用");
};

export class WorkbenchAgentHost {
  readonly sdkVersion = VERSION;
  private readonly sessions = new Map<string, SessionEntry>();
  private readonly authFlows = new Map<string, AuthFlow>();
  private authSequence = 0;

  private constructor(
    private readonly dataDir: string,
    private readonly sessionDir: string,
    private readonly modelsPath: string,
    private readonly modelRuntime: ModelRuntime,
    private readonly credentials: CredentialStore,
    private readonly emit: Emit,
    private readonly gateway: ToolGateway,
    private readonly toolGatewayHealthy: boolean,
  ) {}

  static async create(
    dataDir: string,
    emit: Emit,
    configureRuntime?: (runtime: ModelRuntime) => void,
    gateway: ToolGateway = unavailableGateway,
    credentials: CredentialStore = new InMemoryCredentialStore(),
  ): Promise<WorkbenchAgentHost> {
    await mkdir(dataDir, { recursive: true });
    const sessionDir = path.join(dataDir, "sessions");
    await mkdir(sessionDir, { recursive: true });
    const modelsPath = path.join(dataDir, "models.json");
    const modelRuntime = await ModelRuntime.create({
      credentials,
      modelsPath,
      modelsStorePath: path.join(dataDir, "models-store.json"),
      allowModelNetwork: false,
      refreshOnCreate: false,
    });
    configureRuntime?.(modelRuntime);
    await modelRuntime.refresh({ allowNetwork: false });
    return new WorkbenchAgentHost(dataDir, sessionDir, modelsPath, modelRuntime, credentials, emit, gateway, gateway !== unavailableGateway);
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
      case "auth_start":
        return this.startAuth(request);
      case "auth_flow_get":
        return this.getAuthFlow(request);
      case "auth_respond":
        return this.respondToAuth(request);
      case "auth_cancel":
        return this.cancelAuth(request);
      case "test_provider":
        return this.testProvider(request);
      case "logout_provider":
        return this.logoutProvider(request);
      case "save_custom_provider":
        return this.saveCustomProvider(request);
      case "delete_custom_provider":
        return this.deleteCustomProvider(request);
      case "refresh_models":
        return this.refreshModels(request);
      case "import_legacy_api_keys":
        return this.importLegacyApiKeys(request);
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
    for (const flow of this.authFlows.values()) flow.abort.abort(new Error("Agent Host 已关闭"));
    this.authFlows.clear();
    for (const entry of this.sessions.values()) {
      entry.unsubscribe();
      entry.session.dispose();
    }
    this.sessions.clear();
  }

  private async getModels(): Promise<Record<string, unknown>> {
    const customProviders = (await this.readModelsConfig()).providers;
    return {
      providers: this.modelRuntime.getProviders().map((provider) => {
        const auth = this.modelRuntime.getProviderAuthStatus(provider.id);
        const customConfig = customProviders[provider.id];
        return {
          id: provider.id,
          name: provider.name,
          authConfigured: auth.configured,
          authSource: auth.source,
          authLabel: auth.label,
          authMethods: [
            provider.auth.oauth && {
              type: "oauth",
              interactive: true,
              label: provider.auth.oauth.loginLabel ?? provider.auth.oauth.name,
              subscription: provider.auth.oauth.isSubscription === true,
            },
            provider.auth.apiKey && {
              type: "api_key",
              interactive: typeof provider.auth.apiKey.login === "function",
              label: provider.auth.apiKey.name,
              source: auth.source,
            },
          ].filter(Boolean),
          custom: customConfig !== undefined,
          customConfig: customConfig ? sanitizeCustomProviderConfig(customConfig) : undefined,
          models: this.modelRuntime.getModels(provider.id).map((model) => ({
            id: model.id,
            name: model.name,
            supportsVision: model.input.includes("image"),
            reasoning: model.reasoning,
            supportedThinkingLevels: getSupportedThinkingLevels(model),
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
    const apiKey = requiredString(request, "apiKey");
    await this.modelRuntime.login(providerId, "api_key", {
      prompt: async (prompt) => prompt.type === "secret" || prompt.type === "text" ? apiKey : (() => { throw new Error("此 Provider 需要交互式配置"); })(),
      notify: () => {},
    });
    return { providerId, authConfigured: true };
  }

  private startAuth(request: HostRequest): Record<string, unknown> {
    const providerId = requiredString(request, "providerId");
    const authType = requiredAuthType(request);
    const provider = this.modelRuntime.getProvider(providerId);
    if (!provider) throw new Error(`Provider 不存在：${providerId}`);
    const method = authType === "oauth" ? provider.auth.oauth : provider.auth.apiKey;
    if (!method) throw new Error(`${provider.name} 不支持此认证方式`);
    if (authType === "api_key" && !provider.auth.apiKey?.login) throw new Error(`${provider.name} 只支持读取环境凭据，不能在应用内配置`);
    const id = `auth-${Date.now()}-${++this.authSequence}`;
    const flow: AuthFlow = {
      id,
      providerId,
      authType,
      status: "running",
      notifications: [],
      cancelledPromptIds: [],
      abort: new AbortController(),
    };
    this.authFlows.set(id, flow);
    void this.runAuthFlow(flow);
    return { flowId: id };
  }

  private async runAuthFlow(flow: AuthFlow): Promise<void> {
    try {
      await this.modelRuntime.login(flow.providerId, flow.authType, {
        signal: flow.abort.signal,
        notify: (event) => {
          flow.notifications.push({ ...event, id: `${flow.id}-notify-${flow.notifications.length + 1}` });
        },
        prompt: (prompt) => this.waitForAuthPrompt(flow, prompt),
      });
      const refresh = await this.modelRuntime.refresh({
        allowNetwork: true,
        force: true,
        providers: [flow.providerId],
        signal: AbortSignal.timeout(20_000),
      });
      const refreshError = refresh.errors.get(flow.providerId);
      if (refreshError) {
        flow.notifications.push({
          id: `${flow.id}-notify-${flow.notifications.length + 1}`,
          type: "info",
          message: `账号已连接，但模型列表刷新失败：${refreshError.message}`,
        });
      }
      flow.status = "completed";
    } catch (error) {
      flow.status = flow.abort.signal.aborted ? "cancelled" : "failed";
      flow.error = error instanceof Error ? error.message : String(error);
    } finally {
      this.clearAuthPrompt(flow);
      this.trimAuthFlows();
    }
  }

  private waitForAuthPrompt(flow: AuthFlow, prompt: AuthPrompt): Promise<string> {
    if (flow.prompt) return Promise.reject(new Error("认证流程同时请求了多个输入"));
    if (flow.abort.signal.aborted || prompt.signal?.aborted) {
      return Promise.reject(abortError(prompt.signal?.reason ?? flow.abort.signal.reason));
    }
    const promptId = `${flow.id}-prompt-${Date.now()}`;
    flow.prompt = { ...prompt, id: promptId };
    return new Promise((resolve, reject) => {
      flow.promptResolve = resolve;
      flow.promptReject = reject;
      const cancel = () => {
        if (flow.prompt?.id !== promptId) return;
        flow.cancelledPromptIds.push(promptId);
        this.clearAuthPrompt(flow);
        reject(abortError(prompt.signal?.reason));
      };
      prompt.signal?.addEventListener("abort", cancel, { once: true });
      flow.abort.signal.addEventListener("abort", cancel, { once: true });
    });
  }

  private getAuthFlow(request: HostRequest): Record<string, unknown> {
    return authFlowSnapshot(this.requiredAuthFlow(requiredString(request, "flowId")));
  }

  private respondToAuth(request: HostRequest): Record<string, unknown> {
    const flow = this.requiredAuthFlow(requiredString(request, "flowId"));
    const promptId = requiredString(request, "promptId");
    if (flow.prompt?.id !== promptId || !flow.promptResolve) throw new Error("认证输入已失效");
    const value = requiredString(request, "value");
    const resolve = flow.promptResolve;
    this.clearAuthPrompt(flow);
    resolve(value);
    return { accepted: true };
  }

  private cancelAuth(request: HostRequest): Record<string, unknown> {
    const flow = this.requiredAuthFlow(requiredString(request, "flowId"));
    if (flow.status === "running") flow.abort.abort(new Error("用户取消了登录"));
    return authFlowSnapshot(flow);
  }

  private requiredAuthFlow(flowId: string): AuthFlow {
    const flow = this.authFlows.get(flowId);
    if (!flow) throw new Error("认证流程不存在或已结束");
    return flow;
  }

  private clearAuthPrompt(flow: AuthFlow): void {
    flow.prompt = undefined;
    flow.promptResolve = undefined;
    flow.promptReject = undefined;
  }

  private trimAuthFlows(): void {
    const terminal = [...this.authFlows.values()].filter((flow) => flow.status !== "running");
    for (const flow of terminal.slice(0, Math.max(0, terminal.length - 20))) this.authFlows.delete(flow.id);
  }

  private async testProvider(request: HostRequest): Promise<Record<string, unknown>> {
    const providerId = requiredString(request, "providerId");
    if (!this.modelRuntime.getProvider(providerId)) throw new Error(`Provider 不存在：${providerId}`);
    const auth = await this.modelRuntime.checkAuth(providerId, { signal: AbortSignal.timeout(15_000) });
    return auth
      ? { healthy: true, message: "Provider 连接和认证均正常" }
      : { healthy: false, message: "Provider 未通过认证，请检查 API Key" };
  }

  private async logoutProvider(request: HostRequest): Promise<Record<string, unknown>> {
    const providerId = requiredString(request, "providerId");
    if (!this.modelRuntime.getProvider(providerId)) throw new Error(`Provider 不存在：${providerId}`);
    await this.modelRuntime.logout(providerId);
    return { providerId, authConfigured: false };
  }

  private async saveCustomProvider(request: HostRequest): Promise<Record<string, unknown>> {
    const providerId = requiredString(request, "providerId");
    if (!/^custom-[a-z0-9][a-z0-9-]{1,62}$/.test(providerId)) throw new Error("自定义 AI 服务标识无效");
    const provider = requiredRecord(request, "provider");
    validateCustomProvider(provider);
    const previousProviderId = optionalString(request, "previousProviderId");
    const before = await this.readModelsConfig();
    if (!previousProviderId && before.providers[providerId]) throw new Error("已存在同名的自定义 AI 服务");
    const next = structuredClone(before);
    if (previousProviderId && previousProviderId !== providerId) delete next.providers[previousProviderId];
    next.providers[providerId] = structuredClone(provider);
    await this.writeModelsConfig(next);
    await this.modelRuntime.refresh({ allowNetwork: false });
    const error = this.modelRuntime.getError();
    if (error) {
      await this.writeModelsConfig(before);
      await this.modelRuntime.refresh({ allowNetwork: false });
      throw new Error(error);
    }
    return { providerId };
  }

  private async deleteCustomProvider(request: HostRequest): Promise<Record<string, unknown>> {
    const providerId = requiredString(request, "providerId");
    const config = await this.readModelsConfig();
    if (!(providerId in config.providers)) throw new Error("自定义 AI 服务不存在");
    await this.modelRuntime.logout(providerId).catch(() => {});
    delete config.providers[providerId];
    await this.writeModelsConfig(config);
    await this.modelRuntime.refresh({ allowNetwork: false });
    return { providerId, deleted: true };
  }

  private async refreshModels(request: HostRequest): Promise<Record<string, unknown>> {
    const providerId = optionalString(request, "providerId");
    if (providerId && !this.modelRuntime.getProvider(providerId)) throw new Error(`Provider 不存在：${providerId}`);
    const result = await this.modelRuntime.refresh({
      allowNetwork: true,
      force: true,
      providers: providerId ? [providerId] : undefined,
      signal: AbortSignal.timeout(20_000),
    });
    return {
      refreshed: true,
      errors: [...result.errors].map(([id, error]) => ({ providerId: id, message: error.message })),
    };
  }

  private async importLegacyApiKeys(request: HostRequest): Promise<Record<string, unknown>> {
    const keys = request.keys;
    if (!Array.isArray(keys)) throw new Error("旧凭据列表无效");
    const imported: string[] = [];
    for (const item of keys) {
      if (!item || typeof item !== "object" || Array.isArray(item)) throw new Error("旧凭据条目无效");
      const providerId = requiredRecordString(item as Record<string, unknown>, "providerId");
      const apiKey = requiredRecordString(item as Record<string, unknown>, "apiKey");
      await this.credentials.modify(providerId, async (current) => current ?? { type: "api_key", key: apiKey });
      imported.push(providerId);
    }
    if (imported.length) await this.modelRuntime.refresh({ allowNetwork: false, providers: imported });
    return { imported };
  }

  private async readModelsConfig(): Promise<ModelsJsonConfig> {
    try {
      const parsed: unknown = JSON.parse(await readFile(this.modelsPath, "utf8"));
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("models.json 根节点无效");
      const providers = (parsed as Record<string, unknown>).providers;
      if (!providers || typeof providers !== "object" || Array.isArray(providers)) throw new Error("models.json 缺少 providers");
      return { providers: providers as Record<string, Record<string, unknown>> };
    } catch (error) {
      if (error instanceof Error && "code" in error && (error as NodeJS.ErrnoException).code === "ENOENT") return { providers: {} };
      throw error;
    }
  }

  private async writeModelsConfig(config: ModelsJsonConfig): Promise<void> {
    await mkdir(path.dirname(this.modelsPath), { recursive: true });
    const temporaryPath = `${this.modelsPath}.${process.pid}.tmp`;
    try {
      await writeFile(temporaryPath, `${JSON.stringify(config, null, 2)}\n`, { encoding: "utf8", mode: 0o600 });
      await rename(temporaryPath, this.modelsPath);
    } finally {
      await unlink(temporaryPath).catch(() => {});
    }
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
    const resultToolKind = optionalResultToolKind(request) ?? "agent";
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
        resultToolKind,
        submitResult: (result) => { if (entry) entry.submittedResult = result; },
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
    let submittedResult: Record<string, unknown> | undefined;
    let unsubscribe = () => {};
    const abort = () => void session?.abort();
    input.signal?.addEventListener("abort", abort, { once: true });
    try {
      const model = selectModel(this.modelRuntime, launch.provider, launch.model) ?? parentEntry.session.model;
      if (!model) throw new Error(`${launch.expertType} Agent 没有可用模型`);
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
          {
            allowedToolNames: launch.allowedTools,
            parentTaskId,
            resultToolKind: "expert",
            submitResult: (result) => { submittedResult = result; },
          },
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
      const prompt = `专业任务：${input.task}\n焦点对象：${JSON.stringify(input.focusRefs)}\n先调用必要的工作台读取工具核对事实，结束前必须调用一次 submit_expert_result；不要依赖自由文本 JSON。`;
      await session.prompt(prompt, { images: model.input.includes("image") ? launch.images : undefined });
      const output = session.getLastAssistantText()?.trim();
      const structured = submittedResult ?? (output ? parseJson(output) : undefined);
      if (!structured) throw new Error(`${launch.expertType} Agent 没有返回结果`);
      const completed = gatewayData(await this.gateway({
        toolCallId: `${input.toolCallId}:complete`,
        sessionId: launch.expertSessionId,
        taskId: launch.expertTaskId,
        parentTaskId,
        toolName: "complete_expert",
        arguments: { runtimeSessionId: session.sessionId, result: JSON.stringify(structured) },
      }));
      return {
        expertType: launch.expertType,
        expertSessionId: launch.expertSessionId,
        result: completed && typeof completed === "object" && !Array.isArray(completed) && "result" in completed
          ? (completed as Record<string, unknown>).result
          : structured,
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
    entry.submittedResult = undefined;
    this.emit({ type: "event", event: "task_started", sessionId, taskId });
    const images = imageInputs(request.images);
    void entry.session.prompt(message, { images }).then(
      () => {
        if (entry.activeTaskId === taskId) {
          if (entry.submittedResult) this.emit({ type: "event", event: "structured_result", sessionId, taskId, result: entry.submittedResult });
          this.emit({ type: "event", event: "task_completed", sessionId, taskId });
        }
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
        entry.submittedResult = undefined;
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
    entry.submittedResult = undefined;
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
  images?: ImageInput[];
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
    images: imageInputs(record.images),
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

function requiredRecord(value: HostRequest, key: string): Record<string, unknown> {
  const field = value[key];
  if (!field || typeof field !== "object" || Array.isArray(field)) throw new Error(`${key} 必须是对象`);
  return field as Record<string, unknown>;
}

function requiredAuthType(value: HostRequest): AuthType {
  const type = requiredString(value, "authType");
  if (type !== "oauth" && type !== "api_key") throw new Error("认证方式无效");
  return type;
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

function optionalResultToolKind(value: HostRequest): "agent" | "expert" | "team" | undefined {
  const kind = optionalString(value, "resultToolKind");
  if (!kind) return undefined;
  if (!(["agent", "expert", "team"] as const).includes(kind as "agent" | "expert" | "team")) {
    throw new Error(`无效 resultToolKind：${kind}`);
  }
  return kind as "agent" | "expert" | "team";
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

function authFlowSnapshot(flow: AuthFlow): Record<string, unknown> {
  const prompt = flow.prompt
    ? Object.fromEntries(Object.entries(flow.prompt).filter(([key]) => key !== "signal"))
    : null;
  return {
    flowId: flow.id,
    providerId: flow.providerId,
    authType: flow.authType,
    status: flow.status,
    notifications: structuredClone(flow.notifications),
    prompt,
    cancelledPromptIds: [...flow.cancelledPromptIds],
    error: flow.error ?? null,
  };
}

function abortError(reason: unknown): Error {
  const error = reason instanceof Error ? reason : new Error("认证输入已取消");
  error.name = "AbortError";
  return error;
}

function sanitizeCustomProviderConfig(provider: Record<string, unknown>): Record<string, unknown> {
  const copy = structuredClone(provider);
  if (copy.apiKey !== "workbench-local") delete copy.apiKey;
  return copy;
}

function validateCustomProvider(provider: Record<string, unknown>): void {
  const name = provider.name;
  const baseUrl = provider.baseUrl;
  const api = provider.api;
  const models = provider.models;
  if (typeof name !== "string" || !name.trim()) throw new Error("自定义 AI 服务名称不能为空");
  if (typeof baseUrl !== "string") throw new Error("自定义 AI 服务地址不能为空");
  const url = new URL(baseUrl);
  const local = ["localhost", "127.0.0.1", "::1"].includes(url.hostname);
  if (url.protocol !== "https:" && !(url.protocol === "http:" && local)) throw new Error("远程 AI 服务必须使用 HTTPS；本机服务可使用 HTTP");
  const supportedApis = ["openai-completions", "openai-responses", "anthropic-messages", "google-generative-ai"];
  if (typeof api !== "string" || !supportedApis.includes(api)) throw new Error("自定义 AI 服务协议不受支持");
  if (provider.apiKey !== undefined && provider.apiKey !== "workbench-local") throw new Error("API Key 不能写入 models.json");
  if (!Array.isArray(models) || models.length === 0) throw new Error("至少添加一个模型");
  for (const model of models) {
    if (!model || typeof model !== "object" || Array.isArray(model)) throw new Error("模型配置无效");
    const id = (model as Record<string, unknown>).id;
    if (typeof id !== "string" || !id.trim()) throw new Error("模型 ID 不能为空");
  }
}
