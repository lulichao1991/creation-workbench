import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import {
  defaultModelProfile,
  defaultPromptTemplate,
  promptForEditing,
  type ModelProfile,
  type PromptCompilation,
  type PromptTemplate,
} from "../services/promptCompiler";

interface Props {
  projectPath: string;
  projectId: string;
  taskId: string;
  revision: number;
  officialPrompt: string;
  onRefresh: () => Promise<void>;
  onError: (error: unknown) => void;
}

export function PromptCompilerPanel({ projectPath, projectId, taskId, revision, officialPrompt, onRefresh, onError }: Props) {
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);
  const [profiles, setProfiles] = useState<ModelProfile[]>([]);
  const [templates, setTemplates] = useState<PromptTemplate[]>([]);
  const [profileKey, setProfileKey] = useState("");
  const [templateId, setTemplateId] = useState("");
  const [history, setHistory] = useState<PromptCompilation[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [compare, setCompare] = useState(false);

  const refresh = useCallback(async () => {
    const [nextProfiles, nextTemplates, nextHistory] = await Promise.all([
      api.promptListProfiles(), api.promptListTemplates(projectId), api.promptListCompilations(projectPath, taskId),
    ]);
    setProfiles(nextProfiles);
    setTemplates(nextTemplates);
    setHistory(nextHistory);
    setProfileKey((current) => current && nextProfiles.some((item) => item.key === current) ? current : nextProfiles[0]?.key ?? "");
    setSelectedId((current) => current && nextHistory.some((item) => item.id === current) ? current : nextHistory[0]?.id ?? "");
  }, [projectId, projectPath, taskId]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    void api.getFeatureFlags().then(async (flags) => {
      if (!active) return;
      setEnabled(flags.prompt_compiler);
      if (flags.prompt_compiler) await refresh();
    }).catch(onError).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [refresh, onError]);

  const matchingTemplates = useMemo(() => templates.filter((item) => item.modelProfileKey === profileKey), [profileKey, templates]);
  useEffect(() => { setTemplateId((current) => matchingTemplates.some((item) => item.id === current) ? current : matchingTemplates[0]?.id ?? ""); }, [matchingTemplates]);
  const selected = history.find((item) => item.id === selectedId) ?? null;
  useEffect(() => { setDraft(selected ? promptForEditing(selected) : ""); setConfirming(false); }, [selectedId, selected?.updatedAt]);

  const enable = async () => {
    setBusy(true);
    try {
      await api.setFeatureFlag("prompt_compiler", true);
      await api.promptSaveProfile(defaultModelProfile);
      await api.promptSaveTemplate(defaultPromptTemplate);
      setEnabled(true);
      await refresh();
    } catch (error) { onError(error); } finally { setBusy(false); }
  };

  const compile = async () => {
    if (!profileKey || !templateId) return;
    setBusy(true);
    try {
      const result = await api.promptCompile(projectPath, { requestId: crypto.randomUUID(), generationTaskId: taskId, modelProfileKey: profileKey, templateId });
      await refresh();
      setSelectedId(result.id);
      setDraft(result.compiledPrompt);
    } catch (error) { onError(error); } finally { setBusy(false); }
  };

  const setCurrent = async () => {
    if (!selected || !draft.trim()) return;
    if (!confirming) { setConfirming(true); return; }
    setBusy(true);
    try {
      await api.promptSetCurrent(projectPath, { compilationId: selected.id, prompt: draft, expectedRevision: revision });
      setConfirming(false);
      await onRefresh();
      await refresh();
    } catch (error) { setConfirming(false); onError(error); } finally { setBusy(false); }
  };

  if (loading) return <section className="prompt-compiler-panel"><small>正在读取提示词编译器…</small></section>;
  if (!enabled) return <section className="prompt-compiler-panel prompt-compiler-disabled"><div><strong>提示词编译器</strong><small>把任务、镜头、正式资产和关键帧编译为目标模型提示词；不会调用视频生成。</small></div><button className="secondary" disabled={busy} onClick={() => void enable()}>{busy ? "正在启用…" : "启用并创建通用档案"}</button></section>;

  return <section className="prompt-compiler-panel">
    <div className="prompt-compiler-heading"><div><strong>提示词编译器</strong><small>仅编译与留痕，不调用视频模型。</small></div><button className="primary" disabled={busy || !profileKey || !templateId} title={busy ? "正在处理" : !profileKey || !templateId ? "请先选择模型档案和模板" : "编译候选提示词，不调用模型"} onClick={() => void compile()}>{busy ? "处理中…" : "编译新版本"}</button></div>
    <div className="prompt-compiler-controls">
      <label>模型档案<select value={profileKey} onChange={(event) => setProfileKey(event.target.value)}>{profiles.map((item) => <option key={item.key} value={item.key}>{item.displayName} · v{item.version}</option>)}</select></label>
      <label>模板<select value={templateId} onChange={(event) => setTemplateId(event.target.value)}>{matchingTemplates.map((item) => <option key={item.id} value={item.id}>{item.name} · v{item.version}</option>)}</select></label>
      <label>编译历史<select value={selectedId} onChange={(event) => setSelectedId(event.target.value)}><option value="">尚无编译记录</option>{history.map((item) => <option key={item.id} value={item.id}>{item.status === "current" ? "当前 · " : ""}修订 {item.sourceRevision} · {new Date(item.createdAt).toLocaleString()}</option>)}</select></label>
    </div>
    {selected && <>
      <div className="prompt-version-strip"><span>模型档案 v{selected.modelProfileVersion}</span><span>模板 v{selected.templateVersion}</span><span>来源修订 {selected.sourceRevision}</span><span>{selected.sourceMap.length} 个来源</span></div>
      {selected.warnings.length > 0 && <div className="prompt-warning-list">{selected.warnings.map((warning, index) => <div className={`prompt-warning ${warning.severity}`} key={`${warning.code}-${index}`}><strong>{warning.severity === "error" ? "需要处理" : "请注意"}</strong><span>{warning.message}</span></div>)}</div>}
      <details className="prompt-source-map" open={selected.referenceImages.length > 0}><summary>需上传的参考图（{selected.referenceImages.length}）</summary>{selected.referenceImages.length ? selected.referenceImages.map((reference) => <div key={`${reference.sourceType}:${reference.sourceId}`}><strong>{reference.label}</strong><span>{sourceTypeLabel(reference.sourceType)} · {fileName(reference.filePath)}</span></div>) : <small>本次编译没有正式参考图</small>}</details>
      <label className="prompt-output">候选提示词<textarea value={draft} onChange={(event) => { setDraft(event.target.value); setConfirming(false); }} /></label>
      <div className="prompt-compiler-actions"><button className="ghost" onClick={() => setCompare((value) => !value)}>{compare ? "收起对比" : "对比当前正式稿"}</button><button className={confirming ? "primary" : "secondary"} disabled={busy || !draft.trim()} title={busy ? "正在处理" : draft.trim() ? "需要再次确认后才会替换正式稿" : "候选提示词不能为空"} onClick={() => void setCurrent()}>{confirming ? "再次点击确认设为正式稿" : selected.status === "current" && draft === officialPrompt ? "已是当前正式稿" : "设为当前正式稿"}</button>{confirming && <button className="ghost" onClick={() => setConfirming(false)}>取消</button>}</div>
      {compare && <div className="prompt-compare"><div><strong>当前正式稿</strong><pre>{officialPrompt || "尚未设置"}</pre></div><div><strong>本次候选稿</strong><pre>{draft}</pre></div></div>}
      <details className="prompt-source-map"><summary>查看内容来源（{selected.sourceMap.length}）</summary>{selected.sourceMap.map((entry, index) => <div key={`${entry.sourceId}-${index}`}><strong>{entry.label}</strong><span>{sourceTypeLabel(entry.sourceType)}</span></div>)}</details>
    </>}
  </section>;
}

function sourceTypeLabel(type: string) {
  return ({ project: "项目", contentUnit: "内容单元", scene: "剧本场景", shot: "镜头", asset: "资产", assetRequirement: "资产需求", assetMedia: "资产图片", keyframe: "关键帧", generationTask: "制作批次", relation: "关系" } as Record<string, string>)[type] ?? "创作内容";
}

function fileName(path: string) {
  return path.split(/[\\/]/).pop() || "参考图";
}
