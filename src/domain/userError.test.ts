import { describe, expect, it } from "vitest";
import { toUserErrorMessage } from "./userError";

describe("user-facing errors", () => {
  it("replaces runtime internals with an actionable message", () => {
    expect(toUserErrorMessage(new Error("Pi SDK Agent Host stopped"))).toContain("自动重启");
    expect(toUserErrorMessage("Pi SDK Agent Host 已停止")).toContain("自动重启");
    expect(toUserErrorMessage(new Error("Cannot read properties of undefined (reading 'invoke')"))).not.toContain("invoke");
  });

  it("keeps clear business validation messages", () => {
    expect(toUserErrorMessage(new Error("没有找到该父级，请选择列表中的名称。"))).toBe("没有找到该父级，请选择列表中的名称。");
  });
});
