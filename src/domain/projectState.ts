import type { ContentUnitRow, ProjectState, ShotRow, Workspace } from "../types";

type ShotOrderingState = Pick<ProjectState, "scripts" | "scenes" | "shots">;

export function orderedShotsForUnit(state: ShotOrderingState, unitId: string | null): ShotRow[] {
  const script = state.scripts.find((item) => item.content_unit_id === unitId);
  if (!script) return [];
  const scenes = state.scenes
    .filter((scene) => scene.script_id === script.id)
    .sort((a, b) => a.sort_order - b.sort_order || a.id.localeCompare(b.id));
  const sceneOrder = new Map(scenes.map((scene, index) => [scene.id, index]));
  return state.shots
    .filter((shot) => sceneOrder.has(shot.scene_id))
    .sort((a, b) => {
      const byScene = (sceneOrder.get(a.scene_id) ?? 0) - (sceneOrder.get(b.scene_id) ?? 0);
      return byScene || a.sort_order - b.sort_order || a.id.localeCompare(b.id);
    });
}

const productionWorkspaces = new Set<Workspace>(["script", "shots", "keyframes", "generation"]);

export function supportsWorkspace(unit: ContentUnitRow | null, workspace: Workspace): boolean {
  if (!unit) return workspace === "overview" || workspace === "assets" || workspace === "history";
  if (unit.type === "season") return !productionWorkspaces.has(workspace);
  return true;
}

type AssetNavigationState = Pick<ProjectState, "assetMedia" | "assetRequirements" | "assetRequirementSources" | "assetMediaRequirements">;

export function assetIdForSelection(state: AssetNavigationState, objectType: string | null, objectId: string | null): string | null {
  if (!objectId) return null;
  if (objectType === "asset") return objectId;
  if (objectType === "assetMedia") return state.assetMedia.find((item) => item.id === objectId)?.asset_id ?? null;
  if (objectType === "assetRequirement") return state.assetRequirements.find((item) => item.id === objectId)?.asset_id ?? null;
  if (objectType === "assetRequirementSource") {
    const requirementId = state.assetRequirementSources.find((item) => item.id === objectId)?.asset_requirement_id;
    return state.assetRequirements.find((item) => item.id === requirementId)?.asset_id ?? null;
  }
  if (objectType === "assetMediaRequirement") {
    const mediaId = state.assetMediaRequirements.find((item) => item.id === objectId)?.asset_media_id;
    return state.assetMedia.find((item) => item.id === mediaId)?.asset_id ?? null;
  }
  return null;
}

type ShotNavigationState = Pick<ProjectState, "keyframes">;

export function shotIdForSelection(state: ShotNavigationState, objectType: string | null, objectId: string | null): string | null {
  if (!objectId) return null;
  if (objectType === "shot") return objectId;
  if (objectType === "keyframe") return state.keyframes.find((item) => item.id === objectId)?.shot_id ?? null;
  return null;
}
