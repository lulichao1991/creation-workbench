import { Check, CheckCircle2, KeyRound, LogOut, RefreshCw, Settings2, XCircle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { api } from "../api";
import { toUserErrorMessage } from "../domain/userError";
import { withTimeout } from "../domain/async";
import type {
  AgentModelChoice,
  AgentModelConfiguration,
  AgentModelSettings,
  RuntimeDiagnostics,
} from "../features/agent/runtime";

const roles = [
  ["writer", "编剧"],
  ["director", "导演 / 分镜"],
  ["cinematography", "摄影"],
  ["art", "美术"],
  ["keyframe", "关键帧"],
  ["prompt", "提示词"],
] as const;

const thinkingLevels = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

interface Props {
  disabled: boolean;
  onError: (error: unknown) => void;
  expanded?: boolean;
}

export function AgentModelSettingsPanel({ disabled, onError, expanded = false }: Props) {
  const [configuration, setConfiguration] = useState<AgentModelConfiguration | null>(null);
  const [draft, setDraft] = useState<AgentModelSettings | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [working, setWorking] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [diagnostics, setDiagnostics] = useState<RuntimeDiagnostics | null>(null);
  const [diagnosing, setDiagnosing] = useState(false);
  const [connectionTest, setConnectionTest] = useState<{ healthy: boolean; message: string } | null>(null);
  const [testingConnection, setTestingConnection] = useState(false);

  const load = async () => {
    setLoadError(null);
    try {
      const next = await withTimeout(api.agentModelSettingsGet(), 10000, "读取模型配置超时，请重试。");
      setConfiguration(next);
      setDraft(withDefaultModel(next));
    } catch (error) {
      setLoadError(toUserErrorMessage(error));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const providers = configuration?.catalog.providers.filter((provider) => provider.models.length > 0) ?? [];
  const selectedProvider = providers.find((provider) => provider.id === draft?.defaultModel.provider) ?? null;
  const selectedModel = selectedProvider?.models.find((model) => model.id === draft?.defaultModel.model) ?? null;
  const modelOptions = useMemo(
    () => providers.flatMap((provider) => provider.models.map((model) => ({
      key: `${provider.id}/${model.id}`,
      label: `${provider.name} · ${model.name}`,
    }))),
    [providers],
  );

  const updateDefaultProvider = (providerId: string) => {
    if (!draft) return;
    const provider = providers.find((candidate) => candidate.id === providerId);
    setDraft({
      ...draft,
      defaultModel: { ...draft.defaultModel, provider: providerId, model: provider?.models[0]?.id ?? null },
    });
    setConnectionTest(null);
  };

  const setOverride = (role: string, value: string) => {
    if (!draft) return;
    const overrides = { ...draft.professionalOverrides };
    if (!value) {
      delete overrides[role];
    } else {
      const split = value.indexOf("/");
      overrides[role] = {
        provider: value.slice(0, split),
        model: value.slice(split + 1),
        thinkingLevel: overrides[role]?.thinkingLevel ?? draft.defaultModel.thinkingLevel,
      };
    }
    setDraft({ ...draft, professionalOverrides: overrides });
  };

  const run = async (action: () => Promise<void>) => {
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

  const diagnose = async () => {
    setDiagnosing(true);
    try {
      setDiagnostics(await withTimeout(api.agentRuntimeDoctor(), 15000, "Agent 运行环境检测超时，请重试。"));
      await load();
    } catch (error) {
      onError(error);
    } finally {
      setDiagnosing(false);
    }
  };

  const testConnection = async () => {
    if (!selectedProvider) return;
    setTestingConnection(true);
    setConnectionTest(null);
    try {
      setConnectionTest(await withTimeout(api.agentProviderTest(selectedProvider.id), 20000, "Provider 连接测试超时，请检查网络后重试。"));
      await load();
    } catch (error) {
      onError(error);
    } finally {
      setTestingConnection(false);
    }
  };

  if (!configuration || !draft) {
    return <details className="agent-model-settings" open={expanded || undefined}><summary><Settings2 size={12} />AI 模型设置</summary>{loadError ? <div className="agent-model-load-error"><p>{loadError}</p><button className="ghost" onClick={() => void load()}>重试</button></div> : <p>正在读取模型配置…</p>}</details>;
  }

  return (
    <details className="agent-model-settings" open={expanded || undefined}>
      <summary><Settings2 size={12} />AI 模型设置</summary>
      {providers.length === 0 ? <p className="agent-runtime-error">Pi ModelRuntime 没有可用模型。</p> : (
        <div className="agent-model-settings-body">
          <section className="agent-readiness">
            <div className={diagnostics?.agentHostHealthy ? "ready" : diagnostics ? "error" : "unknown"}>{diagnostics?.agentHostHealthy ? <CheckCircle2 size={14} /> : diagnostics ? <XCircle size={14} /> : <RefreshCw size={14} />}<span>运行环境</span><strong>{diagnostics ? diagnostics.agentHostHealthy ? "正常" : "异常" : "尚未检测"}</strong></div>
            <div className={selectedProvider?.authConfigured ? "ready" : "error"}>{selectedProvider?.authConfigured ? <CheckCircle2 size={14} /> : <XCircle size={14} />}<span>账号认证</span><strong>{selectedProvider?.authConfigured ? "已配置" : "未配置"}</strong></div>
            <div className={selectedModel ? "ready" : "error"}>{selectedModel ? <CheckCircle2 size={14} /> : <XCircle size={14} />}<span>默认模型</span><strong>{selectedModel?.name ?? "未选择"}</strong></div>
            <button className="secondary" disabled={disabled || working || diagnosing} onClick={() => void diagnose()}><RefreshCw size={12} />{diagnosing ? "检测中…" : "检测运行环境"}</button>
            {diagnostics?.error && <p className="agent-runtime-error">{diagnostics.error}</p>}
          </section>
          <div className="agent-model-grid">
            <label>Provider<select value={draft.defaultModel.provider ?? ""} disabled={disabled || working} onChange={(event) => updateDefaultProvider(event.target.value)}>{providers.map((provider) => <option value={provider.id} key={provider.id}>{provider.name}</option>)}</select></label>
            <label>主 Agent 模型<select value={draft.defaultModel.model ?? ""} disabled={disabled || working} onChange={(event) => setDraft({ ...draft, defaultModel: { ...draft.defaultModel, model: event.target.value } })}>{selectedProvider?.models.map((model) => <option value={model.id} key={model.id}>{model.name}</option>)}</select></label>
            <label>thinking level<select value={draft.defaultModel.thinkingLevel ?? "medium"} disabled={disabled || working} onChange={(event) => setDraft({ ...draft, defaultModel: { ...draft.defaultModel, thinkingLevel: event.target.value } })}>{thinkingLevels.map((level) => <option value={level} key={level}>{level}</option>)}</select></label>
          </div>
          <p className="agent-model-capabilities">
            {selectedModel?.supportsVision ? "支持视觉附件" : "仅文本"} · {selectedModel?.reasoning ? "支持推理" : "标准推理"} · 上下文 {selectedModel?.contextWindow.toLocaleString() ?? "—"}
          </p>
          <section className="agent-provider-auth">
            <div><strong>{selectedProvider?.authConfigured ? "Provider 已登录" : "Provider 未登录"}</strong><small>{selectedProvider?.authLabel ?? selectedProvider?.authSource ?? "API Key 由 Pi ModelRuntime 管理"}</small></div>
            <input type="password" autoComplete="off" value={apiKey} placeholder="API Key（保存到 Windows 系统密钥库）" disabled={disabled || working} onChange={(event) => setApiKey(event.target.value)} />
            <button className="secondary" disabled={disabled || working || !selectedProvider || !apiKey.trim()} onClick={() => void run(async () => { await api.agentProviderLogin(selectedProvider!.id, apiKey.trim()); setApiKey(""); })}><KeyRound size={11} />安全保存并登录</button>
            {selectedProvider?.authConfigured && <button className="ghost" disabled={disabled || working} onClick={() => void run(() => api.agentProviderLogout(selectedProvider.id))}><LogOut size={11} />注销</button>}
            <button className="ghost" disabled={disabled || working || testingConnection || !selectedProvider?.authConfigured} onClick={() => void testConnection()}><RefreshCw size={11} />{testingConnection ? "测试中…" : "测试账号连接"}</button>
            {connectionTest && <p className={`provider-test-result ${connectionTest.healthy ? "success" : "error"}`}>{connectionTest.healthy ? <CheckCircle2 size={13} /> : <XCircle size={13} />}{connectionTest.message}</p>}
            <p className="agent-oauth-note">当前桌面版不支持在应用内发起 OAuth 登录；请使用 API Key。若 Pi Runtime 已从外部提供 OAuth 凭据，只显示其认证状态，不会伪装成应用内 OAuth。</p>
          </section>
          <section className="agent-model-overrides">
            <strong>专业 Agent 模型覆盖</strong>
            {roles.map(([role, label]) => {
              const choice = draft.professionalOverrides[role];
              const value = choice?.provider && choice.model ? `${choice.provider}/${choice.model}` : "";
              return <div className="agent-model-override" key={role}><span>{label}</span><select value={value} disabled={disabled || working} onChange={(event) => setOverride(role, event.target.value)}><option value="">沿用主 Agent</option>{modelOptions.map((option) => <option value={option.key} key={option.key}>{option.label}</option>)}</select><select aria-label={`${label} thinking level`} value={choice?.thinkingLevel ?? draft.defaultModel.thinkingLevel ?? "medium"} disabled={disabled || working || !choice} onChange={(event) => setDraft({ ...draft, professionalOverrides: { ...draft.professionalOverrides, [role]: { ...choice, thinkingLevel: event.target.value } as AgentModelChoice } })}>{thinkingLevels.map((level) => <option value={level} key={level}>{level}</option>)}</select></div>;
            })}
          </section>
          <p className="agent-model-capabilities">模型或 thinking level 的修改从新建讨论开始生效；现有讨论继续使用创建时的配置。</p>
          <button className="primary agent-model-save" disabled={disabled || working} onClick={() => void run(async () => { await api.agentModelSettingsSave(draft); })}><Check size={11} />保存模型设置</button>
        </div>
      )}
    </details>
  );
}

function withDefaultModel(configuration: AgentModelConfiguration): AgentModelSettings {
  const settings = structuredClone(configuration.settings);
  if (settings.defaultModel.provider && settings.defaultModel.model) return settings;
  const provider = configuration.catalog.providers.find((candidate) => candidate.authConfigured && candidate.models.length > 0)
    ?? configuration.catalog.providers.find((candidate) => candidate.models.length > 0);
  settings.defaultModel.provider = provider?.id ?? null;
  settings.defaultModel.model = provider?.models[0]?.id ?? null;
  settings.defaultModel.thinkingLevel ??= "medium";
  return settings;
}
