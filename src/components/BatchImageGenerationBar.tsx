import { Images, Settings2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { api } from "../api";
import { generationCostNotice, type ImageOptions, type ImageTargetType, type ProviderConfig } from "../services/imageGeneration";
import { useAppDialog } from "./AppDialog";

export interface BatchImageTarget {
  targetType: ImageTargetType;
  targetId: string;
  label: string;
  prompt: string;
  referenceImages: string[];
}

export function BatchImageGenerationBar({
  projectPath,
  targets,
  onConfigure,
  onError,
}: {
  projectPath: string;
  targets: BatchImageTarget[];
  onConfigure: () => void;
  onError: (error: unknown) => void;
}) {
  const dialog = useAppDialog();
  const [enabled, setEnabled] = useState(false);
  const [providers, setProviders] = useState<ProviderConfig[]>([]);
  const [providerId, setProviderId] = useState("");
  const [size, setSize] = useState<ImageOptions["size"]>("1024x1024");
  const [working, setWorking] = useState(false);
  const eligible = useMemo(() => targets.filter((target) => target.prompt.trim()).slice(0, 20), [targets]);
  const skipped = targets.length - eligible.length;

  const load = async () => {
    const flags = await api.getFeatureFlags();
    setEnabled(flags.image_generation);
    const next = flags.image_generation ? await api.providerList() : [];
    setProviders(next);
    setProviderId((current) => next.some((provider) => provider.id === current) ? current : next[0]?.id ?? "");
  };

  useEffect(() => {
    void load().catch(() => undefined);
    const onUpdate = () => { void load().catch(() => undefined); };
    window.addEventListener("workbench:image-providers-updated", onUpdate);
    return () => window.removeEventListener("workbench:image-providers-updated", onUpdate);
  }, []);

  const provider = providers.find((item) => item.id === providerId) ?? null;
  const generate = async () => {
    if (!provider || eligible.length === 0) return;
    const referenceCount = eligible.reduce((count, item) => count + item.referenceImages.length, 0);
    if (referenceCount > 0 && (!provider.allowImageUpload || !provider.capabilities.referenceImages)) {
      onError(new Error("批次中包含正式参考图，但当前图片服务未允许参考图上传。请更换服务或在设置中明确授权。"));
      return;
    }
    const confirmed = await dialog.confirm(
      `将为 ${eligible.length} 项需求各创建 1 个候选任务。${skipped > 0 ? `\n${skipped} 项因提示词为空或超过单批 20 项而跳过。` : ""}\n\n${generationCostNotice(provider, { size, quality: "auto", count: 1 })}`,
      { title: "确认批量生成？", confirmLabel: "创建批量任务" },
    );
    if (!confirmed) return;
    setWorking(true);
    try {
      const results = await Promise.allSettled(eligible.map((target) => api.imageGenerate(projectPath, {
        requestId: crypto.randomUUID(),
        targetType: target.targetType,
        targetId: target.targetId,
        providerId: provider.id,
        model: provider.defaultModel,
        prompt: target.prompt.trim(),
        referenceImages: target.referenceImages,
        options: { size, quality: "auto", count: 1, background: "auto" },
      })));
      window.dispatchEvent(new Event("workbench:image-jobs-updated"));
      const failures = results.filter((result) => result.status === "rejected");
      if (failures.length) throw new Error(`已创建 ${results.length - failures.length} 个任务，${failures.length} 个任务创建失败。可在生图任务中心查看已创建任务。`);
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  if (targets.length === 0) return null;
  return (
    <div className="batch-image-bar">
      <div><Images size={16} /><span><strong>批量生成候选</strong><small>{eligible.length} 项已有提示词{skipped > 0 ? ` · ${skipped} 项待补提示词/下批处理` : ""}</small></span></div>
      {!enabled || providers.length === 0 ? <button className="ghost" onClick={onConfigure}><Settings2 size={13} />配置图片服务</button> : <>
        <select aria-label="批量生图服务" value={providerId} disabled={working} onChange={(event) => setProviderId(event.target.value)}>{providers.map((item) => <option value={item.id} key={item.id}>{item.displayName} · {item.defaultModel}</option>)}</select>
        <select aria-label="批量图片尺寸" value={size} disabled={working} onChange={(event) => setSize(event.target.value as ImageOptions["size"])}><option value="1024x1024">方形</option><option value="1024x1536">竖版 2:3</option><option value="1536x1024">横版 3:2</option></select>
        <button className="secondary" disabled={working || eligible.length === 0} onClick={() => void generate()}>{working ? "正在创建…" : `生成 ${eligible.length} 项`}</button>
      </>}
    </div>
  );
}
