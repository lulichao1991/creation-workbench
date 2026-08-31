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
});
