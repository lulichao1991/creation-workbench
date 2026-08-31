import { describe, expect, it } from "vitest";
import { buildStructureGraph, detectStructureIssues, elementsForScope, episodesForScope } from "./storyStructure";
import type { ContentUnitRow, ProjectState, RelationRow } from "../types";

const row = (id: string, parent_id: string | null, type: ContentUnitRow["type"], sort_order: number): ContentUnitRow => ({
  id, parent_id, type, sort_order, project_id: "p", name: id, summary: "", maturity: "exploring", sync_status: "normal", created_at: "", updated_at: "",
});

const baseState = (): ProjectState => ({
  projects: [], contentUnits: [], scripts: [], scenes: [], shots: [], assets: [], assetMedia: [], assetRequirements: [], assetRequirementSources: [], assetMediaRequirements: [], shotAssets: [], keyframes: [], generationTasks: [], generationTaskShots: [], relations: [], storyElements: [], storyElementOccurrences: [], graphLayouts: [], projectMemories: [], memorySources: [], changeSets: [], changes: [], snapshots: [],
});

describe("advanced story structure", () => {
  it("limits a season timeline to its ordered 30 episodes", () => {
    const units = [row("season", null, "season", 0), ...Array.from({ length: 30 }, (_, index) => row(`ep-${index}`, "season", "episode", index))];
    expect(episodesForScope(units, "season")).toHaveLength(30);
    expect(episodesForScope(units, "season")[29].id).toBe("ep-29");
  });

  it("inherits a season story element when an episode is the current scope", () => {
    const state = baseState();
    state.contentUnits = [row("season", null, "season", 0), row("ep", "season", "episode", 0)];
    state.storyElements = [{ id: "line", project_id: "p", type: "mainline", name: "季主线", description: "", scope_unit_id: "season", maturity: "usable", status: "active", created_at: "", updated_at: "" }];
    expect(elementsForScope(state, "ep").map((element) => element.id)).toEqual(["line"]);
  });

  it("reports plan-versus-actual gaps and an unreturned foreshadow without changing data", () => {
    const state = baseState();
    state.contentUnits = [{ ...row("ep", null, "episode", 0), summary: "英雄发现信件" }];
    state.storyElements = [{ id: "f", project_id: "p", type: "foreshadow", name: "红色信件", description: "", scope_unit_id: null, maturity: "usable", status: "active", created_at: "", updated_at: "" }];
    state.storyElementOccurrences = [{ id: "o", story_element_id: "f", content_unit_id: "ep", occurrence_type: "埋下", description: "", sort_order: 0, created_at: "", updated_at: "" }];
    const before = JSON.stringify(state);
    const issues = detectStructureIssues(state, null);
    expect(issues.map((issue) => issue.id)).toEqual(["plan-missing:ep", "unpaid:f"]);
    expect(JSON.stringify(state)).toBe(before);
  });

  it("keeps graph relations bounded at 1000", () => {
    const state = baseState();
    state.contentUnits = [row("ep-a", null, "episode", 0), row("ep-b", null, "episode", 1)];
    state.relations = Array.from({ length: 1005 }, (_, index) => ({ id: `r-${index}`, project_id: "p", source_type: "contentUnit", source_id: "ep-a", relation_type: "推进", target_type: "contentUnit", target_id: "ep-b", description: "", importance: 1, status: "active", created_at: "", updated_at: "" } satisfies RelationRow));
    const graph = buildStructureGraph(state, null);
    expect(graph.relations).toHaveLength(1000);
    expect(graph.truncated).toBe(true);
  });

  it("flags a likely plot divergence while leaving the decision to the user", () => {
    const state = baseState();
    state.contentUnits = [{ ...row("ep", null, "episode", 0), summary: "英雄在车站找到失踪的妹妹" }];
    state.scripts = [{ id: "s", content_unit_id: "ep", title: "", summary: "反派在海岛引爆实验室", maturity: "exploring", sync_status: "normal", created_at: "", updated_at: "" }];
    expect(detectStructureIssues(state, null).map((issue) => issue.id)).toContain("plot-diverged:ep");
  });
});
