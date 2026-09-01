import { CheckCircle2, Image, Plus, RefreshCw, Trash2, XCircle } from "lucide-react";
import { useEffect, useState } from "react";

import { api } from "../api";
import type { ProviderConfig, SaveProviderInput } from "../services/imageGeneration";
import { useAppDialog } from "./AppDialog";

interface Props {
  disabled: boolean;
  onError: (error: unknown) => void;
}

const newProvider = (providerType: ProviderConfig["providerType"] = "openai_compatible"): SaveProviderInput => providerType === "mock" ? {
  requestId: crypto.randomUUID(),
  providerType,
  displayName: "Mock 图片服务（验收）",
  baseUrl: "http://127.0.0.1",
  textToImagePath: "/images/generations",
  imageEditPath: "/images/edits",
  defaultModel: "mock-image-1",
  timeoutSeconds: 30,
  maxConcurrency: 1,
  allowImageUpload: true,
} : {
  requestId: crypto.randomUUID(),
  providerType,
  displayName: "OpenAI Images",
  baseUrl: "https://api.openai.com/v1",
  textToImagePath: "/images/generations",
  imageEditPath: "/images/edits",
  defaultModel: "gpt-image-1",
  timeoutSeconds: 180,
  maxConcurrency: 1,
  allowImageUpload: false,
};

export function ImageProviderSettingsPanel({ disabled, onError }: Props) {
  const dialog = useAppDialog();
  const [enabled, setEnabled] = useState(false);
  const [providers, setProviders] = useState<ProviderConfig[]>([]);
  const [draft, setDraft] = useState<SaveProviderInput>(() => newProvider());
  const [working, setWorking] = useState(false);
  const [testResult, setTestResult] = useState<{ id: string; healthy: boolean; message: string } | null>(null);

  const load = async () => {
    const flags = await api.getFeatureFlags();
    setEnabled(flags.image_generation);
    setProviders(flags.image_generation ? await api.providerList() : []);
  };

  useEffect(() => { void load().catch(onError); }, []);

  const run = async (action: () => Promise<void>) => {
    setWorking(true);
    try {
      await action();
      await load();
      window.dispatchEvent(new Event("workbench:image-providers-updated"));
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const edit = (provider: ProviderConfig) => {
    setTestResult(null);
    setDraft({
      requestId: provider.id,
      providerType: provider.providerType,
      displayName: provider.displayName,
      baseUrl: provider.baseUrl,
      textToImagePath: provider.textToImagePath,
      imageEditPath: provider.imageEditPath,
      defaultModel: provider.defaultModel,
      timeoutSeconds: provider.timeoutSeconds,
      maxConcurrency: provider.maxConcurrency,
      allowImageUpload: provider.allowImageUpload,
    });
  };

  if (!enabled) return (
    <section className="settings-section image-provider-settings">
      <div><span className="label">图片生成服务</span><h3>按需启用，不影响手动创作</h3><p>启用后，角色、场景、道具、分镜和关键帧可以在工作台内直接生成图片。</p></div>
      <button className="primary" disabled={disabled || working} onClick={() => void run(async () => { await api.setFeatureFlag("image_generation", true); setDraft(newProvider()); })}><Image size={15} />启用图片生成</button>
    </section>
  );

  return (
    <section className="settings-section image-provider-settings">
      <div className="section-heading inline">
        <div><span className="label">图片生成服务</span><h3>{providers.length ? `${providers.length} 个服务配置` : "尚未配置服务"}</h3></div>
        <button className="ghost" disabled={disabled || working} onClick={() => { setDraft(newProvider()); setTestResult(null); }}><Plus size={14} />新建配置</button>
      </div>

      {providers.length > 0 && <div className="provider-list">{providers.map((provider) => (
        <article className={`provider-row ${provider.id === draft.requestId ? "selected" : ""}`} key={provider.id}>
          <button className="provider-select" onClick={() => edit(provider)}>
            <strong>{provider.displayName}</strong>
            <small>{provider.defaultModel} · {provider.status === "ready" ? "连接正常" : provider.status === "error" ? "连接异常" : provider.hasSecret || provider.providerType === "mock" ? "已配置" : "缺少密钥"}</small>
          </button>
          <button className="ghost" disabled={working} onClick={() => void run(async () => { const result = await api.providerTest(provider.id); setTestResult({ id: provider.id, ...result }); })}><RefreshCw size={13} />测试</button>
          <button className="danger-text" disabled={working} aria-label={`删除 ${provider.displayName}`} onClick={async () => { if (await dialog.confirm("删除后，使用该服务的历史任务仍会保留，但不能再用它生成或重试。", { title: `删除“${provider.displayName}”？`, danger: true, confirmLabel: "删除配置" })) void run(() => api.providerDelete(provider.id)); }}><Trash2 size={13} /></button>
          {testResult?.id === provider.id && <span className={`provider-test-result ${testResult.healthy ? "success" : "error"}`}>{testResult.healthy ? <CheckCircle2 size={13} /> : <XCircle size={13} />}{testResult.message}</span>}
        </article>
      ))}</div>}

      <div className="provider-form">
        <label>服务类型<select value={draft.providerType} disabled={working} onChange={(event) => setDraft(newProvider(event.target.value as ProviderConfig["providerType"]))}><option value="openai_compatible">OpenAI Compatible</option><option value="mock">Mock 验收服务</option></select></label>
        <label>配置名称<input value={draft.displayName} disabled={working} onChange={(event) => setDraft({ ...draft, displayName: event.target.value })} /></label>
        <label>Base URL<input value={draft.baseUrl} disabled={working} onChange={(event) => setDraft({ ...draft, baseUrl: event.target.value })} /></label>
        <label>文生图路径<input value={draft.textToImagePath ?? ""} disabled={working} placeholder="/images/generations" onChange={(event) => setDraft({ ...draft, textToImagePath: event.target.value })} /></label>
        <label>图生图路径<input value={draft.imageEditPath ?? ""} disabled={working} placeholder="/images/edits" onChange={(event) => setDraft({ ...draft, imageEditPath: event.target.value })} /></label>
        <label>默认图片模型<input value={draft.defaultModel} disabled={working} onChange={(event) => setDraft({ ...draft, defaultModel: event.target.value })} /></label>
        {draft.providerType !== "mock" && <label>API Key<input type="password" autoComplete="off" value={draft.apiKey ?? ""} placeholder={providers.some((provider) => provider.id === draft.requestId && provider.hasSecret) ? "已安全保存；留空保持不变" : "保存在 Windows 系统密钥库"} disabled={working} onChange={(event) => setDraft({ ...draft, apiKey: event.target.value })} /></label>}
        {draft.providerType !== "mock" && <label className="provider-upload-consent"><input type="checkbox" checked={draft.allowImageUpload ?? false} disabled={working} onChange={(event) => setDraft({ ...draft, allowImageUpload: event.target.checked })} />允许向此服务上传项目参考图</label>}
        <div className="provider-form-actions">
          <button className="primary" disabled={disabled || working || !draft.displayName.trim() || !draft.baseUrl.trim() || !draft.defaultModel.trim()} onClick={() => void run(async () => { const saved = await api.providerSave(draft); edit(saved); })}>{working ? "处理中…" : "保存配置"}</button>
        </div>
      </div>
    </section>
  );
}
