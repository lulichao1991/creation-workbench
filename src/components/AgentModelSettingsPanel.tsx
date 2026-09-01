import { Check, KeyRound, LogOut, Settings2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { api } from "../api";
import type {
  AgentModelChoice,
  AgentModelConfiguration,
  AgentModelSettings,
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
}

export function AgentModelSettingsPanel({ disabled, onError }: Props) {
  const [configuration, setConfiguration] = useState<AgentModelConfiguration | null>(null);
  const [draft, setDraft] = useState<AgentModelSettings | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [working, setWorking] = useState(false);

  const load = async () => {
    const next = await api.agentModelSettingsGet();
    setConfiguration(next);
    setDraft(withDefaultModel(next));
  };

  useEffect(() => {
    void load().catch(onError);
  }, [onError]);

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

  if (!configuration || !draft) {
    return <details className="agent-model-settings"><summary><Settings2 size={12} />AI 模型设置</summary><p>正在读取 Pi ModelRuntime…</p></details>;
  }

  return (
    <details className="agent-model-settings">
      <summary><Settings2 size={12} />AI 模型设置</summary>
      {providers.length === 0 ? <p className="agent-runtime-error">Pi ModelRuntime 没有可用模型。</p> : (
        <div className="agent-model-settings-body">
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
