import { beforeEach, describe, expect, it } from "vitest";
import { useSelectionStore } from "./selectionStore";

describe("SelectionStore", () => {
  beforeEach(() => useSelectionStore.getState().clear());

  it("separates selection scope from write scope", () => {
    useSelectionStore.getState().select({
      objectType: "shot",
      objectId: "shot-04",
      field: "composition",
      selectionScope: "shot-04",
      writeScope: "shot-04.composition",
    });

    const state = useSelectionStore.getState();
    expect(state.selectionScope).toBe("shot-04");
    expect(state.writeScope).toBe("shot-04.composition");
  });

  it("tracks unsaved field state independently from the selected object", () => {
    useSelectionStore.getState().select({ objectType: "scene", objectId: "scene-01" });
    useSelectionStore.getState().setSaveState("dirty");

    expect(useSelectionStore.getState().objectId).toBe("scene-01");
    expect(useSelectionStore.getState().saveState).toBe("dirty");
    useSelectionStore.getState().clear();
    expect(useSelectionStore.getState().saveState).toBe("saved");
  });

  it("does not report saved while another field is still dirty", () => {
    const store = useSelectionStore.getState();
    store.markFieldDirty("title");
    store.beginFieldSave("title");
    useSelectionStore.getState().markFieldDirty("summary");
    useSelectionStore.getState().finishFieldSave("title", true);

    expect(useSelectionStore.getState().saveState).toBe("dirty");
    useSelectionStore.getState().beginFieldSave("summary");
    useSelectionStore.getState().finishFieldSave("summary", true);
    expect(useSelectionStore.getState().saveState).toBe("saved");
  });
});
