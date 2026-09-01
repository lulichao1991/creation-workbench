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

type Emit = (event: HostEvent) => void;
type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";

interface SessionEntry {
  session: AgentSession;
  unsubscribe: () => void;
  activeTaskId?: string;
}

interface ImageInput {
  type: "image";
  data: string;
  mimeType: string;
}

const MAIN_SYSTEM_PROMPT = `你是创作工作台的主创作 Agent，定位接近制片人和创作总协调者。
你不拥有项目事实，也不能直接修改项目。项目事实只能通过工作台工具读取。
需要专业判断时调用相应专业 Agent。所有修改只能通过 propose_patch 提交，由工作台决定是否应用。`;

export class WorkbenchAgentHost {
  readonly sdkVersion = VERSION;
  private readonly sessions = new Map<string, SessionEntry>();

  private constructor(
    private readonly dataDir: string,
    private readonly modelRuntime: ModelRuntime,
    private readonly emit: Emit,
  ) {}

  static async create(
    dataDir: string,
    emit: Emit,
    configureRuntime?: (runtime: ModelRuntime) => void,
  ): Promise<WorkbenchAgentHost> {
    await mkdir(dataDir, { recursive: true });
    const modelRuntime = await ModelRuntime.create({
      authPath: path.join(dataDir, "auth.json"),
      modelsPath: null,
      allowModelNetwork: false,
      refreshOnCreate: false,
    });
    configureRuntime?.(modelRuntime);
    return new WorkbenchAgentHost(dataDir, modelRuntime, emit);
  }

  doctor(): Record<string, unknown> {
    return {
      healthy: true,
      sdkVersion: this.sdkVersion,
      providerCount: this.modelRuntime.getProviders().length,
      modelCount: this.modelRuntime.getModels().length,
      sessionCount: this.sessions.size,
    };
  }

  async handle(request: HostRequest): Promise<unknown> {
    switch (request.type) {
      case "doctor":
        return this.doctor();
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

  private async createSession(request: HostRequest): Promise<Record<string, unknown>> {
    const sessionId = requiredString(request, "sessionId");
    if (this.sessions.has(sessionId)) {
      const existing = this.sessions.get(sessionId)!;
      return { sessionId, runtimeSessionId: existing.session.sessionId, resumed: true };
    }

    const model = selectModel(this.modelRuntime, optionalString(request, "provider"), optionalString(request, "model"));
    const cwd = this.dataDir;
    const resourceLoader = isolatedResourceLoader(optionalString(request, "systemPrompt") ?? MAIN_SYSTEM_PROMPT);
    const settingsManager = SettingsManager.inMemory({
      compaction: { enabled: true },
      retry: { enabled: true, maxRetries: 2 },
    });
    const { session } = await createAgentSession({
      cwd,
      agentDir: this.dataDir,
      modelRuntime: this.modelRuntime,
      model,
      thinkingLevel: optionalThinkingLevel(request),
      noTools: "builtin",
      customTools: [],
      resourceLoader,
      settingsManager,
      sessionManager: SessionManager.inMemory(cwd, { id: sessionId }),
    });
    const entry: SessionEntry = { session, unsubscribe: () => {} };
    entry.unsubscribe = session.subscribe((event) => this.forwardSessionEvent(sessionId, entry, event));
    this.sessions.set(sessionId, entry);
    return { sessionId, runtimeSessionId: session.sessionId, resumed: false };
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
      if (entry.activeTaskId === taskId) entry.activeTaskId = undefined;
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
    return { cancelled: Boolean(taskId) };
  }

  private disposeSession(sessionId: string): Record<string, unknown> {
    const entry = this.requiredSession(sessionId);
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
