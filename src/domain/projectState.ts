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
