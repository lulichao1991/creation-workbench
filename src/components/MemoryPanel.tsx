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
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [creating, setCreating] = useState(false);
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [memories, setMemories] = useState<MemoryRecord[]>([]);
  const [storage, setStorage] = useState<MemoryStorage>("project");
  const [scope, setScope] = useState<"project" | "contentUnit">("project");
  const [category, setCategory] = useState("preference");
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
    if (conflict && !window.confirm(`同范围“${category.trim()}”已有生效记忆。明确替代它吗？`)) return;
    if (storage === "global" && status === "active" && !window.confirm("确认将这条内容设为跨项目长期记忆？")) return;
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
    if (conflict && !window.confirm(`激活会明确替代“${conflict.content}”。继续吗？`)) return;
    if (memory.storage === "global" && !window.confirm("确认激活这条跨项目长期记忆？")) return;
    await update(memory, { status: "active", supersedesId: conflict?.id, confirmed: true });
  };

  const edit = async (memory: MemoryRecord) => {
    const next = window.prompt("编辑记忆内容", memory.content)?.trim();
    if (next && next !== memory.content) await update(memory, { content: next, confirmed: true });
  };

  const changeScope = async (memory: MemoryRecord) => {
    if (memory.storage !== "project") return;
    const toUnit = memory.scopeType === "project";
    if (toUnit && !currentUnitId) return;
    const scopeType = toUnit ? "contentUnit" : "project";
    const scopeId = toUnit ? currentUnitId! : project.id;
    const conflict = matchingConflict("project", scopeType, scopeId, memory.category, memory.memoryKey, memory.id);
    if (conflict && !window.confirm(`变更范围会明确替代“${conflict.content}”。继续吗？`)) return;
    await update(memory, {
      scopeType,
      scopeId,
      supersedesId: conflict?.id,
      confirmed: true,
    });
  };

  const supersede = async (memory: MemoryRecord) => {
    const replacement = window.prompt("输入替代后的新记忆", memory.content)?.trim();
    if (!replacement || replacement === memory.content) return;
    if (memory.storage === "global" && !window.confirm("确认以新内容替代这条跨项目长期记忆？")) return;
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
    if (!window.confirm("确认让这条记忆失效？历史记录仍会保留。")) return;
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
              <button className="primary" disabled={busy || !content.trim()} onClick={() => void create()}>保存记忆</button>
            </div>
          )}
          <div className="memory-list">
            {!memories.length && <small className="memory-empty">暂无匹配记忆</small>}
            {memories.map((memory) => (
              <article className={`memory-card ${memory.status}`} key={`${memory.storage}:${memory.id}`}>
                <header><span>{memory.storage === "global" ? "长期" : memory.scopeType === "contentUnit" ? "当前单元" : "项目"}</span><strong>{memory.category}{memory.memoryKey ? ` · ${memory.memoryKey}` : ""}</strong><em>{statusLabels[memory.status]}</em></header>
                <p>{memory.content}</p>
                <small>来源：{memory.sourceType}{memory.sources[0]?.excerpt ? ` · ${memory.sources[0].excerpt}` : ""}</small>
                {memory.usedByTaskIds.length > 0 && <small>已用于任务：{memory.usedByTaskIds.join("、")}</small>}
                {memory.conflictIds.length > 0 && <small className="memory-conflict">与 {memory.conflictIds.length} 条生效记忆冲突，激活时需明确替代</small>}
                <footer>
                  {memory.status === "candidate" && <button onClick={() => void activate(memory)}>激活</button>}
                  {(memory.status === "candidate" || memory.status === "active") && <button onClick={() => void edit(memory)}>编辑</button>}
                  {memory.storage === "project" && (memory.status === "candidate" || memory.status === "active") && <button disabled={!currentUnitId && memory.scopeType === "project"} onClick={() => void changeScope(memory)}>{memory.scopeType === "project" ? "限当前单元" : "改为项目"}</button>}
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
