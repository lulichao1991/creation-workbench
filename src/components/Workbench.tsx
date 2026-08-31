import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeft,
  Bot,
  CheckCircle2,
  Clapperboard,
  FileText,
  History,
  Image as ImageIcon,
  Images,
  LayoutDashboard,
  Layers3,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Plus,
  Sparkles,
} from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { assetIdForSelection, orderedShotsForUnit, shotIdForSelection, supportsWorkspace } from "../domain/projectState";
import { useSelectionStore } from "../stores/selectionStore";
import type {
  AssetMediaRow,
  AssetRequirementRow,
  AssetRow,
  BatchMutationRequest,
  BatchMutationResponse,
  ContentUnitRow,
  GenerationTaskRow,
  KeyframeRow,
  MutationRequest,
  MutationResponse,
  ProjectDescriptor,
  ProjectState,
  SceneRow,
  ShotRow,
  Workspace,
} from "../types";
import { NumberField, SelectField, TextField } from "./Fields";
import { WorkspaceEmpty } from "./workspaces/WorkspaceEmpty";

interface Props {
  project: ProjectDescriptor;
  state: ProjectState;
  busy: boolean;
  onBack: () => void;
  onMutate: (request: MutationRequest) => Promise<MutationResponse>;
  onMutateBatch: (request: BatchMutationRequest) => Promise<BatchMutationResponse>;
  onRefresh: () => Promise<void>;
  onError: (error: unknown) => void;
  activeChangeSetId: string | null;
  onCloseChangeSet: () => void;
}

const tabs: Array<[Workspace, string]> = [
  ["overview", "作品结构"],
  ["script", "剧本"],
  ["shots", "分镜"],
  ["assets", "资产"],
  ["keyframes", "关键帧"],
  ["generation", "生成任务"],
  ["history", "历史 / 快照"],
];

const tabIcons = {
  overview: LayoutDashboard,
  script: FileText,
  shots: Clapperboard,
  assets: Images,
  keyframes: ImageIcon,
  generation: Sparkles,
  history: History,
};

export function Workbench({ project, state, busy, onBack, onMutate, onMutateBatch, onRefresh, onError, activeChangeSetId, onCloseChangeSet }: Props) {
  const selection = useSelectionStore();
  const [draggedUnitId, setDraggedUnitId] = useState<string | null>(null);
  const [leftCollapsed, setLeftCollapsed] = useState(false);
  const [rightCollapsed, setRightCollapsed] = useState(false);
  const projectRow = state.projects[0];

  useEffect(() => {
    selection.select({ projectId: project.id });
    if (!selection.contentUnitId && state.contentUnits.length > 0) {
      const preferred = state.contentUnits.find((unit) => unit.type === "episode") ?? state.contentUnits[0];
      selection.select({ contentUnitId: preferred.id });
    }
  }, [project.id, state.contentUnits.length]);

  const currentUnit = state.contentUnits.find((unit) => unit.id === selection.contentUnitId) ?? null;
  useEffect(() => {
    if (!supportsWorkspace(currentUnit, selection.workspace)) {
      selection.select({ workspace: "overview", objectType: null, objectId: null, field: null, selectionScope: null, writeScope: null, selectedIds: [] });
    }
  }, [currentUnit?.id, currentUnit?.type, selection.workspace]);
  const path = contentPath(state.contentUnits, currentUnit?.id ?? null);
  const currentChanges = activeChangeSetId
    ? state.changes.filter((change) => change.change_set_id === activeChangeSetId)
    : [];

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (
        event.ctrlKey &&
        event.key.toLowerCase() === "z" &&
        !["INPUT", "TEXTAREA", "SELECT"].includes(target?.tagName ?? "")
      ) {
        const latest = [...state.changeSets]
          .reverse()
          .find((changeSet) => changeSet.status === "closed" && changeSet.source_type !== "snapshot");
        if (latest) {
          event.preventDefault();
          onCloseChangeSet();
          void api.undoChangeSet(project.path, latest.id).then(onRefresh).catch(onError);
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [project.path, state.changeSets, onRefresh, onError, onCloseChangeSet]);

  const selectUnit = (unit: ContentUnitRow) => {
    selection.select({
      contentUnitId: unit.id,
      objectType: "contentUnit",
      objectId: unit.id,
      field: null,
      selectionScope: `contentUnit:${unit.id}`,
      writeScope: `contentUnit:${unit.id}`,
      selectedIds: [],
    });
  };

  const reorderUnits = async (draggedId: string, targetId: string) => {
    const dragged = state.contentUnits.find((unit) => unit.id === draggedId);
    const target = state.contentUnits.find((unit) => unit.id === targetId);
    if (!dragged || !target || dragged.parent_id !== target.parent_id) return;
    const siblings = state.contentUnits
      .filter((unit) => unit.parent_id === target.parent_id)
      .sort((a, b) => a.sort_order - b.sort_order);
    const from = siblings.findIndex((unit) => unit.id === draggedId);
    const to = siblings.findIndex((unit) => unit.id === targetId);
    if (from < 0 || to < 0 || from === to) return;
    const [moved] = siblings.splice(from, 1);
    siblings.splice(to, 0, moved);
    const mutations = siblings.flatMap((unit, index) => unit.sort_order === index ? [] : [{ action: "move" as const, entityType: "contentUnit", objectId: unit.id, values: { sort_order: index } }]);
    if (mutations.length) await onMutateBatch({ mutations, changeSetName: "调整内容结构" });
  };

  const moveUnit = async (unit: ContentUnitRow) => {
    const choices = ["根", ...state.contentUnits.filter((item) => item.id !== unit.id).map((item) => item.name)];
    const answer = window.prompt(`移动“${unit.name}”到哪个父级？\n${choices.join(" / ")}`, "根")?.trim();
    if (!answer) return;
    const parent = answer === "根" ? null : state.contentUnits.find((item) => item.name === answer);
    if (answer !== "根" && !parent) {
      onError(new Error("没有找到该父级，请输入列表中的完整名称。"));
      return;
    }
    const descendantIds = new Set<string>();
    let frontier = [unit.id];
    while (frontier.length) {
      const next = state.contentUnits.filter((item) => frontier.includes(item.parent_id ?? "")).map((item) => item.id);
      next.forEach((id) => descendantIds.add(id));
      frontier = next;
    }
    if (parent && descendantIds.has(parent.id)) {
      onError(new Error("不能把内容单元移动到自己的下级。"));
      return;
    }
    const siblings = state.contentUnits.filter((item) => item.parent_id === (parent?.id ?? null));
    await onMutate({ action: "move", entityType: "contentUnit", objectId: unit.id, values: { parent_id: parent?.id ?? null, sort_order: siblings.length }, changeSetName: "移动内容单元" });
  };

  return (
    <div className="app-shell">
      <header className="app-header">
        <button className="back-button" onClick={onBack} aria-label="返回项目列表"><ArrowLeft size={18} /></button>
        <span className="header-brand-mark"><Clapperboard size={17} /></span>
        <div className="project-title">
          <p className="eyebrow">CREATION WORKBENCH</p>
          <h1>{projectRow?.name ?? project.name}</h1>
        </div>
        <div className="header-status">
          {busy ? <span className="status-dot busy" /> : <CheckCircle2 size={14} className="saved-icon" />}
          <span>{busy ? "正在保存" : "已保存到本地"}</span>
          <span className="revision">修订 {projectRow?.revision ?? project.revision}</span>
        </div>
      </header>

      <div className="breadcrumb-bar">
        <span>{projectRow?.name ?? project.name}</span>
        {path.map((unit) => <span key={unit.id}>/ {unit.name}</span>)}
        <span>/ {tabs.find(([key]) => key === selection.workspace)?.[1]}</span>
        {selection.field && <strong>/ {selection.field}</strong>}
      </div>

      <nav className="workspace-tabs" aria-label="工作区导航">
        {tabs.map(([key, label]) => {
          const Icon = tabIcons[key];
          const enabled = supportsWorkspace(currentUnit, key);
          return (
          <button
            className={selection.workspace === key ? "active" : ""}
            key={key}
            disabled={!enabled}
            title={enabled ? label : "当前内容单元是结构容器，不能进入此生产工作区"}
            onClick={() => selection.select({ workspace: key, objectType: null, objectId: null, field: null, selectionScope: null, writeScope: null, selectedIds: [] })}
          >
            <Icon size={15} />{label}
          </button>
        );})}
      </nav>

      <div className={`workspace-grid ${leftCollapsed ? "left-collapsed" : ""} ${rightCollapsed ? "right-collapsed" : ""}`}>
        <aside className="left-panel">
          <div className="panel-heading">
            <div>
              <span className="label">作品结构</span>
              <strong>{state.contentUnits.length} 个内容单元</strong>
            </div>
            <button className="panel-toggle" title={leftCollapsed ? "展开作品结构" : "收起作品结构"} onClick={() => setLeftCollapsed((value) => !value)}>
              {leftCollapsed ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}
            </button>
            <button
              className="icon-button"
              title="新建顶层内容"
              onClick={() => void createContentUnit(null, state, project.id, onMutate)}
            ><Plus size={16} /></button>
          </div>
          {leftCollapsed && <span className="collapsed-rail-icon"><Layers3 size={17} /></span>}
          {state.contentUnits.length === 0 ? (
            <div className="panel-empty">先创建一个季、短片或正片。</div>
          ) : (
            <ContentTree
              units={state.contentUnits}
              parentId={null}
              selectedId={currentUnit?.id ?? null}
              onSelect={selectUnit}
              onAdd={(parent) => void createContentUnit(parent, state, project.id, onMutate)}
              onMove={(unit) => void moveUnit(unit).catch(onError)}
              onDelete={(unit) => {
                if (window.confirm(`删除“${unit.name}”？仅允许删除没有下游引用的内容单元。`)) {
                  void onMutate({ action: "delete", entityType: "contentUnit", objectId: unit.id, changeSetName: "删除内容单元" });
                }
              }}
              draggedId={draggedUnitId}
              onDragStart={setDraggedUnitId}
              onDrop={(targetId) => {
                if (draggedUnitId) void reorderUnits(draggedUnitId, targetId).catch(onError);
                setDraggedUnitId(null);
              }}
            />
          )}
        </aside>

        <main className="center-panel">
          {selection.workspace === "overview" && <OverviewWorkspace project={project} state={state} currentUnit={currentUnit} onMutate={onMutate} onMutateBatch={onMutateBatch} />}
          {selection.workspace === "script" && <ScriptWorkspace state={state} currentUnit={currentUnit} onMutate={onMutate} onMutateBatch={onMutateBatch} />}
          {selection.workspace === "shots" && <ShotsWorkspace state={state} currentUnit={currentUnit} onMutate={onMutate} onMutateBatch={onMutateBatch} />}
          {selection.workspace === "assets" && <AssetsWorkspace project={project} state={state} currentUnit={currentUnit} onMutate={onMutate} onMutateBatch={onMutateBatch} onError={onError} />}
          {selection.workspace === "keyframes" && <KeyframesWorkspace project={project} state={state} currentUnit={currentUnit} onMutate={onMutate} onError={onError} />}
          {selection.workspace === "generation" && <GenerationWorkspace state={state} currentUnit={currentUnit} onMutate={onMutate} onMutateBatch={onMutateBatch} />}
          {selection.workspace === "history" && <HistoryWorkspace project={project} state={state} onRefresh={onRefresh} onError={onError} onCloseChangeSet={onCloseChangeSet} />}
        </main>

        <aside className="right-panel">
          <div className="inspector-header">
            <span><Bot size={16} />上下文面板</span>
            <button className="panel-toggle" title={rightCollapsed ? "展开上下文面板" : "收起上下文面板"} onClick={() => setRightCollapsed((value) => !value)}>
              {rightCollapsed ? <PanelRightOpen size={16} /> : <PanelRightClose size={16} />}
            </button>
          </div>
          {rightCollapsed && <span className="collapsed-rail-icon"><Bot size={17} /></span>}
          <div className="inspector-body">
          <div className="inspector-section">
            <span className="label">当前选区</span>
            <strong>{selection.objectType ? `${selection.objectType} / ${selection.field ?? "完整对象"}` : "尚未选择对象"}</strong>
            <small>{selection.selectionScope ?? "点击内容以建立上下文"}</small>
          </div>
          <div className="inspector-section write-scope">
            <span className="label">默认写入范围</span>
            <code>{selection.writeScope ?? "未设置"}</code>
          </div>
          <div className="inspector-section agent-placeholder">
            <div className="agent-avatar">AI</div>
            <div>
              <strong>主 Agent 接入位置</strong>
              <p>第一阶段不调用 AI。当前选区和写入范围已持续记录，可供第二阶段上下文系统使用。</p>
            </div>
            <button disabled>第二阶段启用</button>
          </div>
          <div className="inspector-section">
            <span className="label">本轮修改</span>
            <strong>{currentChanges.length} 项原子变更</strong>
            {activeChangeSetId && <button className="ghost full" onClick={onCloseChangeSet}>结束本轮</button>}
            <button className="secondary full" onClick={() => selection.select({ workspace: "history" })}>查看历史与快照</button>
          </div>
          </div>
        </aside>
      </div>
    </div>
  );
}

interface ContentTreeProps {
  units: ContentUnitRow[];
  parentId: string | null;
  selectedId: string | null;
  onSelect: (unit: ContentUnitRow) => void;
  onAdd: (unit: ContentUnitRow) => void;
  onMove: (unit: ContentUnitRow) => void;
  onDelete: (unit: ContentUnitRow) => void;
  draggedId: string | null;
  onDragStart: (id: string) => void;
  onDrop: (id: string) => void;
  depth?: number;
}

function ContentTree(props: ContentTreeProps) {
  const depth = props.depth ?? 0;
  const children = props.units
    .filter((unit) => unit.parent_id === props.parentId)
    .sort((a, b) => a.sort_order - b.sort_order);
  return (
    <div className="content-tree">
      {children.map((unit) => (
        <div key={unit.id}>
          <div
            className={`tree-row ${props.selectedId === unit.id ? "selected" : ""} ${props.draggedId === unit.id ? "dragging" : ""}`}
            style={{ paddingLeft: 12 + depth * 16 }}
            draggable
            onDragStart={() => props.onDragStart(unit.id)}
            onDragOver={(event) => event.preventDefault()}
            onDrop={() => props.onDrop(unit.id)}
          >
            <button className="tree-main" onClick={() => props.onSelect(unit)}>
              <span className={`unit-icon ${unit.type}`}>{unitIcon(unit.type)}</span>
              <span>{unit.name}</span>
            </button>
            <button className="tree-action" title="添加子内容" onClick={() => props.onAdd(unit)}>＋</button>
            <button className="tree-action" title="移动到其他父级" onClick={() => props.onMove(unit)}>↳</button>
            <button className="tree-action danger" title="删除" onClick={() => props.onDelete(unit)}>×</button>
          </div>
          <ContentTree {...props} parentId={unit.id} depth={depth + 1} />
        </div>
      ))}
    </div>
  );
}

async function createContentUnit(parent: ContentUnitRow | null, state: ProjectState, projectId: string, mutate: Props["onMutate"]) {
  const suggested = parent?.type === "season" ? "episode" : parent?.type === "episode" ? "act" : parent ? "custom" : "season";
  const type = window.prompt("内容类型：season / episode / short / act / custom", suggested)?.trim();
  if (!type || !["season", "episode", "short", "act", "custom"].includes(type)) return;
  const name = window.prompt("内容名称", type === "season" ? "第一季" : type === "episode" ? "EP01" : "新内容")?.trim();
  if (!name) return;
  const siblings = state.contentUnits.filter((unit) => unit.parent_id === (parent?.id ?? null));
  await mutate({
    action: "create",
    entityType: "contentUnit",
    values: { project_id: projectId, parent_id: parent?.id ?? null, type, name, sort_order: siblings.length },
    changeSetName: "新建内容单元",
  });
}

function OverviewWorkspace({ project, state, currentUnit, onMutate, onMutateBatch }: { project: ProjectDescriptor; state: ProjectState; currentUnit: ContentUnitRow | null; onMutate: Props["onMutate"]; onMutateBatch: Props["onMutateBatch"] }) {
  const selection = useSelectionStore();
  const visible = currentUnit
    ? state.contentUnits.filter((unit) => unit.parent_id === currentUnit.id)
    : state.contentUnits.filter((unit) => unit.parent_id === null);
  const bulkCreateEpisodes = async () => {
    if (!currentUnit) return;
    const requested = Number(window.prompt("要补齐到多少集？", "30"));
    if (!Number.isInteger(requested) || requested < 1 || requested > 999) return;
    const existing = state.contentUnits.filter((unit) => unit.parent_id === currentUnit.id && unit.type === "episode");
    const mutations = Array.from({ length: requested - existing.length }, (_, offset) => {
      const index = existing.length + offset;
      return {
        action: "create",
        entityType: "contentUnit",
        values: {
          project_id: project.id,
          parent_id: currentUnit.id,
          type: "episode",
          name: `EP${String(index + 1).padStart(2, "0")}`,
          sort_order: index,
        },
      } as MutationRequest;
    });
    if (mutations.length) await onMutateBatch({ mutations, changeSetName: `批量建立 ${requested} 集` });
  };
  const expandShortToSeries = async () => {
    if (!currentUnit || currentUnit.type !== "short") return;
    const seasonName = window.prompt("新建季名称", "第一季")?.trim();
    if (!seasonName) return;
    const episodeName = window.prompt("原短片转为剧集名称", "EP01")?.trim();
    if (!episodeName) return;
    const seasonId = crypto.randomUUID();
    const topLevelCount = state.contentUnits.filter((unit) => unit.parent_id === null).length;
    await onMutateBatch({
      changeSetName: "短片扩展为系列",
      mutations: [
        { action: "create", entityType: "contentUnit", objectId: seasonId, values: { project_id: project.id, parent_id: null, type: "season", name: seasonName, sort_order: topLevelCount } },
        { action: "move", entityType: "contentUnit", objectId: currentUnit.id, values: { parent_id: seasonId, type: "episode", name: episodeName, sort_order: 0 } },
        { action: "patch", entityType: "project", objectId: project.id, values: { structure_type: "single-season" } },
      ],
    });
    selection.select({ contentUnitId: currentUnit.id, objectType: "contentUnit", objectId: currentUnit.id, selectionScope: `contentUnit:${currentUnit.id}`, writeScope: `contentUnit:${currentUnit.id}` });
  };
  return (
    <div className="workspace-content">
      <div className="workspace-heading">
        <div><p className="eyebrow">OVERVIEW</p><h2>{currentUnit?.name ?? "作品结构"}</h2><p>{currentUnit?.summary || "从结构、时间轴与语义关系观察作品。"}</p></div>
        <div className="heading-actions">
          {currentUnit?.type === "season" && <button className="secondary" onClick={() => void bulkCreateEpisodes()}>批量建立剧集</button>}
          {currentUnit?.type === "short" && currentUnit.parent_id === null && <button className="secondary" onClick={() => void expandShortToSeries()}>扩展为系列</button>}
          {currentUnit && <span className={`maturity ${currentUnit.maturity}`}>{maturityLabel(currentUnit.maturity)}</span>}
        </div>
      </div>
      {currentUnit && (
        <div className="editor-card">
          <TextField label="摘要" value={currentUnit.summary} multiline onFocus={() => selectField("contentUnit", currentUnit.id, "summary", selection.select)} onSave={(value) => onMutate({ action: "patch", entityType: "contentUnit", objectId: currentUnit.id, values: { summary: value }, changeSetName: "编辑内容摘要" }).then(() => undefined)} />
          <div className="field-grid compact">
            <SelectField label="成熟度" value={currentUnit.maturity} options={[["exploring", "探索中"], ["usable", "可用"], ["stable", "稳定"]]} onFocus={() => selectField("contentUnit", currentUnit.id, "maturity", selection.select)} onSave={(value) => onMutate({ action: "patch", entityType: "contentUnit", objectId: currentUnit.id, values: { maturity: value }, changeSetName: "更新成熟度" }).then(() => undefined)} />
            <SelectField label="同步状态" value={currentUnit.sync_status} options={[["normal", "正常"], ["needs_review", "待调整"], ["affected", "受影响"]]} onFocus={() => selectField("contentUnit", currentUnit.id, "sync_status", selection.select)} onSave={(value) => onMutate({ action: "patch", entityType: "contentUnit", objectId: currentUnit.id, values: { sync_status: value }, changeSetName: "更新同步状态" }).then(() => undefined)} />
          </div>
        </div>
      )}
      <section>
        <div className="section-heading inline"><div><span className="label">基础时间轴</span><h3>{visible.length ? `${visible.length} 个下级内容` : "暂无下级内容"}</h3></div></div>
        <div className="timeline">
          {visible.sort((a, b) => a.sort_order - b.sort_order).map((unit, index) => (
            <button className="timeline-card" key={unit.id} onClick={() => selection.select({ contentUnitId: unit.id, objectType: "contentUnit", objectId: unit.id, selectionScope: `contentUnit:${unit.id}`, writeScope: `contentUnit:${unit.id}` })}>
              <span>{String(index + 1).padStart(2, "0")}</span><strong>{unit.name}</strong><small>{unit.summary || "尚未填写摘要"}</small>
            </button>
          ))}
        </div>
      </section>
      <RelationEditor project={project} state={state} currentUnit={currentUnit} onMutate={onMutate} />
    </div>
  );
}

function RelationEditor({ project, state, currentUnit, onMutate }: { project: ProjectDescriptor; state: ProjectState; currentUnit: ContentUnitRow | null; onMutate: Props["onMutate"] }) {
  const relevant = currentUnit ? state.relations.filter((relation) => relation.source_id === currentUnit.id || relation.target_id === currentUnit.id) : state.relations;
  const add = async () => {
    if (!currentUnit) return;
    const targetId = window.prompt("目标内容单元 ID（可从左侧选择后复制）", state.contentUnits.find((unit) => unit.id !== currentUnit.id)?.id ?? "")?.trim();
    if (!targetId || !state.contentUnits.some((unit) => unit.id === targetId)) return;
    const relationType = window.prompt("关系语义", "主线推进")?.trim();
    if (!relationType) return;
    await onMutate({ action: "create", entityType: "relation", values: { project_id: project.id, source_type: "contentUnit", source_id: currentUnit.id, relation_type: relationType, target_type: "contentUnit", target_id: targetId, importance: 1 }, changeSetName: "新建关系" });
  };
  return (
    <section>
      <div className="section-heading inline"><div><span className="label">语义关系</span><h3>关系清单</h3></div><button className="secondary" disabled={!currentUnit} onClick={() => void add()}>＋ 添加关系</button></div>
      <div className="relation-list">
        {relevant.length === 0 ? <div className="panel-empty">当前范围还没有关系。</div> : relevant.map((relation) => (
          <div className="relation-row" key={relation.id}><span>{relation.source_type}</span><strong>{relation.relation_type}</strong><span>→ {relation.target_type}</span><small>{relation.description}</small><button className="danger-text" onClick={() => void onMutate({ action: "delete", entityType: "relation", objectId: relation.id, changeSetName: "删除关系" })}>删除</button></div>
        ))}
      </div>
    </section>
  );
}

function ScriptWorkspace({ state, currentUnit, onMutate, onMutateBatch }: { state: ProjectState; currentUnit: ContentUnitRow | null; onMutate: Props["onMutate"]; onMutateBatch: Props["onMutateBatch"] }) {
  const selection = useSelectionStore();
  const script = state.scripts.find((item) => item.content_unit_id === currentUnit?.id);
  const scenes = script ? state.scenes.filter((scene) => scene.script_id === script.id).sort((a, b) => a.sort_order - b.sort_order) : [];
  const selectedScene = scenes.find((scene) => scene.id === selection.objectId) ?? scenes[0];
  if (!currentUnit) return <WorkspaceEmpty title="请选择内容单元" text="从左侧选择一集、短片或正片。" />;
  if (!script) return <WorkspaceEmpty title="尚未建立剧本" text={`为“${currentUnit.name}”创建剧本后即可添加场。`} action="创建剧本" onAction={() => void onMutate({ action: "create", entityType: "script", values: { content_unit_id: currentUnit.id, title: currentUnit.name }, changeSetName: "创建剧本" })} />;

  const addScene = async () => {
    const title = window.prompt("场标题", `场 ${String(scenes.length + 1).padStart(2, "0")}`)?.trim();
    if (!title) return;
    const result = await onMutate({ action: "create", entityType: "scene", values: { script_id: script.id, title, sort_order: scenes.length }, changeSetName: "新增场" });
    selection.select({ objectType: "scene", objectId: result.objectId, selectionScope: `scene:${result.objectId}`, writeScope: `scene:${result.objectId}` });
  };
  const moveScene = async (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (target < 0 || target >= scenes.length) return;
    await onMutateBatch({ changeSetName: "调整场顺序", mutations: [
      { action: "move", entityType: "scene", objectId: scenes[index].id, values: { sort_order: target } },
      { action: "move", entityType: "scene", objectId: scenes[target].id, values: { sort_order: index } },
    ] });
  };
  return (
    <div className="workspace-content split-workspace">
      <div className="sub-list">
        <div className="panel-heading"><div><span className="label">场列表</span><strong>{scenes.length} 场</strong></div><button className="icon-button" onClick={() => void addScene()}>＋</button></div>
        {scenes.map((scene, index) => <div className="sortable-list-row" key={scene.id}><button className={`sub-list-row ${selectedScene?.id === scene.id ? "selected" : ""}`} onClick={() => selection.select({ objectType: "scene", objectId: scene.id, selectionScope: `scene:${scene.id}`, writeScope: `scene:${scene.id}` })}><span>{String(index + 1).padStart(2, "0")}</span><strong>{scene.title}</strong><small>{scene.location_text || "地点未定"}</small></button><div><button disabled={index === 0} onClick={() => void moveScene(index, -1)}>↑</button><button disabled={index === scenes.length - 1} onClick={() => void moveScene(index, 1)}>↓</button></div></div>)}
      </div>
      <div className="editor-area">
        {!selectedScene ? <WorkspaceEmpty title="剧本为空" text="添加第一场开始写作。" action="新增场" onAction={() => void addScene()} /> : <SceneEditor scene={selectedScene} onMutate={onMutate} />}
      </div>
    </div>
  );
}

function SceneEditor({ scene, onMutate }: { scene: SceneRow; onMutate: Props["onMutate"] }) {
  const selection = useSelectionStore();
  const patch = (field: string, value: unknown) => onMutate({ action: "patch", entityType: "scene", objectId: scene.id, values: { [field]: value }, changeSetName: "编辑剧本场" }).then(() => undefined);
  return (
    <div className="editor-card scene-editor">
      <div className="workspace-heading"><div><p className="eyebrow">SCRIPT SCENE</p><h2>{scene.title}</h2></div><button className="danger-text" onClick={() => { if (window.confirm("删除当前场？场内存在镜头时数据库会拒绝删除。")) void onMutate({ action: "delete", entityType: "scene", objectId: scene.id, changeSetName: "删除场" }); }}>删除场</button></div>
      <div className="field-grid">
        <TextField label="标题" value={scene.title} onFocus={() => selectField("scene", scene.id, "title", selection.select)} onSave={(value) => patch("title", value)} />
        <TextField label="地点" value={scene.location_text} onFocus={() => selectField("scene", scene.id, "location_text", selection.select)} onSave={(value) => patch("location_text", value)} />
        <TextField label="时间" value={scene.time_text} onFocus={() => selectField("scene", scene.id, "time_text", selection.select)} onSave={(value) => patch("time_text", value)} />
        <TextField label="摘要" value={scene.summary} multiline onFocus={() => selectField("scene", scene.id, "summary", selection.select)} onSave={(value) => patch("summary", value)} />
        <TextField label="剧本文本" value={scene.content} multiline onFocus={() => selectField("scene", scene.id, "content", selection.select)} onSave={(value) => patch("content", value)} />
      </div>
    </div>
  );
}

function ShotsWorkspace({ state, currentUnit, onMutate, onMutateBatch }: { state: ProjectState; currentUnit: ContentUnitRow | null; onMutate: Props["onMutate"]; onMutateBatch: Props["onMutateBatch"] }) {
  const selection = useSelectionStore();
  const script = state.scripts.find((item) => item.content_unit_id === currentUnit?.id);
  const scenes = script ? state.scenes.filter((scene) => scene.script_id === script.id).sort((a, b) => a.sort_order - b.sort_order) : [];
  const fallbackScene = scenes[0];
  const selectedShotFromStore = state.shots.find((shot) => shot.id === selection.objectId);
  const [sceneId, setSceneId] = useState<string | null>(selectedShotFromStore?.scene_id ?? fallbackScene?.id ?? null);
  useEffect(() => { if (!scenes.some((scene) => scene.id === sceneId)) setSceneId(fallbackScene?.id ?? null); }, [currentUnit?.id, scenes.length]);
  const shots = state.shots.filter((shot) => shot.scene_id === sceneId).sort((a, b) => a.sort_order - b.sort_order);
  const selectedShot = shots.find((shot) => shot.id === selection.objectId) ?? shots[0];
  if (!script || scenes.length === 0) return <WorkspaceEmpty title="先建立剧本场" text="分镜必须归属于一个剧本场。" />;
  const addShot = async () => {
    if (!sceneId) return;
    const result = await onMutate({ action: "create", entityType: "shot", values: { scene_id: sceneId, sort_order: shots.length, title: `镜头 ${String(shots.length + 1).padStart(2, "0")}`, duration: 2 }, changeSetName: "新增镜头" });
    selection.select({ objectType: "shot", objectId: result.objectId, selectionScope: `shot:${result.objectId}`, writeScope: `shot:${result.objectId}` });
  };
  const duplicate = async () => {
    if (!selectedShot) return;
    const { id: _id, created_at: _created, updated_at: _updated, ...values } = selectedShot;
    await onMutate({ action: "create", entityType: "shot", values: { ...values, sort_order: shots.length, title: `${selectedShot.title} 副本` }, changeSetName: "复制镜头" });
  };
  const reorder = async (draggedId: string, targetId: string) => {
    const ordered = [...shots];
    const from = ordered.findIndex((shot) => shot.id === draggedId);
    const to = ordered.findIndex((shot) => shot.id === targetId);
    if (from < 0 || to < 0 || from === to) return;
    const [moved] = ordered.splice(from, 1); ordered.splice(to, 0, moved);
    const mutations = ordered.flatMap((shot, index) => shot.sort_order === index ? [] : [{ action: "move" as const, entityType: "shot", objectId: shot.id, values: { sort_order: index } }]);
    if (mutations.length) await onMutateBatch({ mutations, changeSetName: "调整镜头顺序" });
  };
  return (
    <div className="workspace-content">
      <div className="workspace-heading"><div><p className="eyebrow">STORYBOARD</p><h2>结构化分镜</h2><p>镜头稳定 ID 与界面序号分离；拖动镜头行可调整顺序。</p></div><div className="heading-actions"><select value={sceneId ?? ""} onChange={(event) => setSceneId(event.target.value)}>{scenes.map((scene) => <option value={scene.id} key={scene.id}>{scene.title}</option>)}</select><button className="secondary" onClick={() => void addShot()}>＋ 镜头</button><button className="ghost" disabled={!selectedShot} onClick={() => void duplicate()}>复制</button></div></div>
      <div className="shot-table-wrap"><table className="shot-table"><thead><tr><th></th><th>编号</th><th>时长</th><th>景别</th><th>主体</th><th>核心动作</th><th>叙事目的</th><th>成熟度</th></tr></thead><tbody>{shots.map((shot, index) => <tr key={shot.id} draggable onDragStart={(event) => event.dataTransfer.setData("shotId", shot.id)} onDragOver={(event) => event.preventDefault()} onDrop={(event) => void reorder(event.dataTransfer.getData("shotId"), shot.id)} className={selectedShot?.id === shot.id ? "selected" : ""} onClick={() => selection.select({ objectType: "shot", objectId: shot.id, selectionScope: `shot:${shot.id}`, writeScope: `shot:${shot.id}` })}><td><input type="checkbox" checked={selection.selectedIds.includes(shot.id)} onClick={(event) => event.stopPropagation()} onChange={(event) => selection.select({ selectedIds: event.target.checked ? [...selection.selectedIds, shot.id] : selection.selectedIds.filter((id) => id !== shot.id) })} /></td><td>#{String(index + 1).padStart(2, "0")}</td><td>{shot.duration}s</td><td>{shot.shot_size || "—"}</td><td>{shot.subjects || "—"}</td><td>{shot.action || "—"}</td><td>{shot.narrative_purpose || "—"}</td><td><span className={`maturity tiny ${shot.maturity}`}>{maturityLabel(shot.maturity)}</span></td></tr>)}</tbody></table></div>
      {selectedShot ? <ShotEditor shot={selectedShot} index={shots.findIndex((shot) => shot.id === selectedShot.id)} state={state} onMutate={onMutate} onMutateBatch={onMutateBatch} /> : <WorkspaceEmpty title="还没有镜头" text="添加第一个镜头开始结构化分镜。" action="新增镜头" onAction={() => void addShot()} />}
    </div>
  );
}

function ShotEditor({ shot, index, state, onMutate, onMutateBatch }: { shot: ShotRow; index: number; state: ProjectState; onMutate: Props["onMutate"]; onMutateBatch: Props["onMutateBatch"] }) {
  const selection = useSelectionStore();
  const links = state.shotAssets.filter((link) => link.shot_id === shot.id);
  const linkedIds = new Set(links.map((link) => link.asset_id));
  const patch = (field: string, value: unknown) => onMutate({ action: "patch", entityType: "shot", objectId: shot.id, values: { [field]: value }, changeSetName: `编辑镜头 ${String(index + 1).padStart(2, "0")}` }).then(() => undefined);
  const focus = (field: string) => selectField("shot", shot.id, field, selection.select);
  const toggleAsset = async (asset: AssetRow, checked: boolean) => {
    const existing = links.find((link) => link.asset_id === asset.id);
    if (checked && !existing) {
      await onMutateBatch({ changeSetName: "关联镜头资产", mutations: [{ action: "create", entityType: "shotAsset", values: { shot_id: shot.id, asset_id: asset.id, role: asset.type === "location" ? "environment" : asset.type === "prop" ? "prop" : "subject" } }] });
    } else if (!checked && existing) {
      await onMutateBatch({ changeSetName: "移除镜头资产", mutations: [{ action: "delete", entityType: "shotAsset", objectId: existing.id }] });
    }
  };
  return (
    <div className="editor-card shot-editor">
      <div className="workspace-heading"><div><p className="eyebrow">SHOT #{String(index + 1).padStart(2, "0")}</p><h3>{shot.title}</h3></div><button className="danger-text" onClick={() => { if (window.confirm("删除当前镜头及其关键帧、任务和资产关联？本轮修改可以撤销。")) void onMutate({ action: "delete", entityType: "shot", objectId: shot.id, changeSetName: "删除镜头及关联" }).then(() => selection.select({ objectType: null, objectId: null, field: null, selectionScope: null, writeScope: null })); }}>删除镜头</button></div>
      <div className="field-grid three">
        <TextField label="标题" value={shot.title} onFocus={() => focus("title")} onSave={(v) => patch("title", v)} />
        <NumberField label="时长（秒）" value={shot.duration} step={0.1} onFocus={() => focus("duration")} onSave={(v) => patch("duration", v)} />
        <TextField label="景别" value={shot.shot_size} onFocus={() => focus("shot_size")} onSave={(v) => patch("shot_size", v)} />
        <TextField label="机位高度" value={shot.camera_height} onFocus={() => focus("camera_height")} onSave={(v) => patch("camera_height", v)} />
        <TextField label="拍摄方向" value={shot.camera_direction} onFocus={() => focus("camera_direction")} onSave={(v) => patch("camera_direction", v)} />
        <TextField label="运镜" value={shot.camera_movement} onFocus={() => focus("camera_movement")} onSave={(v) => patch("camera_movement", v)} />
        <TextField label="主体" value={shot.subjects} onFocus={() => focus("subjects")} onSave={(v) => patch("subjects", v)} />
        <TextField label="动作" value={shot.action} onFocus={() => focus("action")} onSave={(v) => patch("action", v)} />
        <TextField label="对白" value={shot.dialogue} onFocus={() => focus("dialogue")} onSave={(v) => patch("dialogue", v)} />
        <TextField label="叙事目的" value={shot.narrative_purpose} multiline onFocus={() => focus("narrative_purpose")} onSave={(v) => patch("narrative_purpose", v)} />
        <TextField label="新信息" value={shot.new_information} multiline onFocus={() => focus("new_information")} onSave={(v) => patch("new_information", v)} />
        <TextField label="构图" value={shot.composition} multiline onFocus={() => focus("composition")} onSave={(v) => patch("composition", v)} />
        <TextField label="环境" value={shot.environment} multiline onFocus={() => focus("environment")} onSave={(v) => patch("environment", v)} />
        <TextField label="起始状态" value={shot.start_state} multiline onFocus={() => focus("start_state")} onSave={(v) => patch("start_state", v)} />
        <TextField label="结束状态" value={shot.end_state} multiline onFocus={() => focus("end_state")} onSave={(v) => patch("end_state", v)} />
      </div>
      <div className="section-heading inline"><div><span className="label">正式资产引用</span><h3>{links.length} 项</h3></div></div>
      <div className="asset-link-grid">{state.assets.map((asset) => <label key={asset.id}><input type="checkbox" checked={linkedIds.has(asset.id)} onChange={(event) => void toggleAsset(asset, event.target.checked)} /><span className={`unit-icon ${asset.type}`}>{unitIcon(asset.type)}</span><span>{asset.name}</span><small>{assetTypeLabel(asset.type)}</small></label>)}</div>
    </div>
  );
}

function AssetsWorkspace({ project, state, currentUnit, onMutate, onMutateBatch, onError }: { project: ProjectDescriptor; state: ProjectState; currentUnit: ContentUnitRow | null; onMutate: Props["onMutate"]; onMutateBatch: Props["onMutateBatch"]; onError: Props["onError"] }) {
  const selection = useSelectionStore();
  const [type, setType] = useState<AssetRow["type"]>("character");
  const assets = state.assets.filter((asset) => asset.type === type);
  const selectedAssetId = assetIdForSelection(state, selection.objectType, selection.objectId);
  const selected = assets.find((asset) => asset.id === selectedAssetId) ?? assets[0];
  const add = async () => {
    const name = window.prompt("资产名称")?.trim(); if (!name) return;
    const result = await onMutate({ action: "create", entityType: "asset", values: { project_id: project.id, type, name, scope_unit_id: currentUnit?.id ?? null }, changeSetName: "新建资产" });
    selection.select({ objectType: "asset", objectId: result.objectId, selectionScope: `asset:${result.objectId}`, writeScope: `asset:${result.objectId}` });
  };
  return (
    <div className="workspace-content split-workspace">
      <div className="sub-list"><div className="asset-type-tabs">{(["character", "location", "prop"] as const).map((value) => <button className={type === value ? "active" : ""} key={value} onClick={() => { setType(value); selection.select({ objectType: null, objectId: null }); }}>{assetTypeLabel(value)}</button>)}</div><div className="panel-heading"><div><span className="label">资产</span><strong>{assets.length} 项</strong></div><button className="icon-button" onClick={() => void add()}>＋</button></div>{assets.map((asset) => <button className={`sub-list-row ${selected?.id === asset.id ? "selected" : ""}`} key={asset.id} onClick={() => selection.select({ objectType: "asset", objectId: asset.id, selectionScope: `asset:${asset.id}`, writeScope: `asset:${asset.id}` })}><span className={`unit-icon ${asset.type}`}>{unitIcon(asset.type)}</span><strong>{asset.name}</strong><small>{asset.description || "尚无视觉定义"}</small></button>)}</div>
      <div className="editor-area">{selected ? <AssetEditor project={project} asset={selected} state={state} currentUnit={currentUnit} onMutate={onMutate} onMutateBatch={onMutateBatch} onError={onError} /> : <WorkspaceEmpty title={`还没有${assetTypeLabel(type)}资产`} text="建立文字视觉定义，再导入正式图片。" action="新建资产" onAction={() => void add()} />}</div>
    </div>
  );
}

function AssetEditor({ project, asset, state, currentUnit, onMutate, onMutateBatch, onError }: { project: ProjectDescriptor; asset: AssetRow; state: ProjectState; currentUnit: ContentUnitRow | null; onMutate: Props["onMutate"]; onMutateBatch: Props["onMutateBatch"]; onError: Props["onError"] }) {
  const selection = useSelectionStore();
  const media = state.assetMedia.filter((item) => item.asset_id === asset.id).sort((a, b) => a.sort_order - b.sort_order);
  const requirements = state.assetRequirements.filter((item) => item.asset_id === asset.id);
  const sourceShots = orderedShotsForUnit(state, currentUnit?.id ?? null);
  const patch = (field: string, value: unknown) => onMutate({ action: "patch", entityType: "asset", objectId: asset.id, values: { [field]: value }, changeSetName: "编辑资产" }).then(() => undefined);
  const addRequirement = async () => {
    const requirementType = window.prompt("需求类型，例如：标准主图、3/4 侧面、背面", "标准主图")?.trim(); if (!requirementType) return;
    const requirementId = crypto.randomUUID();
    const source = chooseShot(sourceShots, "可选：输入来源镜头序号；留空表示暂不关联");
    await onMutateBatch({ changeSetName: "新建资产需求", mutations: [
      { action: "create", entityType: "assetRequirement", objectId: requirementId, values: { content_unit_id: currentUnit?.id ?? null, asset_id: asset.id, asset_type: asset.type, requirement_type: requirementType, status: "planned" } },
      ...(source ? [{ action: "create" as const, entityType: "assetRequirementSource", values: { asset_requirement_id: requirementId, source_type: "shot", source_id: source.id } }] : []),
    ] });
  };
  const addRequirementSource = async (requirementId: string) => {
    const source = chooseShot(sourceShots, "输入要追加的来源镜头序号");
    if (!source) return;
    const duplicate = state.assetRequirementSources.some((item) => item.asset_requirement_id === requirementId && item.source_type === "shot" && item.source_id === source.id);
    if (!duplicate) await onMutateBatch({ changeSetName: "追加资产需求来源", mutations: [{ action: "create", entityType: "assetRequirementSource", values: { asset_requirement_id: requirementId, source_type: "shot", source_id: source.id } }] });
  };
  const importMedia = async () => {
    const selected = await open({ multiple: false, filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp"] }] });
    if (typeof selected !== "string") return;
    try {
      const relative = await api.importProjectFile(project.path, selected, asset.type);
      const mediaId = crypto.randomUUID();
      const label = window.prompt("图片标签", requirements[0]?.requirement_type ?? "手动导入")?.trim() || "手动导入";
      const requirement = chooseRequirement(requirements, "可选：输入此图片满足的需求序号；留空暂不关联");
      await onMutateBatch({ changeSetName: "导入资产图片", mutations: [
        { action: "create", entityType: "assetMedia", objectId: mediaId, values: { asset_id: asset.id, media_type: "image", file_path: relative, label, sort_order: media.length, is_primary: media.length === 0 ? 1 : 0, source_type: "manual" } },
        ...(requirement ? [{ action: "create" as const, entityType: "assetMediaRequirement", values: { asset_media_id: mediaId, asset_requirement_id: requirement.id } }] : []),
      ] });
    } catch (error) { await api.cleanupProjectMedia(project.path).catch(() => undefined); onError(error); }
  };
  const setPrimary = async (target: AssetMediaRow) => {
    const mutations = media.flatMap((item) => item.is_primary === (item.id === target.id ? 1 : 0) ? [] : [{ action: "patch" as const, entityType: "assetMedia", objectId: item.id, values: { is_primary: item.id === target.id ? 1 : 0 } }]);
    if (mutations.length) await onMutateBatch({ mutations, changeSetName: "设置资产主图" });
  };
  const deleteRequirement = async (requirementId: string) => {
    const sourceLinks = state.assetRequirementSources.filter((item) => item.asset_requirement_id === requirementId);
    const mediaLinks = state.assetMediaRequirements.filter((item) => item.asset_requirement_id === requirementId);
    await onMutateBatch({ changeSetName: "删除资产需求", mutations: [...sourceLinks.map((item) => ({ action: "delete" as const, entityType: "assetRequirementSource", objectId: item.id })), ...mediaLinks.map((item) => ({ action: "delete" as const, entityType: "assetMediaRequirement", objectId: item.id })), { action: "delete", entityType: "assetRequirement", objectId: requirementId }] });
  };
  const deleteMedia = async (mediaId: string) => {
    const links = state.assetMediaRequirements.filter((item) => item.asset_media_id === mediaId);
    await onMutateBatch({ changeSetName: "移除资产图片", mutations: [...links.map((item) => ({ action: "delete" as const, entityType: "assetMediaRequirement", objectId: item.id })), { action: "delete", entityType: "assetMedia", objectId: mediaId }] });
  };
  return (
    <div>
      <div className="workspace-heading"><div><p className="eyebrow">{asset.type.toUpperCase()} ASSET</p><h2>{asset.name}</h2></div><button className="danger-text" onClick={() => { if (window.confirm("删除当前资产及其图片、需求和镜头关联？本轮修改可以撤销。")) void onMutate({ action: "delete", entityType: "asset", objectId: asset.id, changeSetName: "删除资产及关联" }).then(() => selection.select({ objectType: null, objectId: null, field: null, selectionScope: null, writeScope: null })); }}>删除资产</button></div>
      <div className="editor-card"><div className="field-grid"><TextField label="名称" value={asset.name} onFocus={() => selectField("asset", asset.id, "name", selection.select)} onSave={(v) => patch("name", v)} /><SelectField label="作用范围" value={asset.scope_unit_id ?? ""} options={[["", "项目共享"], ...state.contentUnits.map((unit) => [unit.id, unit.name] as [string, string])]} onFocus={() => selectField("asset", asset.id, "scope_unit_id", selection.select)} onSave={(v) => patch("scope_unit_id", v || null)} /><TextField label="文字视觉定义" value={asset.description} multiline onFocus={() => selectField("asset", asset.id, "description", selection.select)} onSave={(v) => patch("description", v)} /></div></div>
      <section><div className="section-heading inline"><div><span className="label">资产需求</span><h3>{requirements.length} 项需求</h3></div><button className="secondary" onClick={() => void addRequirement()}>＋ 添加需求</button></div>{requirements.map((requirement) => { const sources = state.assetRequirementSources.filter((item) => item.asset_requirement_id === requirement.id); return <div className="requirement-card" key={requirement.id}><TextField label="需求类型" value={requirement.requirement_type} onFocus={() => selectField("assetRequirement", requirement.id, "requirement_type", selection.select)} onSave={(v) => onMutate({ action: "patch", entityType: "assetRequirement", objectId: requirement.id, values: { requirement_type: v }, changeSetName: "编辑资产需求" }).then(() => undefined)} /><TextField label="描述" value={requirement.description} multiline onFocus={() => selectField("assetRequirement", requirement.id, "description", selection.select)} onSave={(v) => onMutate({ action: "patch", entityType: "assetRequirement", objectId: requirement.id, values: { description: v }, changeSetName: "编辑资产需求" }).then(() => undefined)} /><TextField label="专业提示词草稿" value={requirement.prompt_draft} multiline onFocus={() => selectField("assetRequirement", requirement.id, "prompt_draft", selection.select)} onSave={(v) => onMutate({ action: "patch", entityType: "assetRequirement", objectId: requirement.id, values: { prompt_draft: v }, changeSetName: "编辑资产提示词" }).then(() => undefined)} /><div className="requirement-sources"><span>来源：{sources.length ? sources.map((source) => state.shots.find((shot) => shot.id === source.source_id)?.title ?? source.source_id).join("、") : "未关联镜头"}</span><button className="ghost" onClick={() => void addRequirementSource(requirement.id)}>追加来源</button></div><button className="danger-text" onClick={() => void deleteRequirement(requirement.id)}>删除需求</button></div>; })}</section>
      <section><div className="section-heading inline"><div><span className="label">正式图片</span><h3>{media.length} 张图片</h3></div><div className="heading-actions"><button className="ghost" disabled title="第一阶段不接真实生图 API">生成图片</button><button className="secondary" onClick={() => void importMedia()}>导入图片</button></div></div><div className="media-grid">{media.map((item) => { const satisfied = state.assetMediaRequirements.filter((link) => link.asset_media_id === item.id).map((link) => requirements.find((requirement) => requirement.id === link.asset_requirement_id)?.requirement_type).filter(Boolean); return <div className={`media-card ${item.is_primary ? "primary-media" : ""}`} key={item.id}><MediaImage projectPath={project.path} relativePath={item.file_path} alt={asset.name} /><div><strong>{item.label || "资产图片"}</strong><small>{satisfied.length ? `满足：${satisfied.join("、")}` : item.is_primary ? "正式主图" : "候选角度"}</small></div><div className="card-actions">{!item.is_primary && <button className="ghost" onClick={() => void setPrimary(item)}>设为主图</button>}<button className="danger-text" onClick={() => void deleteMedia(item.id)}>移除</button></div></div>; })}</div></section>
    </div>
  );
}

function KeyframesWorkspace({ project, state, currentUnit, onMutate, onError }: { project: ProjectDescriptor; state: ProjectState; currentUnit: ContentUnitRow | null; onMutate: Props["onMutate"]; onError: Props["onError"] }) {
  const selection = useSelectionStore();
  const shots = orderedShotsForUnit(state, currentUnit?.id ?? null);
  const selectedShotId = shotIdForSelection(state, selection.objectType, selection.objectId);
  const selectedShot = shots.find((shot) => shot.id === selectedShotId) ?? shots[0];
  const keyframes = state.keyframes.filter((keyframe) => keyframe.shot_id === selectedShot?.id).sort((a, b) => a.sort_order - b.sort_order);
  const selectedFrame = keyframes.find((frame) => frame.id === (selection.objectType === "keyframe" ? selection.objectId : null)) ?? keyframes[0];
  const inheritedAssets = state.shotAssets.filter((link) => link.shot_id === selectedShot?.id).map((link) => state.assets.find((asset) => asset.id === link.asset_id)).filter((asset): asset is AssetRow => Boolean(asset));
  const add = async () => {
    if (!selectedShot) return;
    const type = window.prompt("关键帧类型：single / start / middle / end", "single")?.trim(); if (!type || !["single", "start", "middle", "end"].includes(type)) return;
    const result = await onMutate({ action: "create", entityType: "keyframe", values: { shot_id: selectedShot.id, type, status: "planned", sort_order: keyframes.length }, changeSetName: "新建关键帧需求" });
    selection.select({ objectType: "keyframe", objectId: result.objectId, selectionScope: `keyframe:${result.objectId}`, writeScope: `keyframe:${result.objectId}` });
  };
  if (!selectedShot) return <WorkspaceEmpty title="当前内容还没有镜头" text="建立分镜后才能规划关键帧。" />;
  return (
    <div className="workspace-content split-workspace"><div className="sub-list"><div className="panel-heading"><div><span className="label">镜头</span><strong>{shots.length} 个</strong></div></div>{shots.map((shot, index) => <button className={`sub-list-row ${selectedShot.id === shot.id ? "selected" : ""}`} key={shot.id} onClick={() => selection.select({ objectType: "shot", objectId: shot.id, selectionScope: `shot:${shot.id}`, writeScope: `shot:${shot.id}` })}><span>#{String(index + 1).padStart(2, "0")}</span><strong>{shot.title}</strong><small>{shot.composition || shot.action || "尚无画面描述"}</small></button>)}</div><div className="editor-area"><div className="workspace-heading"><div><p className="eyebrow">KEYFRAMES</p><h2>{selectedShot.title}</h2><p>{selectedShot.composition || "先根据镜头与资产规划画面。"}</p></div><button className="secondary" onClick={() => void add()}>＋ 关键帧需求</button></div><div className="inherited-assets"><span className="label">继承镜头资产</span><strong>{inheritedAssets.length ? inheritedAssets.map((asset) => asset.name).join(" · ") : "尚未关联正式资产"}</strong></div><div className="keyframe-tabs">{keyframes.map((frame) => <button className={selectedFrame?.id === frame.id ? "active" : ""} key={frame.id} onClick={() => selection.select({ objectType: "keyframe", objectId: frame.id, selectionScope: `keyframe:${frame.id}`, writeScope: `keyframe:${frame.id}` })}>{keyframeTypeLabel(frame.type)} · {frame.status === "ready" ? "已就绪" : "规划中"}</button>)}</div>{selectedFrame ? <KeyframeEditor project={project} frame={selectedFrame} onMutate={onMutate} onError={onError} /> : <WorkspaceEmpty title="还没有关键帧需求" text="关键帧可以先只有描述和提示词，之后再导入图片。" action="新建需求" onAction={() => void add()} />}</div></div>
  );
}

function KeyframeEditor({ project, frame, onMutate, onError }: { project: ProjectDescriptor; frame: KeyframeRow; onMutate: Props["onMutate"]; onError: Props["onError"] }) {
  const selection = useSelectionStore();
  const patch = (field: string, value: unknown) => onMutate({ action: "patch", entityType: "keyframe", objectId: frame.id, values: { [field]: value }, changeSetName: "编辑关键帧" }).then(() => undefined);
  const importFrame = async () => {
    const selected = await open({ multiple: false, filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp"] }] }); if (typeof selected !== "string") return;
    try { const relative = await api.importProjectFile(project.path, selected, "keyframe"); await onMutate({ action: "patch", entityType: "keyframe", objectId: frame.id, values: { file_path: relative, status: "ready" }, changeSetName: "导入并启用关键帧" }); } catch (error) { await api.cleanupProjectMedia(project.path).catch(() => undefined); onError(error); }
  };
  return <div className="editor-card"><div className="field-grid"><SelectField label="类型" value={frame.type} options={[["single", "单帧"], ["start", "起始帧"], ["middle", "中间帧"], ["end", "结束帧"]]} onFocus={() => selectField("keyframe", frame.id, "type", selection.select)} onSave={(v) => patch("type", v)} /><TextField label="画面描述" value={frame.description} multiline onFocus={() => selectField("keyframe", frame.id, "description", selection.select)} onSave={(v) => patch("description", v)} /><TextField label="专业提示词草稿" value={frame.prompt_draft} multiline onFocus={() => selectField("keyframe", frame.id, "prompt_draft", selection.select)} onSave={(v) => patch("prompt_draft", v)} /></div><div className="keyframe-media">{frame.file_path ? <MediaImage projectPath={project.path} relativePath={frame.file_path} alt="关键帧" /> : <div className="image-placeholder">尚未导入关键帧图片</div>}<div className="heading-actions"><button className="ghost" disabled title="第一阶段不接真实生图 API">生成图片</button><button className="secondary" onClick={() => void importFrame()}>导入关键帧</button><button className="danger-text" onClick={() => void onMutate({ action: "delete", entityType: "keyframe", objectId: frame.id, changeSetName: "删除关键帧" })}>删除</button></div></div></div>;
}

function GenerationWorkspace({ state, currentUnit, onMutate, onMutateBatch }: { state: ProjectState; currentUnit: ContentUnitRow | null; onMutate: Props["onMutate"]; onMutateBatch: Props["onMutateBatch"] }) {
  const selection = useSelectionStore();
  const shots = orderedShotsForUnit(state, currentUnit?.id ?? null);
  const tasks = state.generationTasks.filter((task) => task.content_unit_id === currentUnit?.id);
  const selectedTask = tasks.find((task) => task.id === selection.objectId) ?? tasks[0];
  const selectedShots = shots.filter((shot) => selection.selectedIds.includes(shot.id));
  const create = async () => {
    if (!currentUnit || selectedShots.length === 0) return;
    const name = window.prompt("生成任务名称", `生成任务 ${String(tasks.length + 1).padStart(2, "0")}`)?.trim(); if (!name) return;
    const taskId = crypto.randomUUID();
    await onMutateBatch({
      changeSetName: "创建生成任务",
      mutations: [
        { action: "create", entityType: "generationTask", objectId: taskId, values: { content_unit_id: currentUnit.id, name, duration: 0, status: "draft" } },
        ...selectedShots.map((shot, index) => ({ action: "create" as const, entityType: "generationTaskShot", values: { generation_task_id: taskId, shot_id: shot.id, sort_order: index } })),
      ],
    });
    selection.select({ objectType: "generationTask", objectId: taskId, selectedIds: [], selectionScope: `generationTask:${taskId}`, writeScope: `generationTask:${taskId}` });
  };
  return <div className="workspace-content"><div className="workspace-heading"><div><p className="eyebrow">GENERATION TASKS</p><h2>生成任务</h2><p>镜头是创作单位，生成任务是用户决定的镜头集合。</p></div><button className="primary" disabled={!selectedShots.length} onClick={() => void create()}>用已选 {selectedShots.length} 个镜头创建任务</button></div><div className="generation-layout"><div className="shot-picker"><span className="label">完整镜头表</span>{shots.map((shot, index) => <label className="picker-row" key={shot.id}><input type="checkbox" checked={selection.selectedIds.includes(shot.id)} onChange={(event) => selection.select({ selectedIds: event.target.checked ? [...selection.selectedIds, shot.id] : selection.selectedIds.filter((id) => id !== shot.id) })} /><span>#{String(index + 1).padStart(2, "0")}</span><strong>{shot.title}</strong><small>{shot.duration}s</small></label>)}</div><div className="task-list"><span className="label">任务</span>{tasks.map((task) => <button className={selectedTask?.id === task.id ? "active" : ""} key={task.id} onClick={() => selection.select({ objectType: "generationTask", objectId: task.id, selectionScope: `generationTask:${task.id}`, writeScope: `generationTask:${task.id}` })}><strong>{task.name}</strong><small>{task.duration}s · {state.generationTaskShots.filter((link) => link.generation_task_id === task.id).length} 镜头</small></button>)}</div></div>{selectedTask ? <GenerationTaskEditor task={selectedTask} state={state} onMutate={onMutate} onMutateBatch={onMutateBatch} /> : <WorkspaceEmpty title="还没有生成任务" text="从左侧镜头表多选镜头，然后创建任务。" />}</div>;
}

function GenerationTaskEditor({ task, state, onMutate, onMutateBatch }: { task: GenerationTaskRow; state: ProjectState; onMutate: Props["onMutate"]; onMutateBatch: Props["onMutateBatch"] }) {
  const selection = useSelectionStore();
  const links = state.generationTaskShots.filter((link) => link.generation_task_id === task.id).sort((a, b) => a.sort_order - b.sort_order);
  const linkedShots = links.map((link) => state.shots.find((shot) => shot.id === link.shot_id)).filter((shot): shot is ShotRow => Boolean(shot));
  const patch = (field: string, value: unknown) => onMutate({ action: "patch", entityType: "generationTask", objectId: task.id, values: { [field]: value }, changeSetName: "编辑生成任务" }).then(() => undefined);
  const move = async (index: number, direction: -1 | 1) => { const target = index + direction; if (target < 0 || target >= links.length) return; await onMutateBatch({ changeSetName: "调整任务镜头顺序", mutations: [{ action: "move", entityType: "generationTaskShot", objectId: `${links[index].generation_task_id}|${links[index].shot_id}`, values: { sort_order: target } }, { action: "move", entityType: "generationTaskShot", objectId: `${links[target].generation_task_id}|${links[target].shot_id}`, values: { sort_order: index } }] }); };
  const addSelected = async () => {
    const linkedIds = new Set(links.map((link) => link.shot_id));
    const additions = selection.selectedIds.filter((id) => !linkedIds.has(id) && state.shots.some((shot) => shot.id === id));
    if (!additions.length) return;
    await onMutateBatch({ changeSetName: "向生成任务添加镜头", mutations: additions.map((shotId, offset) => ({ action: "create", entityType: "generationTaskShot", values: { generation_task_id: task.id, shot_id: shotId, sort_order: links.length + offset } })) });
    selection.select({ selectedIds: [] });
  };
  const replaceWithSelected = async () => {
    const selected = selection.selectedIds.filter((id) => state.shots.some((shot) => shot.id === id));
    if (!selected.length) return;
    await onMutateBatch({ changeSetName: "重新组合生成任务", mutations: [...links.map((link) => ({ action: "delete" as const, entityType: "generationTaskShot", objectId: `${link.generation_task_id}|${link.shot_id}` })), ...selected.map((shotId, index) => ({ action: "create" as const, entityType: "generationTaskShot", values: { generation_task_id: task.id, shot_id: shotId, sort_order: index } }))] });
    selection.select({ selectedIds: [] });
  };
  const deleteTask = async () => {
    await onMutateBatch({ changeSetName: "删除生成任务", mutations: [...links.map((link) => ({ action: "delete" as const, entityType: "generationTaskShot", objectId: `${link.generation_task_id}|${link.shot_id}` })), { action: "delete", entityType: "generationTask", objectId: task.id }] });
    selection.select({ objectType: null, objectId: null, selectionScope: null, writeScope: null });
  };
  return <div className="editor-card"><div className="workspace-heading"><div><p className="eyebrow">TASK</p><h3>{task.name}</h3></div><div className="heading-actions"><button className="ghost" disabled={!selection.selectedIds.length} onClick={() => void addSelected()}>添加已选</button><button className="ghost" disabled={!selection.selectedIds.length} onClick={() => void replaceWithSelected()}>用已选替换</button><button className="danger-text" onClick={() => { if (window.confirm("删除生成任务？任务镜头关系将一起删除。")) void deleteTask(); }}>删除任务</button></div></div><div className="field-grid"><TextField label="任务名称" value={task.name} onFocus={() => selectField("generationTask", task.id, "name", selection.select)} onSave={(v) => patch("name", v)} /><TextField label="目标视频模型" value={task.target_model} onFocus={() => selectField("generationTask", task.id, "target_model", selection.select)} onSave={(v) => patch("target_model", v)} /><TextField label="成品提示词" value={task.prompt} multiline onFocus={() => selectField("generationTask", task.id, "prompt", selection.select)} onSave={(v) => patch("prompt", v)} /></div><div className="linked-shots"><span className="label">镜头顺序 · 总时长 {task.duration}s</span>{linkedShots.map((shot, index) => <div className="linked-shot" key={shot.id}><span>{index + 1}</span><strong>{shot.title}</strong><small>{shot.duration}s</small><button className="ghost" disabled={index === 0} onClick={() => void move(index, -1)}>↑</button><button className="ghost" disabled={index === linkedShots.length - 1} onClick={() => void move(index, 1)}>↓</button><button className="danger-text" onClick={() => void onMutateBatch({ changeSetName: "移出任务镜头", mutations: [{ action: "delete", entityType: "generationTaskShot", objectId: `${task.id}|${shot.id}` }] })}>移出</button></div>)}</div></div>;
}

function HistoryWorkspace({ project, state, onRefresh, onError, onCloseChangeSet }: { project: ProjectDescriptor; state: ProjectState; onRefresh: () => Promise<void>; onError: Props["onError"]; onCloseChangeSet: Props["onCloseChangeSet"] }) {
  const [snapshotName, setSnapshotName] = useState("");
  const changeCount = (id: string) => state.changes.filter((change) => change.change_set_id === id).length;
  const create = async () => { if (!snapshotName.trim()) return; try { await api.createSnapshot(project.path, snapshotName, "用户手动创建"); setSnapshotName(""); await onRefresh(); } catch (error) { onError(error); } };
  const cleanup = async () => { if (!window.confirm("清理数据库未引用的资产和关键帧文件？此操作不可撤销。")) return; try { const count = await api.cleanupProjectMedia(project.path); window.alert(`已清理 ${count} 个孤立媒体文件。`); } catch (error) { onError(error); } };
  return <div className="workspace-content"><div className="workspace-heading"><div><p className="eyebrow">HISTORY</p><h2>变更与快照</h2><p>每一次正式写入都有 revision 和原子变更记录。</p></div><button className="ghost" onClick={() => void cleanup()}>清理孤立媒体</button></div><section className="snapshot-create"><input value={snapshotName} onChange={(event) => setSnapshotName(event.target.value)} placeholder="快照名称，例如：第一版完整分镜" /><button className="primary" disabled={!snapshotName.trim()} onClick={() => void create()}>创建快照</button></section><div className="history-columns"><section><div className="section-heading inline"><div><span className="label">变更集</span><h3>{state.changeSets.length} 轮</h3></div></div><div className="history-list">{[...state.changeSets].reverse().map((set) => <div className="history-row" key={set.id}><div><strong>{set.name}</strong><small>{formatDateTime(set.created_at)} · {changeCount(set.id)} 项</small></div><span className={`history-status ${set.status}`}>{set.status === "undone" ? "已撤销" : "已记录"}</span>{set.status !== "undone" && set.source_type !== "snapshot" && <button className="ghost" onClick={async () => { try { onCloseChangeSet(); await api.undoChangeSet(project.path, set.id); await onRefresh(); } catch (error) { onError(error); } }}>撤销</button>}</div>)}</div></section><section><div className="section-heading inline"><div><span className="label">快照</span><h3>{state.snapshots.length} 个</h3></div></div><div className="history-list">{[...state.snapshots].reverse().map((snapshot) => <div className="history-row" key={snapshot.id}><div><strong>{snapshot.name}</strong><small>修订 {snapshot.revision} · {formatDateTime(snapshot.created_at)}</small></div><button className="ghost" onClick={() => window.alert(snapshotSummary(snapshot.snapshot_json, snapshot.name))}>查看</button><button className="secondary" onClick={async () => { if (!window.confirm(`恢复快照“${snapshot.name}”？当前业务数据会替换为快照版本。`)) return; try { onCloseChangeSet(); await api.restoreSnapshot(project.path, snapshot.id); await onRefresh(); } catch (error) { onError(error); } }}>恢复</button></div>)}</div></section></div></div>;
}

function MediaImage({ projectPath, relativePath, alt }: { projectPath: string; relativePath: string; alt: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => { let active = true; void api.readProjectMedia(projectPath, relativePath).then((result) => { if (active) setSrc(`data:${result.mimeType};base64,${result.data}`); }).catch(() => { if (active) setSrc(null); }); return () => { active = false; }; }, [projectPath, relativePath]);
  return src ? <img src={src} alt={alt} /> : <div className="image-placeholder">无法预览</div>;
}

function contentPath(units: ContentUnitRow[], id: string | null) {
  const result: ContentUnitRow[] = [];
  let current = units.find((unit) => unit.id === id);
  while (current) { result.unshift(current); current = units.find((unit) => unit.id === current?.parent_id); }
  return result;
}

function chooseShot(shots: ShotRow[], promptText: string): ShotRow | null {
  if (!shots.length) return null;
  const answer = window.prompt(`${promptText}\n${shots.map((shot, index) => `${index + 1}. ${shot.title}`).join("\n")}`, "")?.trim();
  if (!answer) return null;
  const index = Number(answer) - 1;
  return Number.isInteger(index) ? shots[index] ?? null : null;
}

function chooseRequirement(requirements: AssetRequirementRow[], promptText: string): AssetRequirementRow | null {
  if (!requirements.length) return null;
  const answer = window.prompt(`${promptText}\n${requirements.map((item, index) => `${index + 1}. ${item.requirement_type}`).join("\n")}`, "")?.trim();
  if (!answer) return null;
  const index = Number(answer) - 1;
  return Number.isInteger(index) ? requirements[index] ?? null : null;
}

function selectField(objectType: string, objectId: string, field: string, select: ReturnType<typeof useSelectionStore.getState>["select"]) {
  select({ objectType, objectId, field, selectionScope: `${objectType}:${objectId}`, writeScope: `${objectType}:${objectId}.${field}` });
}

function unitIcon(type: string) { return ({ season: "S", episode: "E", short: "短", act: "幕", custom: "·", character: "角", location: "景", prop: "道" } as Record<string, string>)[type] ?? "·"; }
function assetTypeLabel(type: AssetRow["type"]) { return ({ character: "角色", location: "场景", prop: "道具" })[type]; }
function maturityLabel(value: string) { return ({ exploring: "探索中", usable: "可用", stable: "稳定" } as Record<string, string>)[value] ?? value; }
function keyframeTypeLabel(value: string) { return ({ single: "单帧", start: "起始帧", middle: "中间帧", end: "结束帧" } as Record<string, string>)[value] ?? value; }
function formatDateTime(value: string) { const date = new Date(value); return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }).format(date); }
function snapshotSummary(raw: string, name: string) { try { const data = JSON.parse(raw) as Record<string, unknown[]>; const lines = Object.entries(data).filter(([table]) => table !== "projects").map(([table, rows]) => `${table}: ${Array.isArray(rows) ? rows.length : 0}`); return `${name}\n\n${lines.join("\n")}`; } catch { return `${name}\n\n快照内容无法解析。`; } }
