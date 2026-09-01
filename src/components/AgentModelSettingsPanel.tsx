import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, CheckCircle2, ChevronDown, KeyRound, LogOut, Plus, RefreshCw, Trash2, XCircle } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { api } from "../api";
import { withTimeout } from "../domain/async";
import { toUserErrorMessage } from "../domain/userError";
import type {
  AgentAuthFlow,
  AgentAuthPrompt,
  AgentCustomProviderApi,
  AgentCustomProviderConfig,
  AgentModel,
  AgentModelChoice,
  AgentModelConfiguration,
  AgentModelProvider,
  AgentModelSettings,
} from "../features/agent/runtime";
import { useAppDialog } from "./AppDialog";

const roles = [
  ["writer", "编剧"],
  ["director", "导演 / 分镜"],
  ["cinematography", "摄影"],
  ["art", "美术"],
  ["keyframe", "关键帧"],
  ["prompt", "提示词"],
] as const;

const commonProviders = ["openai-codex", "openai", "anthropic", "google", "deepseek"];
const thinkingLabels: Record<string, string> = {
  off: "关闭", minimal: "最低", low: "低", medium: "中", high: "高", xhigh: "极高", max: "最大",
};

interface Props {
  disabled: boolean;
  onError: (error: unknown) => void;
}

interface CustomDraft {
  providerId: string | null;
  name: string;
  baseUrl: string;
  api: AgentCustomProviderApi;
  authMode: "none" | "api_key";
  modelId: string;
  modelName: string;
  reasoning: boolean;
  vision: boolean;
  headers: string;
}

const emptyCustomDraft = (): CustomDraft => ({
  providerId: null,
  name: "",
  baseUrl: "http://127.0.0.1:11434/v1",
  api: "openai-completions",
  authMode: "none",
  modelId: "",
  modelName: "",
  reasoning: false,
  vision: false,
  headers: "",
});

export function AgentModelSettingsPanel({ disabled, onError }: Props) {
  const dialog = useAppDialog();
  const [configuration, setConfiguration] = useState<AgentModelConfiguration | null>(null);
  const [draft, setDraft] = useState<AgentModelSettings | null>(null);
  const [working, setWorking] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [authFlow, setAuthFlow] = useState<AgentAuthFlow | null>(null);
  const [authPromptValue, setAuthPromptValue] = useState("");
  const [showCustomForm, setShowCustomForm] = useState(false);
  const [showAllProviders, setShowAllProviders] = useState(false);
  const [customDraft, setCustomDraft] = useState<CustomDraft>(() => emptyCustomDraft());
  const openedAuthUrls = useRef(new Set<string>());

  const load = async () => {
    setLoadError(null);
    try {
      const next = await withTimeout(api.agentModelSettingsGet(), 10000, "读取模型配置超时，请重试。");
      setConfiguration(next);
      setDraft(structuredClone(next.settings));
    } catch (error) {
      setLoadError(toUserErrorMessage(error));
    }
  };

  useEffect(() => { void load(); }, []);
  useEffect(() => { setAuthPromptValue(""); }, [authFlow?.prompt?.id]);

  useEffect(() => {
    if (!authFlow || authFlow.status !== "running") return;
    let active = true;
    const poll = async () => {
      try {
        const next = await api.agentProviderAuthGet(authFlow.flowId);
        if (!active) return;
        for (const notification of next.notifications) {
          if (notification.type === "auth_url" && !openedAuthUrls.current.has(notification.id)) {
            openedAuthUrls.current.add(notification.id);
            void openUrl(notification.url).catch(onError);
          }
        }
        setAuthFlow(next);
        if (next.status !== "running") await load();
      } catch (error) {
        if (active) onError(error);
      }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 350);
    return () => { active = false; window.clearInterval(timer); };
  }, [authFlow?.flowId, authFlow?.status]);

  const providers = configuration?.catalog.providers ?? [];
  const sortedProviders = useMemo(() => [...providers]
    .filter((provider) => `${providerDisplayName(provider)} ${provider.name}`.toLowerCase().includes(query.trim().toLowerCase()))
    .sort((a, b) => providerRank(a) - providerRank(b) || providerDisplayName(a).localeCompare(providerDisplayName(b), "zh-CN")), [providers, query]);
  const compactProviders = sortedProviders.filter((provider) => provider.authConfigured || provider.custom || commonProviders.includes(provider.id));
  const visibleProviders = query.trim() || showAllProviders ? sortedProviders : compactProviders;
  const selectedProvider = providers.find((provider) => provider.id === draft?.defaultModel.provider) ?? null;
  const selectedModel = selectedProvider?.models.find((model) => model.id === draft?.defaultModel.model) ?? null;
  const modelOptions = useMemo(() => providers.filter((provider) => provider.authConfigured).flatMap((provider) => provider.models.map((model) => ({
    key: `${provider.id}/${model.id}`,
    label: `${providerDisplayName(provider)} · ${model.name}`,
  }))), [providers]);

  const run = async (action: () => Promise<unknown>) => {
    setWorking(true);
    try {
      await action();
      await load();
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const startAuth = async (providerId: string, authType: "oauth" | "api_key") => {
    setWorking(true);
    try {
      const started = await api.agentProviderAuthStart(providerId, authType);
      setAuthFlow(await api.agentProviderAuthGet(started.flowId));
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const refreshModels = async (providerId?: string) => {
    await run(async () => {
      const result = await api.agentModelsRefresh(providerId);
      if (result.errors.length) throw new Error(result.errors.map((error) => error.message).join("；"));
    });
  };

  const updateDefaultProvider = (providerId: string) => {
    if (!draft) return;
    const provider = providers.find((candidate) => candidate.id === providerId);
    const model = provider?.models[0] ?? null;
    setDraft({ ...draft, defaultModel: { provider: providerId || null, model: model?.id ?? null, thinkingLevel: preferredThinkingLevel(model) } });
  };

  const updateDefaultModel = (modelId: string) => {
    if (!draft) return;
    const model = selectedProvider?.models.find((candidate) => candidate.id === modelId) ?? null;
    setDraft({ ...draft, defaultModel: { ...draft.defaultModel, model: modelId, thinkingLevel: preferredThinkingLevel(model) } });
  };

  const setOverride = (role: string, value: string) => {
    if (!draft) return;
    const overrides = { ...draft.professionalOverrides };
    if (!value) delete overrides[role];
    else {
      const split = value.indexOf("/");
      const providerId = value.slice(0, split);
      const modelId = value.slice(split + 1);
      const model = providers.find((provider) => provider.id === providerId)?.models.find((candidate) => candidate.id === modelId) ?? null;
      overrides[role] = { provider: providerId, model: modelId, thinkingLevel: preferredThinkingLevel(model) };
    }
    setDraft({ ...draft, professionalOverrides: overrides });
  };

  const editCustom = (provider: AgentModelProvider) => {
    const config = provider.customConfig;
    if (!config) return;
    const model = config.models[0];
    setCustomDraft({
      providerId: provider.id,
      name: config.name,
      baseUrl: config.baseUrl,
      api: config.api,
      authMode: config.apiKey === "workbench-local" && config.authHeader === false ? "none" : "api_key",
      modelId: model?.id ?? "",
      modelName: model?.name ?? "",
      reasoning: model?.reasoning ?? false,
      vision: model?.input?.includes("image") ?? false,
      headers: config.headers ? JSON.stringify(config.headers, null, 2) : "",
    });
    setShowCustomForm(true);
  };

  const saveCustom = async () => {
    const headers = parseHeaders(customDraft.headers);
    const providerId = customDraft.providerId ?? customProviderId(customDraft.name);
    const provider: AgentCustomProviderConfig = {
      name: customDraft.name.trim(),
      baseUrl: customDraft.baseUrl.trim(),
      api: customDraft.api,
      ...(customDraft.authMode === "none" ? { apiKey: "workbench-local", authHeader: false } : { authHeader: true }),
      ...(headers ? { headers } : {}),
      models: [{
        id: customDraft.modelId.trim(),
        ...(customDraft.modelName.trim() ? { name: customDraft.modelName.trim() } : {}),
        reasoning: customDraft.reasoning,
        input: customDraft.vision ? ["text", "image"] : ["text"],
      }],
    };
    await api.agentCustomProviderSave({ providerId, previousProviderId: customDraft.providerId ?? undefined, provider });
    setShowCustomForm(false);
    setCustomDraft(emptyCustomDraft());
  };

  if (!configuration || !draft) return <section className="ai-settings-loading">{loadError ? <><p>{loadError}</p><button className="ghost" onClick={() => void load()}>重试</button></> : <p>正在读取 AI 服务和模型…</p>}</section>;

  return (
    <div className="ai-settings">
      <section className="ai-settings-section">
        <div className="ai-settings-heading">
          <div><h3>连接 AI 服务</h3><p>选择服务；支持的登录方式会自动显示。</p></div>
          <div className="ai-settings-heading-actions">
            <button className="ghost" disabled={disabled || working} onClick={() => void refreshModels()}><RefreshCw size={14} />刷新模型列表</button>
            <button className="secondary" disabled={disabled || working} onClick={() => { setCustomDraft(emptyCustomDraft()); setShowCustomForm(true); }}><Plus size={14} />添加 AI 服务</button>
          </div>
        </div>
        <label className="ai-provider-search"><span className="sr-only">搜索 AI 服务</span><input value={query} placeholder="搜索 AI 服务" onChange={(event) => setQuery(event.target.value)} /></label>
        <div className="ai-provider-list">
          {visibleProviders.map((provider) => (
            <article className={`ai-provider-card ${provider.authConfigured ? "connected" : ""}`} key={provider.id}>
              <div className="ai-provider-main">
                <span className={`ai-provider-status ${provider.authConfigured ? "ready" : "idle"}`}>{provider.authConfigured ? <CheckCircle2 size={16} /> : <span />}</span>
                <div><strong>{providerDisplayName(provider)}</strong><small>{providerDescription(provider)}</small></div>
                <span className="ai-provider-state">{providerConnectionLabel(provider)}</span>
              </div>
              <div className="ai-provider-actions">
                {!provider.authConfigured && provider.authMethods.filter((method) => method.interactive).map((method) => <button className={method.type === "oauth" ? "primary" : "secondary"} disabled={disabled || working || authFlow?.status === "running"} key={method.type} onClick={() => void startAuth(provider.id, method.type)}>{method.type === "oauth" ? <Check size={14} /> : <KeyRound size={14} />}{authActionLabel(provider, method.type)}</button>)}
                {!provider.authConfigured && provider.authMethods.length > 0 && provider.authMethods.every((method) => !method.interactive) && <small className="ai-ambient-auth">此服务只读取系统或环境凭据</small>}
                {provider.authConfigured && !isNoAuthProvider(provider) && <button className="ghost" disabled={disabled || working} onClick={() => void run(() => api.agentProviderLogout(provider.id))}><LogOut size={14} />退出连接</button>}
                {provider.custom && <button className="ghost" disabled={working} onClick={() => editCustom(provider)}>编辑</button>}
                {provider.custom && <button className="danger-text" disabled={working} aria-label={`删除 ${providerDisplayName(provider)}`} onClick={async () => {
                  if (await dialog.confirm("删除后将不能继续使用这个 AI 服务，但已有讨论记录不会删除。", { title: `删除“${providerDisplayName(provider)}”？`, danger: true, confirmLabel: "删除服务" })) void run(() => api.agentCustomProviderDelete(provider.id));
                }}><Trash2 size={14} /></button>}
              </div>
              {authFlow?.providerId === provider.id && <AuthFlowPanel flow={authFlow} value={authPromptValue} onValueChange={setAuthPromptValue} onRespond={(value) => {
                if (!authFlow.prompt) return;
                void api.agentProviderAuthRespond(authFlow.flowId, authFlow.prompt.id, value).then(() => setAuthPromptValue("")).catch(onError);
              }} onCancel={() => void api.agentProviderAuthCancel(authFlow.flowId).then(setAuthFlow).catch(onError)} />}
            </article>
          ))}
          {sortedProviders.length === 0 && <p className="ai-provider-empty">没有匹配的 AI 服务。</p>}
          {!query.trim() && sortedProviders.length > compactProviders.length && <button className={`ai-provider-more ${showAllProviders ? "expanded" : ""}`} onClick={() => setShowAllProviders((current) => !current)}><ChevronDown size={15} />{showAllProviders ? "收起更多 AI 服务" : `查看更多 AI 服务（${sortedProviders.length - compactProviders.length}）`}</button>}
        </div>
        {showCustomForm && <CustomProviderForm draft={customDraft} disabled={working} onChange={setCustomDraft} onCancel={() => { setShowCustomForm(false); setCustomDraft(emptyCustomDraft()); }} onSave={() => void run(saveCustom)} />}
      </section>

      <section className="ai-settings-section">
        <div className="ai-settings-heading"><div><h3>默认模型</h3><p>新建讨论将使用这里保存的模型。</p></div></div>
        <div className="ai-default-model-grid">
          <label>AI 服务<select value={draft.defaultModel.provider ?? ""} disabled={disabled || working} onChange={(event) => updateDefaultProvider(event.target.value)}><option value="">请选择已连接的服务</option>{providers.filter((provider) => provider.authConfigured && provider.models.length > 0).map((provider) => <option value={provider.id} key={provider.id}>{providerDisplayName(provider)}</option>)}</select></label>
          <label>模型<select value={draft.defaultModel.model ?? ""} disabled={disabled || working || !selectedProvider} onChange={(event) => updateDefaultModel(event.target.value)}><option value="">请选择模型</option>{selectedProvider?.models.map((model) => <option value={model.id} key={model.id}>{model.name}</option>)}</select></label>
          {selectedModel && selectedModel.supportedThinkingLevels.length > 1 ? <label>推理强度<select value={draft.defaultModel.thinkingLevel ?? preferredThinkingLevel(selectedModel)} disabled={disabled || working} onChange={(event) => setDraft({ ...draft, defaultModel: { ...draft.defaultModel, thinkingLevel: event.target.value } })}>{selectedModel.supportedThinkingLevels.map((level) => <option value={level} key={level}>{thinkingLabels[level] ?? level}</option>)}</select></label> : <label>推理强度<span className="ai-readonly-value">不支持</span></label>}
        </div>
        {selectedModel && <p className="ai-model-meta">{selectedModel.supportsVision ? "支持图片" : "仅文本"} · 上下文 {selectedModel.contextWindow.toLocaleString()}</p>}
        <button className="primary ai-model-save" disabled={disabled || working || !selectedProvider?.authConfigured || !selectedModel} onClick={() => void run(() => api.agentModelSettingsSave(draft))}><Check size={14} />保存默认模型</button>
      </section>

      <section className="ai-settings-section ai-advanced-section">
        <details>
          <summary><span><strong>不同专业 Agent 使用不同模型</strong><small>可选；未设置时全部沿用默认模型。</small></span><ChevronDown size={17} /></summary>
          <div className="ai-agent-overrides">
            {roles.map(([role, label]) => {
              const choice = draft.professionalOverrides[role];
              const value = choice?.provider && choice.model ? `${choice.provider}/${choice.model}` : "";
              const model = modelForChoice(providers, choice);
              return <div className="ai-agent-override" key={role}><span>{label}</span><select value={value} disabled={disabled || working} onChange={(event) => setOverride(role, event.target.value)}><option value="">沿用默认模型</option>{modelOptions.map((option) => <option value={option.key} key={option.key}>{option.label}</option>)}</select>{model && model.supportedThinkingLevels.length > 1 ? <select aria-label={`${label}推理强度`} value={choice?.thinkingLevel ?? preferredThinkingLevel(model)} disabled={disabled || working} onChange={(event) => setDraft({ ...draft, professionalOverrides: { ...draft.professionalOverrides, [role]: { ...choice, thinkingLevel: event.target.value } as AgentModelChoice } })}>{model.supportedThinkingLevels.map((level) => <option value={level} key={level}>{thinkingLabels[level] ?? level}</option>)}</select> : <span className="ai-override-thinking">—</span>}</div>;
            })}
            <button className="secondary ai-model-save" disabled={disabled || working || !selectedModel} onClick={() => void run(() => api.agentModelSettingsSave(draft))}><Check size={14} />保存高级设置</button>
          </div>
        </details>
      </section>
    </div>
  );
}

function AuthFlowPanel({ flow, value, onValueChange, onRespond, onCancel }: { flow: AgentAuthFlow; value: string; onValueChange: (value: string) => void; onRespond: (value: string) => void; onCancel: () => void }) {
  return <div className={`ai-auth-flow ${flow.status}`}>
    {flow.notifications.map((notification) => {
      if (notification.type === "auth_url") return <p key={notification.id}>{notification.instructions ?? "请在浏览器中完成登录。"} <button className="link-button" onClick={() => void openUrl(notification.url)}>重新打开登录页面</button></p>;
      if (notification.type === "device_code") return <p key={notification.id}>打开 <button className="link-button" onClick={() => void openUrl(notification.verificationUri)}>{notification.verificationUri}</button>，输入设备码 <code>{notification.userCode}</code></p>;
      return <p key={notification.id}>{notification.message}</p>;
    })}
    {flow.prompt && <AuthPromptField prompt={flow.prompt} value={value} onChange={onValueChange} onSubmit={onRespond} />}
    {flow.status === "running" && <button className="ghost" onClick={onCancel}>取消登录</button>}
    {flow.status === "completed" && <p className="ai-auth-success"><CheckCircle2 size={15} />登录完成</p>}
    {flow.status === "failed" && <p className="ai-auth-error"><XCircle size={15} />{flow.error ?? "登录失败"}</p>}
    {flow.status === "cancelled" && <p>登录已取消。</p>}
  </div>;
}

function AuthPromptField({ prompt, value, onChange, onSubmit }: { prompt: AgentAuthPrompt; value: string; onChange: (value: string) => void; onSubmit: (value: string) => void }) {
  const selectedValue = value || (prompt.type === "select" ? prompt.options[0]?.id ?? "" : "");
  return <label className="ai-auth-prompt"><span>{prompt.message}</span>{prompt.type === "select" ? <select value={selectedValue} onChange={(event) => onChange(event.target.value)}>{prompt.options.map((option) => <option value={option.id} key={option.id}>{option.label}{option.description ? ` — ${option.description}` : ""}</option>)}</select> : <input type={prompt.type === "secret" ? "password" : "text"} autoComplete="off" value={value} placeholder={prompt.placeholder} onChange={(event) => onChange(event.target.value)} onKeyDown={(event) => event.key === "Enter" && value.trim() && onSubmit(value)} />}<button className="primary" disabled={!selectedValue.trim()} onClick={() => onSubmit(selectedValue)}>继续</button></label>;
}

function CustomProviderForm({ draft, disabled, onChange, onCancel, onSave }: { draft: CustomDraft; disabled: boolean; onChange: (draft: CustomDraft) => void; onCancel: () => void; onSave: () => void }) {
  const valid = draft.name.trim() && draft.baseUrl.trim() && draft.modelId.trim();
  return <div className="ai-custom-provider-form">
    <div className="ai-settings-heading"><div><h3>{draft.providerId ? "编辑 AI 服务" : "添加 AI 服务"}</h3><p>服务配置只用于当前工作台。</p></div></div>
    <div className="ai-custom-grid">
      <label>名称<input value={draft.name} disabled={disabled} placeholder="我的本地模型" onChange={(event) => onChange({ ...draft, name: event.target.value })} /></label>
      <label>接口地址<input value={draft.baseUrl} disabled={disabled} onChange={(event) => onChange({ ...draft, baseUrl: event.target.value })} /></label>
      <label>接口协议<select value={draft.api} disabled={disabled} onChange={(event) => onChange({ ...draft, api: event.target.value as AgentCustomProviderApi })}><option value="openai-completions">OpenAI Chat Completions</option><option value="openai-responses">OpenAI Responses</option><option value="anthropic-messages">Anthropic Messages</option><option value="google-generative-ai">Google Generative AI</option></select></label>
      <label>认证方式<select value={draft.authMode} disabled={disabled} onChange={(event) => onChange({ ...draft, authMode: event.target.value as CustomDraft["authMode"] })}><option value="none">无需认证</option><option value="api_key">API Key</option></select></label>
      <label>模型 ID<input value={draft.modelId} disabled={disabled} placeholder="qwen2.5-coder:7b" onChange={(event) => onChange({ ...draft, modelId: event.target.value })} /></label>
      <label>显示名称<input value={draft.modelName} disabled={disabled} placeholder="可选" onChange={(event) => onChange({ ...draft, modelName: event.target.value })} /></label>
      <label className="ai-custom-check"><input type="checkbox" checked={draft.reasoning} disabled={disabled} onChange={(event) => onChange({ ...draft, reasoning: event.target.checked })} />支持推理</label>
      <label className="ai-custom-check"><input type="checkbox" checked={draft.vision} disabled={disabled} onChange={(event) => onChange({ ...draft, vision: event.target.checked })} />支持图片</label>
      <label className="ai-custom-headers">自定义 Header（JSON，可选）<textarea rows={4} value={draft.headers} disabled={disabled} placeholder={'{"X-Custom-Header":"$ENV_VAR"}'} onChange={(event) => onChange({ ...draft, headers: event.target.value })} /></label>
    </div>
    <div className="ai-custom-actions"><button className="ghost" onClick={onCancel}>取消</button><button className="primary" disabled={disabled || !valid} onClick={onSave}>保存 AI 服务</button></div>
  </div>;
}

function providerDisplayName(provider: AgentModelProvider): string {
  if (provider.id === "openai-codex") return "ChatGPT Plus / Pro";
  if (provider.id === "openai") return "OpenAI API";
  return provider.name;
}

function providerDescription(provider: AgentModelProvider): string {
  if (provider.id === "openai-codex") return "使用 ChatGPT 订阅";
  if (provider.id === "openai") return "使用 API Key，按 API 用量计费";
  const subscription = provider.authMethods.find((method) => method.subscription);
  if (subscription) return subscription.label;
  return provider.custom ? "自定义 AI 服务" : `${provider.models.length} 个模型`;
}

function isNoAuthProvider(provider: AgentModelProvider): boolean {
  return provider.customConfig?.apiKey === "workbench-local" && provider.customConfig.authHeader === false;
}

function providerConnectionLabel(provider: AgentModelProvider): string {
  if (isNoAuthProvider(provider)) return "可用 · 无需认证";
  if (!provider.authConfigured) return "未连接";
  return `已连接${provider.authLabel ? ` · ${provider.authLabel}` : ""}`;
}

function providerRank(provider: AgentModelProvider): number {
  if (provider.authConfigured) return 0;
  if (provider.custom) return 1;
  const common = commonProviders.indexOf(provider.id);
  return common >= 0 ? 10 + common : 100;
}

function authActionLabel(provider: AgentModelProvider, type: "oauth" | "api_key"): string {
  if (provider.id === "openai-codex") return "登录 ChatGPT";
  return type === "oauth" ? "账号登录" : "配置 API Key";
}

function preferredThinkingLevel(model: AgentModel | null): string {
  if (!model) return "off";
  if (model.supportedThinkingLevels.includes("medium")) return "medium";
  return model.supportedThinkingLevels[0] ?? "off";
}

function modelForChoice(providers: AgentModelProvider[], choice: AgentModelChoice | undefined): AgentModel | null {
  if (!choice?.provider || !choice.model) return null;
  return providers.find((provider) => provider.id === choice.provider)?.models.find((model) => model.id === choice.model) ?? null;
}

function customProviderId(name: string): string {
  const slug = name.toLowerCase().normalize("NFKD").replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 48);
  return `custom-${slug || crypto.randomUUID().slice(0, 8)}`;
}

function parseHeaders(value: string): Record<string, string> | undefined {
  if (!value.trim()) return undefined;
  const parsed: unknown = JSON.parse(value);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed) || !Object.values(parsed).every((item) => typeof item === "string")) throw new Error("自定义 Header 必须是字符串键值的 JSON 对象");
  return parsed as Record<string, string>;
}
