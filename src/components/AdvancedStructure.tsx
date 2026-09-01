import { AlertTriangle, GitBranch, ListTree, Plus, RotateCcw, Save, Table2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import {
  buildStructureGraph,
  detectStructureIssues,
  occurrenceOptions,
  storyElementLabels,
} from "../domain/storyStructure";
import type {
  BatchMutationRequest,
  BatchMutationResponse,
  ContentUnitRow,
  MutationRequest,
  MutationResponse,
  ProjectDescriptor,
  ProjectState,
  StoryElementRow,
  StoryElementType,
} from "../types";
import { useSelectionStore } from "../stores/selectionStore";
import { useAppDialog } from "./AppDialog";

type ViewType = "timeline" | "graph" | "episodes";
type FocusMode = "all" | "character" | "foreshadow" | "unreturned" | "affected" | "inconsistent";

interface Props {
  project: ProjectDescriptor;
  state: ProjectState;
  currentUnit: ContentUnitRow | null;
  onMutate: (request: MutationRequest) => Promise<MutationResponse>;
  onMutateBatch: (request: BatchMutationRequest) => Promise<BatchMutationResponse>;
}

const elementTypes = Object.entries(storyElementLabels) as Array<[StoryElementType, string]>;

export function AdvancedStructure({ project, state, currentUnit, onMutate, onMutateBatch }: Props) {
  const selection = useSelectionStore();
  const dialog = useAppDialog();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [view, setView] = useState<ViewType>("timeline");
  const [focus, setFocus] = useState<FocusMode>("all");
  const [selectedElementId, setSelectedElementId] = useState<string | null>(null);
  const scopeId = currentUnit?.id ?? null;
  const graph = useMemo(() => buildStructureGraph(state, scopeId), [state, scopeId]);
  const issues = useMemo(() => detectStructureIssues(state, scopeId), [state, scopeId]);
  const unpaidIds = new Set(issues.filter((issue) => issue.id.startsWith("unpaid:")).map((issue) => issue.elementId));
  const issueUnitIds = new Set(issues.map((issue) => issue.contentUnitId).filter(Boolean));

  useEffect(() => {
    void api.getFeatureFlags().then((flags) => setEnabled(flags.story_graph)).catch(() => setEnabled(false));
  }, []);

  useEffect(() => {
    const stored = state.graphLayouts.find((layout) => layout.scope_type === (scopeId ? "contentUnit" : "project") && layout.scope_id === (scopeId ?? project.id) && layout.view_type === view);
    if (!stored) return;
    try {
      const parsed = JSON.parse(stored.filter_json) as { focus?: FocusMode; selectedElementId?: string | null };
      if (parsed.focus) setFocus(parsed.focus);
      setSelectedElementId(parsed.selectedElementId ?? null);
    } catch {
      // Invalid legacy layout should not block the creative facts.
    }
  }, [scopeId, project.id, state.graphLayouts, view]);

  const visibleElements = graph.elements.filter((element) => {
    if (selectedElementId && element.id !== selectedElementId) return false;
    if (focus === "character" && element.type !== "character_arc") return false;
    if (focus === "foreshadow" && element.type !== "foreshadow") return false;
    if (focus === "unreturned" && !unpaidIds.has(element.id)) return false;
    if (focus === "affected") {
      return graph.occurrences.some((item) => item.story_element_id === element.id && graph.units.find((unit) => unit.id === item.content_unit_id)?.sync_status === "affected");
    }
    if (focus === "inconsistent" && !issues.some((issue) => issue.elementId === element.id || (issue.contentUnitId && graph.occurrences.some((item) => item.story_element_id === element.id && item.content_unit_id === issue.contentUnitId)))) return false;
    return true;
  });
  const visibleUnits = focus === "affected"
    ? graph.units.filter((unit) => unit.sync_status === "affected")
    : focus === "inconsistent" ? graph.units.filter((unit) => issueUnitIds.has(unit.id)) : graph.units;

  const enable = async () => {
    const flags = await api.setFeatureFlag("story_graph", true);
    setEnabled(flags.story_graph);
  };

  const saveLayout = async () => {
    await api.saveGraphLayout(project.path, {
      scopeType: scopeId ? "contentUnit" : "project",
      scopeId: scopeId ?? project.id,
      viewType: view,
      filterJson: JSON.stringify({ focus, selectedElementId }),
      layoutJson: JSON.stringify({ version: 1 }),
    });
  };

  const resetLayout = async () => {
    await api.resetGraphLayout(project.path, scopeId ? "contentUnit" : "project", scopeId ?? project.id, view);
    setFocus("all");
    setSelectedElementId(null);
  };

  const createElement = async () => {
    const type = await dialog.prompt("新建故事元素", { label: "元素类型", options: elementTypes.map(([value, label]) => ({ value, label })) }) as StoryElementType | null;
    if (!type || !storyElementLabels[type]) return;
    const name = await dialog.prompt("命名故事元素", { label: "名称", defaultValue: type === "foreshadow" ? "新伏笔" : "新故事线" });
    if (!name) return;
    const elementId = crypto.randomUUID();
    const firstUnit = graph.units[0];
    const addFirst = Boolean(firstUnit && await dialog.confirm(`是否同时在“${firstUnit.name}”建立第一个节点？`, { title: "建立起始节点", confirmLabel: "同时建立" }));
    const mutations: MutationRequest[] = [{
      action: "create",
      entityType: "storyElement",
      objectId: elementId,
      values: { project_id: project.id, type, name, scope_unit_id: scopeId, maturity: "exploring", status: "active" },
    }];
    if (addFirst) {
      mutations.push({ action: "create", entityType: "storyElementOccurrence", objectId: crypto.randomUUID(), values: { story_element_id: elementId, content_unit_id: firstUnit.id, occurrence_type: occurrenceOptions[type][0], sort_order: 0 } });
    }
    await onMutateBatch({ mutations, changeSetName: "建立故事元素与节点" });
    chooseElement({ id: elementId, project_id: project.id, type, name } as StoryElementRow);
  };

  const chooseElement = (element: StoryElementRow) => {
    setSelectedElementId((current) => current === element.id ? null : element.id);
    selection.select({ objectType: "storyElement", objectId: element.id, field: null, selectionScope: `storyElement:${element.id}`, writeScope: `storyElement:${element.id}`, selectedIds: [] });
  };

  if (enabled === null) return null;
  if (!enabled) {
    return (
      <section className="advanced-structure-gate">
        <GitBranch size={22} />
        <div><strong>高级作品结构</strong><p>按季或项目查看故事线、伏笔、人物弧光及计划与事实层偏差。</p></div>
        <button className="secondary" onClick={() => void enable()}>启用高级结构</button>
      </section>
    );
  }

  return (
    <section className="advanced-structure">
      <div className="section-heading inline structure-toolbar">
        <div><span className="label">高级作品结构</span><h3>{graph.units.length} 个内容节点 · {graph.elements.length} 条故事线</h3></div>
        <div className="structure-actions">
          <select value={focus} aria-label="聚焦模式" onChange={(event) => setFocus(event.target.value as FocusMode)}>
            <option value="all">全部</option><option value="character">人物线</option><option value="foreshadow">伏笔</option><option value="unreturned">未回收伏笔</option><option value="affected">受影响内容</option><option value="inconsistent">计划 / 事实不一致</option>
          </select>
          <select value={selectedElementId ?? ""} aria-label="聚焦故事元素" onChange={(event) => setSelectedElementId(event.target.value || null)}>
            <option value="">全部故事元素</option>{graph.elements.map((element) => <option key={element.id} value={element.id}>{element.name}</option>)}
          </select>
          <button className="ghost" title="保存当前筛选" onClick={() => void saveLayout()}><Save size={13} />保存视图</button>
          <button className="ghost" title="重置当前视图" onClick={() => void resetLayout()}><RotateCcw size={13} />重置</button>
          <button className="secondary" onClick={() => void createElement()}><Plus size={14} />故事元素</button>
        </div>
      </div>

      <div className="structure-view-tabs">
        <button className={view === "timeline" ? "active" : ""} onClick={() => setView("timeline")}><ListTree size={14} />时间轴</button>
        <button className={view === "graph" ? "active" : ""} onClick={() => setView("graph")}><GitBranch size={14} />关系图</button>
        <button className={view === "episodes" ? "active" : ""} onClick={() => setView("episodes")}><Table2 size={14} />剧集表</button>
      </div>

      {issues.length > 0 && (
        <div className="structure-issues">
          {issues.map((issue) => <article key={issue.id}><AlertTriangle size={15} /><div><strong>{issue.title}</strong><p>{issue.detail}</p></div></article>)}
        </div>
      )}

      {view === "timeline" && <StructureTimeline elements={visibleElements} units={visibleUnits} state={state} onMutate={onMutate} onChoose={chooseElement} />}
      {view === "graph" && <StructureGraphView elements={visibleElements} units={visibleUnits} state={state} relations={graph.relations} truncated={graph.truncated} onChoose={chooseElement} />}
      {view === "episodes" && <EpisodeStructureTable units={visibleUnits} elements={visibleElements} state={state} onMutate={onMutate} />}
    </section>
  );
}

function StructureTimeline({ elements, units, state, onMutate, onChoose }: { elements: StoryElementRow[]; units: ContentUnitRow[]; state: ProjectState; onMutate: Props["onMutate"]; onChoose: (element: StoryElementRow) => void }) {
  const dialog = useAppDialog();
  const addOccurrence = async (element: StoryElementRow, unit: ContentUnitRow) => {
    const options = occurrenceOptions[element.type];
    const occurrenceType = await dialog.prompt("添加故事节点", { label: "节点语义", options: options.map((value) => ({ value, label: value })) });
    if (!occurrenceType || !options.includes(occurrenceType)) return;
    const description = await dialog.prompt("补充节点说明", { label: "说明", optional: true, multiline: true, placeholder: "可留空" }) ?? "";
    const count = state.storyElementOccurrences.filter((item) => item.story_element_id === element.id).length;
    await onMutate({ action: "create", entityType: "storyElementOccurrence", values: { story_element_id: element.id, content_unit_id: unit.id, occurrence_type: occurrenceType, description, sort_order: count }, changeSetName: "添加故事节点" });
  };
  return (
    <div className="structure-matrix-wrap">
      <table className="structure-matrix"><thead><tr><th>故事元素</th>{units.map((unit) => <th key={unit.id}>{unit.name}</th>)}</tr></thead><tbody>
        {elements.map((element) => <tr key={element.id}><th><button onClick={() => onChoose(element)}><small>{storyElementLabels[element.type]}</small><strong>{element.name}</strong></button></th>{units.map((unit) => {
          const items = state.storyElementOccurrences.filter((item) => item.story_element_id === element.id && item.content_unit_id === unit.id);
          return <td key={unit.id}>{items.map((item) => <span className="occurrence-chip" title={item.description} key={item.id}>{item.occurrence_type}</span>)}<button className="matrix-add" title="添加节点" onClick={() => void addOccurrence(element, unit)}>＋</button></td>;
        })}</tr>)}
      </tbody></table>
      {elements.length === 0 && <div className="panel-empty">当前筛选下没有故事元素。</div>}
    </div>
  );
}

function StructureGraphView({ elements, units, state, relations, truncated, onChoose }: { elements: StoryElementRow[]; units: ContentUnitRow[]; state: ProjectState; relations: ProjectState["relations"]; truncated: boolean; onChoose: (element: StoryElementRow) => void }) {
  const width = Math.max(760, units.length * 115);
  const height = Math.max(260, 100 + elements.length * 72);
  const xForUnit = (index: number) => 90 + index * ((width - 180) / Math.max(1, units.length - 1));
  const yForElement = (index: number) => 105 + index * 65;
  const visibleElementIds = new Set(elements.map((element) => element.id));
  const visibleUnitIds = new Set(units.map((unit) => unit.id));
  const edges = state.storyElementOccurrences.filter((item) => visibleElementIds.has(item.story_element_id) && visibleUnitIds.has(item.content_unit_id)).slice(0, 200);
  const pointFor = (id: string) => {
    const unitIndex = units.findIndex((item) => item.id === id);
    if (unitIndex >= 0) return { x: xForUnit(unitIndex), y: 42 };
    const elementIndex = elements.findIndex((item) => item.id === id);
    return elementIndex >= 0 ? { x: 170, y: yForElement(elementIndex) } : null;
  };
  return (
    <div className="story-graph-wrap"><svg className="story-graph" width={width} height={height} role="img" aria-label="当前范围故事关系图">
      {edges.map((edge) => {
        const elementIndex = elements.findIndex((item) => item.id === edge.story_element_id);
        const unitIndex = units.findIndex((item) => item.id === edge.content_unit_id);
        return <line key={edge.id} x1={170} y1={yForElement(elementIndex)} x2={xForUnit(unitIndex)} y2={42} />;
      })}
      {relations.slice(0, Math.max(0, 200 - edges.length)).map((relation) => {
        const source = pointFor(relation.source_id);
        const target = pointFor(relation.target_id);
        return source && target ? <line className="formal-relation" key={relation.id} x1={source.x} y1={source.y} x2={target.x} y2={target.y}><title>{relation.relation_type}</title></line> : null;
      })}
      {units.map((unit, index) => <g key={unit.id} transform={`translate(${xForUnit(index)},42)`}><circle r="17" /><text y="4" textAnchor="middle">{index + 1}</text><text className="graph-label" y="31" textAnchor="middle">{unit.name}</text></g>)}
      {elements.map((element, index) => <g className="element-node" key={element.id} transform={`translate(170,${yForElement(index)})`} onClick={() => onChoose(element)}><rect x="-94" y="-19" width="188" height="38" rx="8" /><text x="-82" y="-4">{storyElementLabels[element.type]}</text><text className="graph-label" x="-82" y="11">{element.name}</text></g>)}
    </svg><small>正式关系 {relations.length} 条；节点连线按当前范围按需绘制{edges.length + relations.length >= 200 ? "（画布仅绘制前 200 条）" : ""}{truncated ? "，关系数据已限制到 1000 条" : ""}。</small></div>
  );
}

function EpisodeStructureTable({ units, elements, state, onMutate }: { units: ContentUnitRow[]; elements: StoryElementRow[]; state: ProjectState; onMutate: Props["onMutate"] }) {
  const dialog = useAppDialog();
  const occurrenceText = (unitId: string, type: StoryElementType) => state.storyElementOccurrences
    .filter((item) => item.content_unit_id === unitId && elements.find((element) => element.id === item.story_element_id)?.type === type)
    .map((item) => `${elements.find((element) => element.id === item.story_element_id)?.name}·${item.occurrence_type}`).join("；");
  const addProgress = async (unit: ContentUnitRow, type: StoryElementType) => {
    const candidates = elements.filter((element) => element.type === type);
    if (!candidates.length) return;
    const elementId = await dialog.prompt(`选择${storyElementLabels[type]}`, { label: "故事元素", options: candidates.map((element) => ({ value: element.id, label: element.name })) });
    const element = candidates.find((item) => item.id === elementId);
    if (!element) return;
    const options = occurrenceOptions[type];
    const occurrenceType = await dialog.prompt("更新剧集故事进度", { label: "节点语义", options: options.map((value) => ({ value, label: value })) });
    if (!occurrenceType || !options.includes(occurrenceType)) return;
    await onMutate({ action: "create", entityType: "storyElementOccurrence", values: { story_element_id: element.id, content_unit_id: unit.id, occurrence_type: occurrenceType, sort_order: state.storyElementOccurrences.filter((item) => item.story_element_id === element.id).length }, changeSetName: "更新剧集故事进度" });
  };
  return (
    <div className="episode-structure-table-wrap"><table className="episode-structure-table"><thead><tr><th>剧集</th><th>一句话剧情</th><th>主线进度</th><th>人物进度</th><th>伏笔</th><th>成熟度</th><th>同步</th></tr></thead><tbody>
      {units.map((unit) => <tr key={unit.id}><th>{unit.name}</th><td><input defaultValue={unit.summary} aria-label={`${unit.name} 一句话剧情`} onBlur={(event) => event.target.value !== unit.summary && void onMutate({ action: "patch", entityType: "contentUnit", objectId: unit.id, values: { summary: event.target.value }, changeSetName: "更新剧集一句话剧情" })} /></td><td>{occurrenceText(unit.id, "mainline") || "—"}<button className="table-add" onClick={() => void addProgress(unit, "mainline")}>＋</button></td><td>{occurrenceText(unit.id, "character_arc") || "—"}<button className="table-add" onClick={() => void addProgress(unit, "character_arc")}>＋</button></td><td>{occurrenceText(unit.id, "foreshadow") || "—"}<button className="table-add" onClick={() => void addProgress(unit, "foreshadow")}>＋</button></td><td><select value={unit.maturity} onChange={(event) => void onMutate({ action: "patch", entityType: "contentUnit", objectId: unit.id, values: { maturity: event.target.value }, changeSetName: "更新剧集成熟度" })}><option value="exploring">探索中</option><option value="usable">可用</option><option value="stable">稳定</option></select></td><td><select value={unit.sync_status} onChange={(event) => void onMutate({ action: "patch", entityType: "contentUnit", objectId: unit.id, values: { sync_status: event.target.value }, changeSetName: "更新剧集同步状态" })}><option value="normal">正常</option><option value="needs_review">待调整</option><option value="affected">受影响</option></select></td></tr>)}
    </tbody></table></div>
  );
}
