import { describe, expect, it } from "vitest";
import { assetIdForSelection, orderedShotsForUnit, shotIdForSelection, supportsWorkspace } from "./projectState";
import type { ContentUnitRow, ProjectState, SceneRow, ScriptRow, ShotRow } from "../types";

const base = { created_at: "", updated_at: "" };
const script = { ...base, id: "script", content_unit_id: "episode", title: "", summary: "", maturity: "exploring", sync_status: "normal" } as ScriptRow;
const scene = (id: string, sort_order: number) => ({ ...base, id, script_id: "script", title: id, sort_order, location_text: "", time_text: "", summary: "", content: "", maturity: "exploring", sync_status: "normal" }) as SceneRow;
const shot = (id: string, scene_id: string, sort_order: number) => ({ ...base, id, scene_id, sort_order, title: id, duration: 1, narrative_purpose: "", new_information: "", shot_size: "", camera_height: "", camera_direction: "", composition: "", camera_movement: "", subjects: "", action: "", dialogue: "", environment: "", start_state: "", end_state: "", maturity: "exploring", sync_status: "normal" }) as ShotRow;

describe("project state derivations", () => {
  it("orders shots by scene order before local shot order", () => {
    const state = {
      scripts: [script],
      scenes: [scene("scene-b", 1), scene("scene-a", 0)],
      shots: [shot("b-1", "scene-b", 0), shot("a-2", "scene-a", 1), shot("a-1", "scene-a", 0), shot("b-2", "scene-b", 1)],
    } as Pick<ProjectState, "scripts" | "scenes" | "shots">;
    expect(orderedShotsForUnit(state, "episode").map((item) => item.id)).toEqual(["a-1", "a-2", "b-1", "b-2"]);
  });

  it("prevents production workspaces on season containers", () => {
    const season = { type: "season" } as ContentUnitRow;
    expect(supportsWorkspace(season, "overview")).toBe(true);
    expect(supportsWorkspace(season, "assets")).toBe(true);
    expect(supportsWorkspace(season, "shots")).toBe(false);
  });

  it("keeps the parent asset selected while editing a requirement", () => {
    const state = {
      assetMedia: [],
      assetRequirementSources: [],
      assetMediaRequirements: [],
      assetRequirements: [{ id: "requirement-2", asset_id: "asset-2" }],
    } as unknown as Pick<ProjectState, "assetMedia" | "assetRequirements" | "assetRequirementSources" | "assetMediaRequirements">;
    expect(assetIdForSelection(state, "assetRequirement", "requirement-2")).toBe("asset-2");
  });

  it("keeps the parent shot selected while editing a keyframe", () => {
    const state = { keyframes: [{ id: "frame-5", shot_id: "shot-5" }] } as unknown as Pick<ProjectState, "keyframes">;
    expect(shotIdForSelection(state, "keyframe", "frame-5")).toBe("shot-5");
  });
});
