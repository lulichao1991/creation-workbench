import { describe, expect, it } from "vitest";
import { buildScriptRequest, contentPresets, creationTypes, emptyStudioDraft, importedScript, normalizeCreativeStyle, normalizeCreationType, parseScriptResult, scriptMutations, styleLibrarySections, stylePresets, stylePrompt, styleSelections, toggleStylePreset, withStyleDimension, type CreationType } from "./scriptStudio";
import type { ContentUnitRow, ProjectState } from "../types";
import { shuoVisualCategories, shuoVisualStyles } from "./shuoVisualStyles";

const unit = { id: "unit", project_id: "project", parent_id: null, sort_order: 0, type: "short", name: "正片" } as ContentUnitRow;
const state = { scripts: [], scenes: [], contentUnits: [unit] } as unknown as ProjectState;
const draft = importedScript("  原文\n对白：别动。\n", "来信");
const ink = stylePresets.visual.find((item) => item.id === "oriental-ink-wash")!;
describe("script studio", () => {
  it("bundles each matched SHUO thumbnail locally without changing prompt settings", () => {
    const assets = import.meta.glob<string>("../assets/shuo-story-styles/*.webp", { eager: true, import: "default", query: "?url" });
    const illustrated = Object.values(stylePresets).flat().filter((item) => item.thumbnail);
    expect(illustrated).toHaveLength(103);
    expect(Object.keys(assets)).toHaveLength(new Set(illustrated.map((item) => item.thumbnail)).size);
    for (const item of illustrated) {
      expect(assets[`../assets/shuo-story-styles/${item.thumbnail}.webp`]).toBeTruthy();
      expect(item.prompt).not.toContain(".webp");
      expect(item.prompt).not.toContain("SHUO");
    }
    expect(stylePresets.visual.some((item) => item.id === "watercolor")).toBe(false);
    expect(contentPresets.every((item) => !item.thumbnail)).toBe(true);
    expect(stylePresets.platform.every((item) => !item.thumbnail)).toBe(true);
  });
  it("keeps all SHUO visual styles and original categories without added descriptions", () => {
    expect(shuoVisualCategories.map((item) => item.label)).toEqual(["全部", "真人", "2D", "3D"]);
    expect(shuoVisualStyles).toHaveLength(94);
    expect(["live", "2d", "3d"].map((category) => shuoVisualStyles.filter((item) => item.category === category).length)).toEqual([35, 30, 29]);
    expect(shuoVisualStyles.every((item) => item.name === item.prompt && item.thumbnail === item.id && item.description === "")).toBe(true);
    expect(shuoVisualStyles[0].name).toBe("复古科幻原子朋克");
    expect(shuoVisualStyles[93].name).toBe("像素风");
    expect(shuoVisualStyles.some((item) => item.name === "上美画风")).toBe(true);
    const migrated = normalizeCreativeStyle({ dimensions: { visual: "东方水墨、墨色层次、大面积留白；不限定古装或时代", genre: "已选题材" } });
    expect(migrated?.dimensions.visual).toBe("东方水墨画风");
    expect(migrated?.dimensions.genre).toBe("已选题材");
  });
  it("provides exactly four independent preset libraries without model selectors", () => {
    expect(Object.values(styleLibrarySections)).toEqual(["内容形态", "题材类型", "视觉风格", "发布平台"]);
    expect(contentPresets.map((item) => item.id)).toEqual(Object.keys(creationTypes).filter((key) => key !== "auto"));
    for (const presets of Object.values(stylePresets)) {
      expect(new Set(presets.map((item) => item.id)).size).toBe(presets.length);
      for (const item of presets) {
        expect(item.name && item.prompt).toBeTruthy();
        expect(item.prompt).not.toContain("\n");
      }
    }
    const prompt = buildScriptRequest({ ...emptyStudioDraft, text: "故事设定", style: withStyleDimension(null, "visual", ink.prompt), episodes: 3 });
    expect(prompt).toContain("生成 3 集");
    expect(prompt).toContain("不修改项目");
    expect(prompt).not.toMatch(/Seedance|gemini|gpt-/i);
  });
  it("combines multiple genres while keeping visual and platform choices independent", () => {
    let selected = toggleStylePreset(null, "genre", stylePresets.genre[0].prompt);
    selected = toggleStylePreset(selected, "genre", stylePresets.genre[1].prompt);
    selected = toggleStylePreset(selected, "visual", ink.prompt);
    selected = toggleStylePreset(selected, "platform", stylePresets.platform[0].prompt);
    expect(styleSelections(selected).map((item) => item.label)).toEqual(["悬疑", "科幻", "东方水墨画风", "竖屏短视频"]);
    const prompt = buildScriptRequest({ ...emptyStudioDraft, text: "未来城市的失踪案", contentType: "drama", style: selected });
    for (const label of Object.values(styleLibrarySections)) expect(prompt).toContain(`${label}：`);
    expect(prompt).toContain("不是平台硬性规格");
    const removed = toggleStylePreset(selected, "genre", stylePresets.genre[0].prompt);
    expect(removed?.dimensions.genre).toBe(stylePresets.genre[1].prompt);
    expect(removed?.dimensions.visual).toBe(ink.prompt);
    expect(removed?.dimensions.platform).toBe(stylePresets.platform[0].prompt);
    expect(styleSelections(selected)).toHaveLength(4);
    const replaced = toggleStylePreset(removed, "visual", stylePresets.visual[0].prompt);
    expect(replaced?.dimensions.visual).toBe(stylePresets.visual[0].prompt);
    expect(styleSelections(toggleStylePreset(replaced, "visual", stylePresets.visual[0].prompt))).toHaveLength(2);
    expect(withStyleDimension(withStyleDimension(null, "visual", ink.prompt), "visual", "")).toBeNull();
  });
  it("applies only a visual preference when no other dimension was selected", () => {
    const selected = withStyleDimension(null, "visual", ink.prompt);
    expect(selected?.dimensions.genre).toBe("");
    expect(selected?.dimensions.platform).toBe("");
    expect(stylePrompt(selected)).toContain("视觉风格：东方水墨");
    expect(stylePrompt(selected)).not.toMatch(/题材类型：|发布平台：|叙事：|情绪：|节奏：|语言：|镜头：|声音：/);
    expect(stylePrompt(null)).toBe("不预设风格，以用户的内容设定为准。");
  });
  it("reads old visual choices without applying retired hidden dimensions or mutating the source", () => {
    const old = { name: "旧组合", dimensions: { narrative: "强制反转", tone: "强制冷峻", visual: ink.prompt, camera: "强制手持" } };
    const migrated = normalizeCreativeStyle(old);
    expect(migrated).toEqual({ dimensions: { genre: "", visual: ink.prompt, platform: "" } });
    expect(styleSelections(migrated)[0].label).toBe("东方水墨画风");
    expect(stylePrompt(old as unknown as NonNullable<typeof migrated>)).not.toContain("强制");
    expect(old.dimensions.narrative).toBe("强制反转");
    expect(styleSelections(normalizeCreativeStyle({ dimensions: { visual: "用户已有的独立视觉偏好" } }))[0].prompt).toBe("用户已有的独立视觉偏好");
    for (const bad of [null, {}, { dimensions: [] }, { dimensions: { visual: {} } }, { dimensions: { genre: ["科幻"] } }]) expect(normalizeCreativeStyle(bad)).toBeNull();
  });
  it("keeps content form separate from genre and respects non-fiction constraints", () => {
    const style = toggleStylePreset(withStyleDimension(null, "visual", ink.prompt), "genre", stylePresets.genre[1].prompt);
    const request = (contentType: CreationType) => buildScriptRequest({ ...emptyStudioDraft, text: "记录一个手艺人的一天", style, contentType });
    expect(request("documentary")).toContain("不得编造真实人物经历");
    expect(request("documentary")).not.toContain("允许按用户设定虚构剧情");
    expect(request("documentary")).toContain("虚构题材与非虚构任务冲突时，不得编造事实");
    expect(request("advertising")).toContain("不得编造产品性能");
    expect(request("explainer")).toContain("不编造数据或来源");
    expect(request("music")).toContain("可以无对白、无线性剧情");
    expect(request("drama")).toContain("允许按用户设定虚构剧情");
    expect(normalizeCreationType(undefined)).toBe("auto");
    expect(normalizeCreationType("toString")).toBe("auto");
  });
  it("preserves imported text exactly and validates empty or oversized sources", () => {
    expect(draft.episodes[0].scenes[0].content).toBe("  原文\n对白：别动。\n");
    expect(() => importedScript(" ", "空白")).toThrow();
    expect(() => importedScript("文".repeat(100001), "过长")).toThrow();
    expect(() => buildScriptRequest({ ...emptyStudioDraft, text: "故事", episodes: 100 })).toThrow();
  });
  it("rejects malformed, truncated or wrong-episode AI output before any write", () => {
    expect(parseScriptResult({ findings: [draft] }, 1)).toEqual(draft);
    for (const invalid of [null, {}, { findings: {} }, { findings: [{ ...draft, episodes: [] }] }, { findings: [{ ...draft, episodes: [null] }] }, { findings: [{ ...draft, episodes: [{ title: "场", summary: "", scenes: [] }] }] }]) expect(() => parseScriptResult(invalid, 1)).toThrow("生成结果不完整");
    expect(() => parseScriptResult({ findings: [draft] }, 3)).toThrow();
  });
  it("builds a single batch with explicit foreign keys and preserves unrelated content", () => {
    let id = 0;
    const batch = scriptMutations(draft, state, unit, () => `new-${++id}`);
    expect(batch.firstUnitId).toBe(unit.id);
    expect(batch.mutations.map((item) => item.entityType)).toEqual(["script", "scene"]);
    expect(batch.mutations[1].values?.script_id).toBe(batch.mutations[0].objectId);
    const series = scriptMutations({ ...draft, episodes: [draft.episodes[0], draft.episodes[0], draft.episodes[0]] }, state, unit, () => `new-${++id}`);
    expect(series.mutations.filter((item) => item.entityType === "contentUnit")).toHaveLength(3);
    expect(series.mutations.every((item) => item.action === "create")).toBe(true);
    expect(series.mutations.some((item) => item.objectId === unit.id)).toBe(false);
    const ads = scriptMutations({ ...draft, episodes: [draft.episodes[0], draft.episodes[0]] }, state, unit, () => `new-${++id}`, "advertising");
    expect(ads.mutations[0].values?.name).toBe("第 1 条 · 来信");
  });
  it("refuses to replace an existing script or populated scene", () => {
    const existing = { id: "script", content_unit_id: unit.id, summary: "已有梗概" };
    expect(() => scriptMutations(draft, { ...state, scripts: [existing] } as ProjectState, unit)).toThrow("未覆盖");
    expect(() => scriptMutations(draft, { ...state, scripts: [{ ...existing, summary: "" }], scenes: [{ script_id: "script" }] } as ProjectState, unit)).toThrow("未覆盖");
  });
});
