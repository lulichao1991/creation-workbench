import { describe, expect, it } from "vitest";
import { buildAgentSelection, buildWriteScope } from "./panelState";

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
});
