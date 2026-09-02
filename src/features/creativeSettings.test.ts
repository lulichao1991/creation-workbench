import { describe, expect, it } from "vitest";
import { creativeSettingsMutation, legacySettingsMutations, readCreativeSettings, studioStorageKey } from "./creativeSettings";
import { buildScriptRequest, emptyStudioDraft, importedScript, scriptMutations, withStyleDimension } from "./scriptStudio";
import type { ContentUnitRow, ProjectState } from "../types";

const unit = { id: "unit", project_id: "project", parent_id: null, sort_order: 0 } as ContentUnitRow;
const settings = { contentType: "documentary" as const, style: withStyleDimension(null, "visual", "东方水墨画风") };

describe("shared creative settings", () => {
  it("migrates only missing project settings, preserving the original local draft", () => {
    const original = JSON.stringify({ draft: { ...emptyStudioDraft, text: "未经采访的拍摄计划", ...settings }, taskId: "running", result: null });
    const reads = new Map([[studioStorageKey(unit.project_id, unit.id), original]]);
    const mutations = legacySettingsMutations([unit], (key) => reads.get(key) ?? null);
    expect(mutations).toEqual([creativeSettingsMutation(unit.id, settings)]);
    expect(reads.get(studioStorageKey(unit.project_id, unit.id))).toBe(original);
    expect(legacySettingsMutations([{ ...unit, creative_settings_json: "{}" }], () => original)).toEqual([]);
    expect(legacySettingsMutations([unit], () => "broken json")).toEqual([]);
  });
  it("uses the project value for scripts and respects clear without reviving old preferences", () => {
    const row = { ...unit, creative_settings_json: JSON.stringify(settings) };
    const shared = readCreativeSettings(row);
    const prompt = buildScriptRequest({ ...emptyStudioDraft, text: "一天的拍摄计划", ...shared });
    expect(prompt).toContain("非虚构纪录片");
    expect(prompt).toContain("东方水墨画风");
    expect(readCreativeSettings({ ...unit, creative_settings_json: "{}" })).toEqual({ contentType: "auto", style: null });
    expect(readCreativeSettings({ ...unit, creative_settings_json: "bad" })).toEqual({ contentType: "auto", style: null });
    expect(readCreativeSettings({ ...unit, id: "another" })).toEqual({ contentType: "auto", style: null });
  });
  it("does not revive local preferences after undo restores the original empty field", () => {
    const undone = { ...unit, creative_settings_json: "" };
    const legacy = JSON.stringify({ draft: { ...emptyStudioDraft, ...settings } });
    const history = [{ object_type: "contentUnit", object_id: unit.id, field_name: "creative_settings_json" }];
    expect(readCreativeSettings(undone)).toEqual({ contentType: "auto", style: null });
    expect(legacySettingsMutations([undone], () => legacy, history)).toEqual([]);
    expect(legacySettingsMutations([{ ...undone, id: "another" }], () => legacy, history)).toHaveLength(1);
    expect(legacySettingsMutations([undone], () => legacy, [{ ...history[0], field_name: "summary" }])).toHaveLength(1);
  });
  it("copies settings atomically to new episodes without changing existing content", () => {
    const row = { ...unit, creative_settings_json: JSON.stringify(settings) };
    const state = { scripts: [], scenes: [], contentUnits: [row] } as unknown as ProjectState;
    const result = importedScript("不可改写的原文", "原稿");
    result.episodes = Array.from({ length: 3 }, () => result.episodes[0]);
    let id = 0;
    const { mutations } = scriptMutations(result, state, row, () => `id-${id++}`, settings.contentType);
    const created = mutations.filter((mutation) => mutation.entityType === "contentUnit");
    expect(created).toHaveLength(3);
    expect(created.every((mutation) => mutation.values?.creative_settings_json === row.creative_settings_json)).toBe(true);
    expect(mutations.every((mutation) => mutation.action === "create")).toBe(true);
    expect(state.scenes).toEqual([]);
  });
});
