import type {
  ContentUnitRow,
  ProjectState,
  RelationRow,
  StoryElementOccurrenceRow,
  StoryElementRow,
  StoryElementType,
} from "../types";

export const storyElementLabels: Record<StoryElementType, string> = {
  mainline: "主线",
  character_arc: "人物弧光",
  foreshadow: "伏笔",
  event: "事件",
  theme: "主题",
  custom: "自定义",
};

export const occurrenceOptions: Record<StoryElementType, string[]> = {
  foreshadow: ["埋下", "强化", "误导", "部分揭示", "回收"],
  character_arc: ["建立", "推进", "受挫", "转折", "修复", "完成"],
  mainline: ["推进", "阻碍", "揭示", "转折", "高潮"],
  event: ["发生", "升级", "转折", "结束"],
  theme: ["提出", "对照", "深化", "回应"],
  custom: ["出现", "推进", "变化", "完成"],
};

export interface StructureIssue {
  id: string;
  contentUnitId: string | null;
  elementId: string | null;
  title: string;
  detail: string;
}

export interface StructureGraph {
  units: ContentUnitRow[];
  elements: StoryElementRow[];
  occurrences: StoryElementOccurrenceRow[];
  relations: RelationRow[];
  truncated: boolean;
}

function descendantIds(units: ContentUnitRow[], scopeId: string | null): Set<string> {
  if (!scopeId) return new Set(units.map((unit) => unit.id));
  const children = new Map<string, string[]>();
  for (const unit of units) {
    if (!unit.parent_id) continue;
    children.set(unit.parent_id, [...(children.get(unit.parent_id) ?? []), unit.id]);
  }
  const result = new Set([scopeId]);
  let frontier = [scopeId];
  while (frontier.length) {
    const next = frontier.flatMap((id) => children.get(id) ?? []);
    next.forEach((id) => result.add(id));
    frontier = next;
  }
  return result;
}

export function episodesForScope(units: ContentUnitRow[], scopeId: string | null): ContentUnitRow[] {
  const allowed = descendantIds(units, scopeId);
  const byId = new Map(units.map((unit) => [unit.id, unit]));
  const path = (unit: ContentUnitRow) => {
    const result: Array<[number, string]> = [];
    let current: ContentUnitRow | undefined = unit;
    const seen = new Set<string>();
    while (current && !seen.has(current.id)) {
      seen.add(current.id);
      result.unshift([current.sort_order, current.name]);
      current = current.parent_id ? byId.get(current.parent_id) : undefined;
    }
    return result;
  };
  const comparePath = (a: Array<[number, string]>, b: Array<[number, string]>) => {
    for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
      if (!a[index]) return -1;
      if (!b[index]) return 1;
      const order = a[index][0] - b[index][0];
      if (order) return order;
      const name = a[index][1].localeCompare(b[index][1], "zh-CN");
      if (name) return name;
    }
    return 0;
  };
  return units
    .filter((unit) => allowed.has(unit.id) && ["episode", "short"].includes(unit.type))
    .map((unit) => ({ unit, path: path(unit) }))
    .sort((left, right) => comparePath(left.path, right.path) || left.unit.id.localeCompare(right.unit.id))
    .map(({ unit }) => unit);
}

export function elementsForScope(state: ProjectState, scopeId: string | null): StoryElementRow[] {
  const allowed = descendantIds(state.contentUnits, scopeId);
  let ancestorId = scopeId ? state.contentUnits.find((unit) => unit.id === scopeId)?.parent_id ?? null : null;
  while (ancestorId) {
    allowed.add(ancestorId);
    ancestorId = state.contentUnits.find((unit) => unit.id === ancestorId)?.parent_id ?? null;
  }
  return state.storyElements.filter((element) =>
    element.status === "active" && (!element.scope_unit_id || allowed.has(element.scope_unit_id)),
  );
}

export function textOverlap(left: string, right: string): number {
  const normalize = (value: string) => value.toLocaleLowerCase().replace(/[\s，。！？、,.!?；;：:（）()“”"']/g, "");
  const a = normalize(left);
  const b = normalize(right);
  if (!a || !b) return 1;
  const grams = (value: string) => new Set(Array.from({ length: Math.max(1, value.length - 1) }, (_, index) => value.slice(index, index + 2)));
  const aGrams = grams(a);
  const bGrams = grams(b);
  const common = [...aGrams].filter((value) => bGrams.has(value)).length;
  return common / (aGrams.size + bGrams.size - common);
}

export function detectStructureIssues(state: ProjectState, scopeId: string | null): StructureIssue[] {
  const episodes = episodesForScope(state.contentUnits, scopeId);
  const episodeIds = new Set(episodes.map((episode) => episode.id));
  const elements = elementsForScope(state, scopeId);
  const elementIds = new Set(elements.map((element) => element.id));
  const occurrences = state.storyElementOccurrences.filter((item) => elementIds.has(item.story_element_id) && episodeIds.has(item.content_unit_id));
  const occurrencesByUnit = new Map<string, StoryElementOccurrenceRow[]>();
  const occurrencesByElement = new Map<string, StoryElementOccurrenceRow[]>();
  for (const occurrence of occurrences) {
    occurrencesByUnit.set(occurrence.content_unit_id, [...(occurrencesByUnit.get(occurrence.content_unit_id) ?? []), occurrence]);
    occurrencesByElement.set(occurrence.story_element_id, [...(occurrencesByElement.get(occurrence.story_element_id) ?? []), occurrence]);
  }
  const textByUnit = new Map<string, string[]>();
  const scriptUnit = new Map<string, string>();
  const sceneUnit = new Map<string, string>();
  const addText = (unitId: string | undefined, values: string[]) => {
    if (!unitId) return;
    const text = values.filter((value) => value.trim());
    if (text.length) textByUnit.set(unitId, [...(textByUnit.get(unitId) ?? []), ...text]);
  };
  for (const script of state.scripts) {
    scriptUnit.set(script.id, script.content_unit_id);
    addText(script.content_unit_id, [script.summary]);
  }
  for (const scene of state.scenes) {
    const unitId = scriptUnit.get(scene.script_id);
    if (unitId) sceneUnit.set(scene.id, unitId);
    addText(unitId, [scene.summary, scene.content]);
  }
  for (const shot of state.shots) {
    addText(sceneUnit.get(shot.scene_id), [shot.narrative_purpose, shot.action, shot.dialogue]);
  }
  const issues: StructureIssue[] = [];

  for (const episode of episodes) {
    const actualText = (textByUnit.get(episode.id) ?? []).join(" ");
    const hasActual = Boolean(actualText);
    const planned = occurrencesByUnit.get(episode.id) ?? [];
    if ((episode.summary.trim() || planned.length) && !hasActual) {
      issues.push({ id: `plan-missing:${episode.id}`, contentUnitId: episode.id, elementId: null, title: `${episode.name} 的计划尚未落到事实层`, detail: "已有一句话剧情或结构节点，但剧本、场和分镜中还没有实际内容。" });
    }
    if (hasActual && !episode.summary.trim()) {
      issues.push({ id: `actual-unplanned:${episode.id}`, contentUnitId: episode.id, elementId: null, title: `${episode.name} 缺少一句话剧情`, detail: "事实层已有内容，但计划层没有对应的一句话剧情。" });
    }
    if (hasActual && episode.summary.trim() && textOverlap(episode.summary, actualText) < 0.2) {
      issues.push({ id: `plot-diverged:${episode.id}`, contentUnitId: episode.id, elementId: null, title: `${episode.name} 的事实剧情可能偏离计划`, detail: "一句话剧情与剧本、场或分镜的文字重合度较低，请人工复核；系统不会自动改写。" });
    }
  }

  for (const element of elements.filter((item) => item.type === "foreshadow")) {
    const chain = occurrencesByElement.get(element.id) ?? [];
    const planted = chain.some((item) => ["埋下", "plant"].includes(item.occurrence_type));
    const paid = chain.some((item) => ["回收", "payoff"].includes(item.occurrence_type));
    if (planted && !paid) {
      issues.push({ id: `unpaid:${element.id}`, contentUnitId: null, elementId: element.id, title: `伏笔“${element.name}”尚未回收`, detail: "当前范围有埋下节点，但没有回收节点。" });
    }
  }
  return issues;
}

export function buildStructureGraph(state: ProjectState, scopeId: string | null, maxRelations = 1000): StructureGraph {
  const units = episodesForScope(state.contentUnits, scopeId);
  const unitIds = new Set(units.map((unit) => unit.id));
  const elements = elementsForScope(state, scopeId);
  const elementIds = new Set(elements.map((element) => element.id));
  const occurrences = state.storyElementOccurrences.filter((item) => elementIds.has(item.story_element_id) && unitIds.has(item.content_unit_id));
  const allowedIds = new Set([...unitIds, ...elementIds]);
  const relevant = state.relations.filter((relation) => allowedIds.has(relation.source_id) && allowedIds.has(relation.target_id));
  return {
    units,
    elements,
    occurrences,
    relations: relevant.slice(0, maxRelations),
    truncated: relevant.length > maxRelations,
  };
}
