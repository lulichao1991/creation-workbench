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

type WorkspaceSelectionState = Pick<ProjectState, "scripts" | "scenes" | "shots" | "assets" | "keyframes" | "generationTasks">;

export function defaultObjectForWorkspace(
  state: WorkspaceSelectionState,
  unitId: string | null,
  workspace: Workspace,
): { objectType: string; objectId: string } | null {
  if (!unitId) return null;
  if (workspace === "script") {
    const script = state.scripts.find((item) => item.content_unit_id === unitId);
    const scene = script && state.scenes.filter((item) => item.script_id === script.id).sort((a, b) => a.sort_order - b.sort_order)[0];
    return scene ? { objectType: "scene", objectId: scene.id } : script ? { objectType: "script", objectId: script.id } : { objectType: "contentUnit", objectId: unitId };
  }
  if (workspace === "shots") {
    const shot = orderedShotsForUnit(state, unitId)[0];
    return shot ? { objectType: "shot", objectId: shot.id } : { objectType: "contentUnit", objectId: unitId };
  }
  if (workspace === "assets") {
    const asset = state.assets.find((item) => item.type === "character") ?? state.assets[0];
    return asset ? { objectType: "asset", objectId: asset.id } : { objectType: "contentUnit", objectId: unitId };
  }
  if (workspace === "keyframes") {
    const shot = orderedShotsForUnit(state, unitId)[0];
    const frame = shot && state.keyframes.filter((item) => item.shot_id === shot.id).sort((a, b) => a.sort_order - b.sort_order)[0];
    return frame ? { objectType: "keyframe", objectId: frame.id } : shot ? { objectType: "shot", objectId: shot.id } : { objectType: "contentUnit", objectId: unitId };
  }
  if (workspace === "generation") {
    const task = state.generationTasks.find((item) => item.content_unit_id === unitId);
    return task ? { objectType: "generationTask", objectId: task.id } : { objectType: "contentUnit", objectId: unitId };
  }
  return { objectType: "contentUnit", objectId: unitId };
}
