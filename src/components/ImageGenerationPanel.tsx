import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { loadProjectMediaDataUrl } from "../services/media";
import { useAppDialog } from "./AppDialog";
import {
  generationCostNotice,
  terminalImageStatuses,
  type ImageJob,
  type ImageOptions,
  type ImageResult,
  type ImageTargetType,
  type ProviderConfig,
} from "../services/imageGeneration";

interface Props {
  projectPath: string;
  targetType: ImageTargetType;
  targetId: string;
  prompt: string;
  referenceImages?: string[];
  onSelected: () => Promise<void>;
  onError: (error: unknown) => void;
  onConfigure?: () => void;
}

const statusLabels: Record<ImageJob["status"], string> = {
  created: "已创建",
  queued: "排队中",
  running: "生成中",
  completed: "已完成",
  partial: "部分成功",
  cancelled: "已取消",
  failed: "失败",
  interrupted: "已中断",
};

export function ImageGenerationPanel({ projectPath, targetType, targetId, prompt, referenceImages = [], onSelected, onError, onConfigure }: Props) {
  const dialog = useAppDialog();
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);
  const [providers, setProviders] = useState<ProviderConfig[]>([]);
  const [providerId, setProviderId] = useState("");
  const [jobs, setJobs] = useState<ImageJob[]>([]);
  const [options, setOptions] = useState<ImageOptions>({ size: "1024x1024", quality: "auto", count: 1, background: "auto" });
  const [confirmingGeneration, setConfirmingGeneration] = useState(false);
  const [confirmingSelectionId, setConfirmingSelectionId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const [nextProviders, nextJobs] = await Promise.all([
      api.providerList(),
      api.imageListJobs(projectPath, targetType, targetId),
    ]);
    setProviders(nextProviders);
    setProviderId((current) => current && nextProviders.some((item) => item.id === current) ? current : nextProviders[0]?.id ?? "");
    setJobs(nextJobs);
  }, [projectPath, targetId, targetType]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    void api.getFeatureFlags()
      .then(async (flags) => {
        if (!active) return;
        setEnabled(flags.image_generation);
        if (flags.image_generation) await refresh();
      })
      .catch(onError)
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [refresh]);

  useEffect(() => {
    const onProvidersUpdated = () => { void refresh().catch(onError); };
    window.addEventListener("workbench:image-providers-updated", onProvidersUpdated);
    return () => window.removeEventListener("workbench:image-providers-updated", onProvidersUpdated);
  }, [refresh, onError]);

  const hasActiveJob = jobs.some((job) => !terminalImageStatuses.has(job.status));
  useEffect(() => {
    if (!enabled || !hasActiveJob) return;
    const timer = window.setInterval(() => { void refresh().catch(onError); }, 900);
    return () => window.clearInterval(timer);
  }, [enabled, hasActiveJob, refresh]);

  const selectedProvider = providers.find((item) => item.id === providerId) ?? null;
  const candidates = useMemo(
    () => jobs.flatMap((job) => job.results.map((result) => ({ job, result }))).filter(({ result }) => result.selectionState !== "deleted"),
    [jobs],
  );

  const enable = async () => {
    try {
      await api.setFeatureFlag("image_generation", true);
      setEnabled(true);
      await refresh();
    } catch (error) { onError(error); }
  };

  const generate = async () => {
    if (!selectedProvider || !prompt.trim()) return;
    const canUseReferences = selectedProvider.allowImageUpload && selectedProvider.capabilities.referenceImages;
    if (referenceImages.length > 0 && !canUseReferences) {
      onError(new Error("当前 Provider 未启用参考图上传；请在 Provider 配置中明确允许后再生成。"));
      return;
    }
    if (!confirmingGeneration) {
      setConfirmingGeneration(true);
      return;
    }
    setConfirmingGeneration(false);
    try {
      await api.imageGenerate(projectPath, {
        requestId: crypto.randomUUID(),
        targetType,
        targetId,
        providerId: selectedProvider.id,
        model: selectedProvider.defaultModel,
        prompt: prompt.trim(),
        referenceImages,
        options,
      });
      window.dispatchEvent(new Event("workbench:image-jobs-updated"));
      await refresh();
    } catch (error) { onError(error); }
  };

  const select = async (result: ImageResult) => {
    if (confirmingSelectionId !== result.id) {
      setConfirmingSelectionId(result.id);
      return;
    }
    setConfirmingSelectionId(null);
    try {
      await api.imageSelectResult(projectPath, result.id);
      window.dispatchEvent(new Event("workbench:image-jobs-updated"));
      await Promise.all([refresh(), onSelected()]);
    } catch (error) { onError(error); }
  };

  const updateState = async (result: ImageResult, state: "rejected" | "archived" | "deleted") => {
    if (state === "deleted" && !await dialog.confirm("候选文件会从本地永久移除，此操作无法撤销。", { title: "永久删除候选图片？", confirmLabel: "永久删除", danger: true })) return;
    try {
      await api.imageUpdateResultState(projectPath, result.id, state);
      window.dispatchEvent(new Event("workbench:image-jobs-updated"));
      await refresh();
    } catch (error) { onError(error); }
  };

  const retry = async (job: ImageJob) => {
    try {
      await api.imageGenerate(projectPath, {
        requestId: crypto.randomUUID(),
        targetType,
        targetId,
        providerId: job.provider,
        model: job.model,
        prompt: job.prompt,
        referenceImages: job.referenceImages,
        options: job.options,
      });
      window.dispatchEvent(new Event("workbench:image-jobs-updated"));
      await refresh();
    } catch (error) { onError(error); }
  };

  const copyPrompt = async () => {
    try { await navigator.clipboard.writeText(prompt); } catch (error) { onError(error); }
  };

  if (loading) return <div className="image-generation-panel"><span className="label">静态生图加载中…</span></div>;
  if (!enabled) return (
    <div className="image-generation-panel image-generation-disabled">
      <div><span className="label">静态生图 · 默认关闭</span><strong>启用后仍只在你点击并确认时生成</strong></div>
      <button className="secondary" onClick={() => void enable()}>启用静态生图</button>
    </div>
  );

  return (
    <div className="image-generation-panel">
      <div className="image-generation-heading">
        <div><span className="label">STATIC IMAGE GENERATION</span><strong>候选区（非正式图片）</strong></div>
        <div><small>候选必须手动选择后才进入正式目录</small><button className="ghost" disabled={!prompt.trim()} onClick={() => void copyPrompt()}>复制提示词</button></div>
      </div>

      {providers.length === 0 && <div className="image-provider-empty"><div><strong>尚未配置图片生成服务</strong><small>你仍可导入图片或复制提示词；配置后可在当前位置直接生成。</small></div>{onConfigure && <button className="secondary" onClick={onConfigure}>前往全局设置</button>}</div>}

      {providers.length > 0 && <div className="generation-controls">
        <label>Provider<select value={providerId} onChange={(event) => setProviderId(event.target.value)}>{providers.map((provider) => <option value={provider.id} key={provider.id}>{provider.displayName} · {provider.defaultModel}</option>)}</select></label>
        <label>尺寸<select value={options.size} onChange={(event) => setOptions({ ...options, size: event.target.value as ImageOptions["size"] })}><option value="1024x1024">方形 1024</option><option value="1024x1536">竖版 2:3</option><option value="1536x1024">横版 3:2</option></select></label>
        <label>质量<select value={options.quality} onChange={(event) => setOptions({ ...options, quality: event.target.value })}><option value="auto">自动</option><option value="low">低</option><option value="medium">中</option><option value="high">高</option></select></label>
        <label>数量<select value={options.count} onChange={(event) => setOptions({ ...options, count: Number(event.target.value) })}>{[1, 2, 3, 4].map((count) => <option value={count} key={count}>{count} 张</option>)}</select></label>
        <div className="cost-notice">{selectedProvider && generationCostNotice(selectedProvider, options)}{referenceImages.length > 0 && <small>{selectedProvider?.capabilities.referenceImages ? ` · 将上传 ${referenceImages.length} 张正式参考图` : " · 当前 Provider 未启用参考图上传"}</small>}</div>
        <button className="primary" disabled={!selectedProvider || !prompt.trim() || hasActiveJob || (referenceImages.length > 0 && !selectedProvider?.capabilities.referenceImages)} onClick={() => void generate()}>{hasActiveJob ? "正在生成…" : confirmingGeneration ? "再次点击，确认生成" : "确认参数并生成候选"}</button>
        {confirmingGeneration && <button className="ghost generation-confirm-cancel" onClick={() => setConfirmingGeneration(false)}>取消</button>}
      </div>}

      {jobs.length > 0 && <div className="image-job-list">{jobs.slice(0, 4).map((job) => <div className={`image-job ${job.status}`} key={job.id}><span>{statusLabels[job.status]}</span><small>{job.model || providers.find((item) => item.id === job.provider)?.defaultModel}</small>{job.error?.message && <em>{job.error.message}</em>}{!terminalImageStatuses.has(job.status) && <button className="danger-text" onClick={() => void api.imageCancel(projectPath, job.id).then(async () => { window.dispatchEvent(new Event("workbench:image-jobs-updated")); await refresh(); }).catch(onError)}>取消</button>}{["failed", "interrupted", "cancelled"].includes(job.status) && <button className="ghost" disabled={hasActiveJob} onClick={() => void retry(job)}>重试</button>}</div>)}</div>}

      {candidates.length > 0 && <div className="candidate-grid">{candidates.map(({ result }) => <article className={`candidate-card ${result.selectionState}`} key={result.id}><CandidateImage projectPath={projectPath} result={result} /><div><strong>{result.selectionState === "selected" ? "已选为正式图片" : result.selectionState === "available" ? "候选图片" : result.selectionState === "rejected" ? "已拒绝" : "已归档"}</strong><small>候选文件 · 不会自动使用</small></div><div className="card-actions">{result.selectionState === "available" && <><button className="primary" onClick={() => void select(result)}>{confirmingSelectionId === result.id ? "再次点击，写入正式目录" : "选为正式"}</button><button className="ghost" onClick={() => void updateState(result, "rejected")}>拒绝</button></>}{result.selectionState === "rejected" && <button className="ghost" onClick={() => void updateState(result, "archived")}>归档</button>}{result.selectionState !== "selected" && <button className="danger-text" onClick={() => void updateState(result, "deleted")}>删除</button>}</div></article>)}</div>}
      {providers.length > 0 && jobs.length === 0 && <p className="candidate-empty">尚无候选。填写上方提示词后，由你确认参数并发起一次生成。</p>}
    </div>
  );
}

function CandidateImage({ projectPath, result }: { projectPath: string; result: ImageResult }) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let active = true;
    setFailed(false);
    void loadProjectMediaDataUrl(projectPath, result.filePath)
      .then((value) => { if (active) setSrc(value); })
      .catch(() => { if (active) { setSrc(null); setFailed(true); } });
    return () => { active = false; };
  }, [projectPath, result.filePath]);
  return src ? <img src={src} alt="静态生图候选" /> : <div className="image-placeholder">{failed ? "候选不可预览" : "正在加载候选…"}</div>;
}
