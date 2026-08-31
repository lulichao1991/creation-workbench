import { describe, expect, it } from "vitest";
import { promptForEditing, type PromptCompilation } from "./promptCompiler";

const compilation = (patch: Partial<PromptCompilation> = {}): PromptCompilation => ({
  id: "c", generationTaskId: "t", modelProfileKey: "m", modelProfileVersion: "1", templateId: "p", templateVersion: "1", sourceRevision: 1,
  compiledPrompt: "编译稿", userOverride: null, currentPrompt: null, sourceMap: [], warnings: [], status: "compiled", createdAt: "", updatedAt: "", ...patch,
});

describe("prompt compiler presentation", () => {
  it("keeps compiler output, override and current prompt distinct", () => {
    expect(promptForEditing(compilation())).toBe("编译稿");
    expect(promptForEditing(compilation({ userOverride: "覆盖稿" }))).toBe("覆盖稿");
    expect(promptForEditing(compilation({ userOverride: "覆盖稿", currentPrompt: "正式稿" }))).toBe("正式稿");
  });
});
