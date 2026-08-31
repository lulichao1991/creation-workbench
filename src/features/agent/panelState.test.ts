import { describe, expect, it } from "vitest";
import { buildAgentSelection, buildChangeAnalysisSelection, buildWriteScope, canRequestExpertTeam, isExpertTeamRunning } from "./panelState";

describe("Agent panel selection and permission derivation", () => {
  it("uses the exact selected field in edit mode", () => {
    const selection = buildAgentSelection({
      projectId: "project",
      revision: 7,
      objectType: "shot",
      objectId: "shot-04",
      field: "composition",
      selectedIds: [],
      currentUnitId: "ep08",
    });
    expect(selection.center?.field).toBe("composition");
    expect(buildWriteScope(selection, "edit").refs).toEqual([selection.center]);
    expect(buildWriteScope(selection, "discussion").refs).toEqual([]);
  });

  it("protects narrative fields for multi-shot photography edits", () => {
    const selection = buildAgentSelection({
      projectId: "project",
      revision: 8,
      objectType: "shot",
      objectId: "shot-04",
      field: null,
      selectedIds: ["shot-04", "shot-05", "shot-06"],
      currentUnitId: "ep08",
    });
    const scope = buildWriteScope(selection, "edit");
    expect(scope.refs).toContainEqual(expect.objectContaining({ objectId: "shot-05", field: "composition" }));
    expect(scope.protectedRefs).toContainEqual(expect.objectContaining({ objectId: "shot-05", field: "dialogue" }));
    expect(scope.refs.some((reference) => reference.field === "dialogue")).toBe(false);
  });

  it("centers explicit change analysis on the active ChangeSet", () => {
    const selection = buildChangeAnalysisSelection("project", "changeset-13", 13);
    expect(selection).toEqual({
      projectId: "project",
      center: { projectId: "project", objectType: "changeSet", objectId: "changeset-13" },
      selected: [],
      projectRevision: 13,
    });
    expect(buildWriteScope(selection, "suggestion")).toEqual({ refs: [], protectedRefs: [] });
  });

  it("requires two experts and a separate explicit confirmation step", () => {
    const members = new Set(["writer", "director"] as const);
    expect(canRequestExpertTeam("这一场哪里不对？", members, false)).toBe(true);
    expect(canRequestExpertTeam("", members, false)).toBe(false);
    expect(canRequestExpertTeam("检查", new Set(["writer"] as const), false)).toBe(false);
    expect(isExpertTeamRunning("awaiting_confirmation")).toBe(false);
    expect(isExpertTeamRunning("running")).toBe(true);
    expect(isExpertTeamRunning("synthesizing")).toBe(true);
  });
});
