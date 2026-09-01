import { Brain, ChevronDown, ChevronRight, Plus, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type {
  CreateMemoryInput,
  MemoryRecord,
  MemoryStorage,
  UpdateMemoryInput,
} from "../features/memory";
import type { ProjectDescriptor } from "../types";
import { useAppDialog } from "./AppDialog";

interface Props {
  project: ProjectDescriptor;
  currentUnitId: string | null;
  onError: (error: unknown) => void;
}

type MemoryUpdate = Omit<UpdateMemoryInput, "storage" | "memoryId">;

const statusLabels: Record<MemoryRecord["status"], string> = {
  candidate: "候选",
  active: "生效",
  superseded: "已替代",
  invalidated: "已失效",
};

export function MemoryPanel({ project, currentUnitId, onError }: Props) {
  const dialog = useAppDialog();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [creating, setCreating] = useState(false);
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [memories, setMemories] = useState<MemoryRecord[]>([]);
  const [storage, setStorage] = useState<MemoryStorage>("project");
  const [scope, setScope] = useState<"project" | "contentUnit">("project");
  const [category, setCategory] = useState("偏好");
  const [memoryKey, setMemoryKey] = useState("");
  const [content, setContent] = useState("");
  const [status, setStatus] = useState<"candidate" | "active">("candidate");

  const refresh = async (search = query) => {
    setMemories(await api.memoryList(project.path, search.trim() || undefined));
  };

  useEffect(() => {
    void api.getFeatureFlags()
      .then((flags) => {
        setEnabled(flags.memory);
        if (flags.memory) return refresh("");
      })
      .catch(onError);
    // The project path identifies the backing memory stores for this panel.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.path, onError]);

  useEffect(() => {
    if (!enabled) return;
    const timer = window.setTimeout(() => void refresh(query).catch(onError), 220);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, enabled]);

  const activeCount = useMemo(
    () => memories.filter((memory) => memory.status === "active").length,
    [memories],
  );

  const enable = async () => {
    setBusy(true);
    try {
      const flags = await api.setFeatureFlag("memory", true);
      setEnabled(flags.memory);
      setExpanded(true);
      await refresh("");
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  };

  const selectedScope = () => {
    if (storage === "global") return { scopeType: "global" as const, scopeId: undefined };
    if (scope === "contentUnit" && currentUnitId) {
      return { scopeType: "contentUnit" as const, scopeId: currentUnitId };
    }
    return { scopeType: "project" as const, scopeId: project.id };
  };

  const matchingConflict = (targetStorage: MemoryStorage, scopeType: string, scopeId: string | undefined, targetCategory: string, targetMemoryKey: string | null, exceptId?: string) =>
    targetMemoryKey ? memories.find((memory) => memory.id !== exceptId
      && memory.storage === targetStorage
      && memory.status === "active"
      && memory.scopeType === scopeType
      && memory.scopeId === (scopeId ?? null)
      && memory.category === targetCategory
      && memory.memoryKey === targetMemoryKey) : undefined;

  const create = async () => {
    if (!content.trim() || !category.trim()) return;
    const target = selectedScope();
    const conflictKey = memoryKey.trim() || null;
    const conflict = status === "active"
      ? matchingConflict(storage, target.scopeType, target.scopeId, category.trim(), conflictKey)
      : undefined;
    if (conflict && !await dialog.confirm(`同范围“${category.trim()}”已有生效记忆。新内容会替代旧内容。`, { title: "替代现有记忆？", confirmLabel: "替代" })) return;
    if (storage === "global" && status === "active" && !await dialog.confirm("它会在其他项目中也生效。", { title: "设为跨项目长期记忆？", confirmLabel: "设为长期记忆" })) return;
    const input: CreateMemoryInput = {
      requestId: crypto.randomUUID(),
      storage,
      ...target,
      category: category.trim(),
      memoryKey: conflictKey ?? undefined,
      content: content.trim(),
      status,
      sourceType: "user",
      excerpt: "用户在记忆面板中明确创建",
      supersedesId: conflict?.id,
      confirmed: storage === "project" || status === "active",
    };
    setBusy(true);
    try {
      await api.memoryCreate(project.path, input);
      setContent("");
      setMemoryKey("");
      setCreating(false);
      await refresh();
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  };

  const update = async (memory: MemoryRecord, values: MemoryUpdate) => {
    setBusy(true);
    try {
      await api.memoryUpdate(project.path, {
        storage: memory.storage,
        memoryId: memory.id,
        ...values,
      });
      await refresh();
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  };

  const activate = async (memory: MemoryRecord) => {
    const conflict = matchingConflict(memory.storage, memory.scopeType, memory.scopeId ?? undefined, memory.category, memory.memoryKey, memory.id);
    if (conflict && !await dialog.confirm(`激活后会替代“${conflict.content}”。`, { title: "激活并替代？", confirmLabel: "激活" })) return;
    if (memory.storage === "global" && !await dialog.confirm("它会在其他项目中也生效。", { title: "激活跨项目长期记忆？", confirmLabel: "激活" })) return;
    await update(memory, { status: "active", supersedesId: conflict?.id, confirmed: true });
  };

  const edit = async (memory: MemoryRecord) => {
    const next = await dialog.prompt("编辑记忆", { label: "记忆内容", defaultValue: memory.content, multiline: true });
    if (next && next !== memory.content) await update(memory, { content: next, confirmed: true });
  };

  const changeScope = async (memory: MemoryRecord) => {
    if (memory.storage !== "project") return;
    const toUnit = memory.scopeType === "project";
    if (toUnit && !currentUnitId) return;
    const scopeType = toUnit ? "contentUnit" : "project";
    const scopeId = toUnit ? currentUnitId! : project.id;
    const conflict = matchingConflict("project", scopeType, scopeId, memory.category, memory.memoryKey, memory.id);
    if (conflict && !await dialog.confirm(`变更范围后会替代“${conflict.content}”。`, { title: "变更记忆范围？", confirmLabel: "变更范围" })) return;
    await update(memory, {
      scopeType,
      scopeId,
      supersedesId: conflict?.id,
      confirmed: true,
    });
  };

  const supersede = async (memory: MemoryRecord) => {
    const replacement = await dialog.prompt("替代记忆", { label: "新的记忆内容", defaultValue: memory.content, multiline: true, confirmLabel: "替代" });
    if (!replacement || replacement === memory.content) return;
    if (memory.storage === "global" && !await dialog.confirm("新内容会作为跨项目长期记忆生效。", { title: "替代长期记忆？", confirmLabel: "替代" })) return;
    setBusy(true);
    try {
      await api.memoryCreate(project.path, {
        requestId: crypto.randomUUID(),
        storage: memory.storage,
        scopeType: memory.scopeType,
        scopeId: memory.scopeId ?? undefined,
        category: memory.category,
        memoryKey: memory.memoryKey ?? undefined,
        content: replacement,
        status: "active",
        sourceType: "user",
        excerpt: "用户在记忆面板中明确替代",
        supersedesId: memory.id,
        confirmed: true,
      });
      await refresh();
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  };

  const invalidate = async (memory: MemoryRecord) => {
    if (!await dialog.confirm("历史记录会保留，但此内容不再影响后续创作。", { title: "让这条记忆失效？", confirmLabel: "设为失效", danger: true })) return;
    setBusy(true);
    try {
      await api.memoryInvalidate(project.path, memory.storage, memory.id);
      await refresh();
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  };

  if (enabled === null) return null;
  if (!enabled) {
    return (
      <section className="memory-enable">
        <Brain size={14} /><span><strong>记忆</strong><small>默认关闭，仅保存明确内容</small></span>
        <button disabled={busy} onClick={() => void enable()}>{busy ? "启用中" : "启用"}</button>
      </section>
    );
  }

  return (
    <section className={`memory-panel ${expanded ? "expanded" : ""}`}>
      <button className="memory-heading" onClick={() => setExpanded((value) => !value)}>
        <Brain size={14} /><strong>记忆</strong><span>{activeCount} 生效 · {memories.length} 条</span>
        {expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
      </button>
      {expanded && (
        <div className="memory-body">
          <div className="memory-toolbar">
            <label><Search size={12} /><input aria-label="搜索记忆" value={query} placeholder="全文搜索" onChange={(event) => setQuery(event.target.value)} /></label>
            <button className="ghost" onClick={() => setCreating((value) => !value)}><Plus size={11} />新增</button>
          </div>
          {creating && (
            <div className="memory-create">
              <textarea aria-label="记忆内容" value={content} placeholder="需要明确记住的偏好或共识" onChange={(event) => setContent(event.target.value)} />
              <div>
                <input aria-label="记忆分类" value={category} placeholder="分类" onChange={(event) => setCategory(event.target.value)} />
                <input aria-label="记忆冲突键" value={memoryKey} placeholder="冲突键（可选；同键才互斥）" onChange={(event) => setMemoryKey(event.target.value)} />
                <select aria-label="存储位置" value={storage} onChange={(event) => setStorage(event.target.value as MemoryStorage)}>
                  <option value="project">当前项目</option><option value="global">跨项目长期</option>
                </select>
                {storage === "project" && <select aria-label="记忆范围" value={scope} onChange={(event) => setScope(event.target.value as typeof scope)}><option value="project">整个项目</option><option value="contentUnit" disabled={!currentUnitId}>当前内容单元</option></select>}
                <select aria-label="初始状态" value={status} onChange={(event) => setStatus(event.target.value as typeof status)}><option value="candidate">候选</option><option value="active">生效</option></select>
              </div>
              <button className="primary" disabled={busy || !content.trim()} title={busy ? "正在保存" : content.trim() ? "保存这条明确记忆" : "请先填写记忆内容"} onClick={() => void create()}>保存记忆</button>
            </div>
          )}
          <div className="memory-list">
            {!memories.length && <small className="memory-empty">暂无匹配记忆</small>}
            {memories.map((memory) => (
              <article className={`memory-card ${memory.status}`} key={`${memory.storage}:${memory.id}`}>
                <header><span>{memory.storage === "global" ? "长期" : memory.scopeType === "contentUnit" ? "当前单元" : "项目"}</span><strong>{memoryCategoryLabel(memory.category)}{memory.memoryKey ? ` · ${memory.memoryKey}` : ""}</strong><em>{statusLabels[memory.status]}</em></header>
                <p>{memory.content}</p>
                <small>来源：{memorySourceLabel(memory.sourceType)}{memory.sources[0]?.excerpt ? ` · ${memory.sources[0].excerpt}` : ""}</small>
                {memory.usedByTaskIds.length > 0 && <small>已用于 {memory.usedByTaskIds.length} 次 Agent 任务</small>}
                {memory.conflictIds.length > 0 && <small className="memory-conflict">与 {memory.conflictIds.length} 条生效记忆冲突，激活时需明确替代</small>}
                <footer>
                  {memory.status === "candidate" && <button onClick={() => void activate(memory)}>激活</button>}
                  {(memory.status === "candidate" || memory.status === "active") && <button onClick={() => void edit(memory)}>编辑</button>}
                  {memory.storage === "project" && (memory.status === "candidate" || memory.status === "active") && <button disabled={!currentUnitId && memory.scopeType === "project"} title={!currentUnitId && memory.scopeType === "project" ? "请先在左侧选择一个内容单元" : "调整记忆生效范围"} onClick={() => void changeScope(memory)}>{memory.scopeType === "project" ? "限当前单元" : "改为项目"}</button>}
                  {memory.status === "active" && <button onClick={() => void supersede(memory)}>替代</button>}
                  {(memory.status === "candidate" || memory.status === "active") && <button className="danger" onClick={() => void invalidate(memory)}>失效</button>}
                </footer>
              </article>
            ))}
          </div>
        </div>
      )}
    </section>
  );
}

function memoryCategoryLabel(category: string) {
  return ({ preference: "偏好", decision: "创作决策", constraint: "约束", style: "风格", character: "角色设定", workflow: "工作方式" } as Record<string, string>)[category] ?? category;
}

function memorySourceLabel(source: string) {
  return ({ user: "用户明确创建", agent: "Agent 建议", imported: "导入内容", system: "系统记录" } as Record<string, string>)[source] ?? "已记录来源";
}
