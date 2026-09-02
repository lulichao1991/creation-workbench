import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { buildAgentSelection } from "../features/agent/panelState";
import type { AgentTask } from "../features/agent";
import { buildScriptRequest, emptyStudioDraft, importedScript, normalizeCreativeStyle, normalizeCreationType, parseScriptResult, scriptMutations, type CreationType, type CreativeStyle, type ScriptDraftResult, type StudioDraft } from "../features/scriptStudio";
import { toUserErrorMessage } from "../domain/userError";
import { useSelectionStore } from "../stores/selectionStore";
import type { BatchMutationRequest, BatchMutationResponse, ContentUnitRow, ProjectDescriptor, ProjectState } from "../types";
import { ScriptStudio, StylePicker } from "./ScriptStudio";
import { creativeSettingsMutation, normalizeCreativeSettings, readCreativeSettings, studioStorageKey } from "../features/creativeSettings";

interface SavedStudio { draft: StudioDraft; result: ScriptDraftResult | null; taskId: string | null; expectedEpisodes: number }
const storageKey = studioStorageKey;
function loadStudio(key: string): SavedStudio {
  const empty = { draft: { ...emptyStudioDraft }, result: null, taskId: null, expectedEpisodes: 1 };
  try {
    const saved = JSON.parse(localStorage.getItem(key) ?? "null") as SavedStudio | null;
    if (!saved || !saved.draft || typeof saved.draft.text !== "string" || !["original", "import", "rewrite"].includes(saved.draft.mode) || ![1, 3, 5].includes(saved.draft.episodes)) return empty;
    saved.draft.style = normalizeCreativeStyle(saved.draft.style);
    saved.draft.contentType = normalizeCreationType(saved.draft.contentType);
    saved.draft.direction = typeof saved.draft.direction === "string" ? saved.draft.direction : "";
    saved.draft.fileName = typeof saved.draft.fileName === "string" ? saved.draft.fileName : "";
    saved.draft.scriptMode = saved.draft.scriptMode === "narration" ? "narration" : "drama";
    saved.taskId = typeof saved.taskId === "string" ? saved.taskId : null;
    saved.expectedEpisodes = [1, 3, 5].includes(saved.expectedEpisodes) ? saved.expectedEpisodes : saved.draft.episodes;
    if (saved.result) parseScriptResult({ findings: [saved.result] }, saved.result.episodes.length);
    return saved;
  } catch { return empty; }
}

interface Props {
  project: ProjectDescriptor;
  state: ProjectState;
  unit: ContentUnitRow;
  onMutateBatch: (request: BatchMutationRequest) => Promise<BatchMutationResponse>;
  onManual: () => Promise<void>;
  onTaskSettled: () => void;
}

export function ScriptCreation({ project, state, unit, onMutateBatch, onManual, onTaskSettled }: Props) {
  const key = storageKey(project.id, unit.id);
  const [saved, setSaved] = useState(() => loadStudio(key));
  const [starting, setStarting] = useState(false);
  const [accepting, setAccepting] = useState(false);
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [error, setError] = useState("");
  const lockRef = useRef(false);
  const select = useSelectionStore((selection) => selection.select);
  const busy = starting || Boolean(saved.taskId);
  const draft = busy || saved.result ? saved.draft : { ...saved.draft, ...readCreativeSettings(unit) };

  const changeSettings = async (style: CreativeStyle | null, contentType: CreationType) => {
    if (lockRef.current || busy) throw new Error("请等待当前操作完成。");
    const settings = normalizeCreativeSettings({ style, contentType });
    if (JSON.stringify(settings) === JSON.stringify(readCreativeSettings(unit))) return;
    lockRef.current = true;
    setSettingsSaving(true);
    try {
      await onMutateBatch({ mutations: [creativeSettingsMutation(unit.id, settings)], changeSetName: "修改创作设定" });
      setSaved((current) => ({ ...current, draft: { ...current.draft, ...settings } }));
      setError("");
    } finally { lockRef.current = false; setSettingsSaving(false); }
  };

  useEffect(() => {
    try { localStorage.setItem(key, JSON.stringify(saved)); } catch { setError("本地草稿未能保存，请勿关闭页面；仍可继续创作或采用当前结果。"); }
  }, [key, saved]);

  useEffect(() => {
    if (!saved.taskId) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      let task: AgentTask;
      try { task = await api.agentGetTask(project.path, saved.taskId!); } catch {
        if (!disposed) {
          setError("暂时无法读取进度，正在重新连接。任务与输入已保留。");
          timer = setTimeout(() => void poll(), 3000);
        }
        return;
      }
      try {
        if (disposed) return;
        setError("");
        if (["created", "context_building", "queued", "running"].includes(task.status)) {
          timer = setTimeout(() => void poll(), 1000);
          return;
        }
        let result: ScriptDraftResult | null = null;
        if (task.status === "completed") result = parseScriptResult(task.result, saved.expectedEpisodes);
        else if (task.status === "cancelled") setError("已停止，输入内容已保留。");
        else throw new Error(task.status === "stale" ? "项目内容已变化，请重新生成草稿。" : task.status === "waiting_for_user" ? "还需要补充故事设定，请完善输入后重试。" : toUserErrorMessage(task.error ?? "创作中断，请重试。"));
        setSaved((current) => ({ ...current, taskId: null, result }));
        onTaskSettled();
      } catch (reason) {
        if (!disposed) { setError(toUserErrorMessage(reason)); setSaved((current) => ({ ...current, taskId: null })); onTaskSettled(); }
      }
    };
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [saved.taskId, saved.expectedEpisodes, project.path, onTaskSettled]);

  const generate = async () => {
    if (busy || lockRef.current) return;
    lockRef.current = true;
    setError("");
    setStarting(true);
    setSaved((current) => ({ ...current, draft }));
    try {
      if (draft.mode === "import") {
        setSaved((current) => ({ ...current, draft, result: importedScript(draft.text, draft.fileName.replace(/\.(txt|md)$/i, "") || unit.name) }));
        return;
      }
      const message = buildScriptRequest(draft);
      const flags = await api.getFeatureFlags();
      if (!flags.agent_core || !flags.expert_agents) throw new Error("请先展开 Agent 并启用创作助手，再开始创作。");
      const session = await api.agentCreateSession(project.path, { requestId: crypto.randomUUID(), projectId: project.id, scopeType: "contentUnit", scopeId: unit.id, title: `剧本创作 · ${unit.name}` });
      const dispatch = await api.agentSendMessage(project.path, {
        requestId: crypto.randomUUID(), sessionId: session.id, message, workspace: "script", mode: "discussion",
        selection: buildAgentSelection({ projectId: project.id, revision: state.projects[0]?.revision ?? project.revision, objectType: "contentUnit", objectId: unit.id, field: null, selectedIds: [], currentUnitId: unit.id }),
        writeScope: { refs: [], protectedRefs: [] }, tokenBudget: 16_000,
      });
      const next = { ...saved, draft, taskId: dispatch.taskId, expectedEpisodes: draft.episodes, result: null };
      // Keep the handle even if the user switched workspaces while dispatch was pending.
      try { localStorage.setItem(key, JSON.stringify(next)); } catch { setError("任务已启动，但本地存储空间不足。请在本页等待完成。"); }
      setSaved(next);
    } catch (reason) { setError(toUserErrorMessage(reason)); } finally { setStarting(false); lockRef.current = false; }
  };

  const accept = async () => {
    if (!saved.result || lockRef.current) return;
    lockRef.current = true;
    setAccepting(true);
    setError("");
    try {
      const latest = await api.loadProjectState(project.path);
      const currentUnit = latest.contentUnits.find((item) => item.id === unit.id);
      if (!currentUnit) throw new Error("当前内容单元已不存在，草稿仍保留在本页。");
      const { mutations, firstUnitId } = scriptMutations(saved.result, latest, { ...currentUnit, creative_settings_json: JSON.stringify(normalizeCreativeSettings(saved.draft)) }, undefined, saved.draft.contentType);
      await onMutateBatch({ mutations, changeSetName: saved.draft.mode === "import" ? "导入剧本原稿" : "采用创作剧本" });
      const next = { ...saved, result: null, taskId: null };
      try {
        localStorage.setItem(key, JSON.stringify(next));
        for (const mutation of mutations.filter((item) => item.entityType === "contentUnit")) localStorage.setItem(storageKey(project.id, mutation.objectId!), JSON.stringify(next));
      } catch { /* The confirmed script is already saved in the project database. */ }
      setSaved(next);
      select({ contentUnitId: firstUnitId, objectType: "contentUnit", objectId: firstUnitId, field: null, selectedIds: [], selectionScope: `contentUnit:${firstUnitId}`, writeScope: `contentUnit:${firstUnitId}` });
    } catch (reason) { setError(toUserErrorMessage(reason)); } finally { setAccepting(false); lockRef.current = false; }
  };

  const manual = async () => {
    if (lockRef.current || busy) return;
    lockRef.current = true;
    setAccepting(true);
    try { await onManual(); } catch (reason) { setError(toUserErrorMessage(reason)); } finally { lockRef.current = false; setAccepting(false); }
  };
  return <ScriptStudio draft={draft} onChange={(next) => setSaved((current) => ({ ...current, draft: next }))} onSettingsChange={changeSettings} busy={busy} result={saved.result} error={error} onGenerate={() => void generate()} onAccept={() => void accept()} accepting={accepting || settingsSaving} onBack={() => setSaved((current) => ({ ...current, result: null }))} onManual={() => void manual()} onCancel={() => { if (saved.taskId) void api.agentCancelTask(saved.taskId).catch((reason) => setError(toUserErrorMessage(reason))); }} />;
}

export function ScriptStyleSettings({ unit, onMutateBatch }: { unit: ContentUnitRow; onMutateBatch: Props["onMutateBatch"] }) {
  const { style, contentType } = readCreativeSettings(unit);
  const [saving, setSaving] = useState(false);
  const change = async (next: CreativeStyle | null, nextType: CreationType) => {
    if (saving) throw new Error("正在保存，请稍候。");
    setSaving(true);
    try { await onMutateBatch({ mutations: [creativeSettingsMutation(unit.id, { style: next, contentType: nextType })], changeSetName: "修改创作设定" }); }
    finally { setSaving(false); }
  };
  return <div className="script-editor-toolbar"><StylePicker label="创作设定" value={style} contentType={contentType} disabled={saving} onChange={change} /></div>;
}
