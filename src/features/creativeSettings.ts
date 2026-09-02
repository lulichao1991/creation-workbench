import type { ChangeRow, ContentUnitRow, MutationRequest } from "../types";
import { normalizeCreativeStyle, normalizeCreationType, type CreativeStyle, type CreationType } from "./scriptStudio";

export interface CreativeSettings { contentType: CreationType; style: CreativeStyle | null }
export const studioStorageKey = (projectId: string, unitId: string) => `workbench.scriptStudio:${projectId}:${unitId}`;

export function normalizeCreativeSettings(value: unknown): CreativeSettings {
  const source = value && typeof value === "object" ? value as Partial<CreativeSettings> : {};
  return { contentType: normalizeCreationType(source.contentType), style: normalizeCreativeStyle(source.style) };
}

export function readCreativeSettings(unit: ContentUnitRow): CreativeSettings {
  // Legacy preferences are migrated before opening the project, never read as a fallback.
  try { return normalizeCreativeSettings(JSON.parse(unit.creative_settings_json || "null")); }
  catch { return normalizeCreativeSettings(null); }
}

export function creativeSettingsMutation(unitId: string, settings: CreativeSettings): MutationRequest {
  return { action: "patch", entityType: "contentUnit", objectId: unitId, values: { creative_settings_json: JSON.stringify(normalizeCreativeSettings(settings)) } };
}

export function legacySettingsMutations(units: ContentUnitRow[], read: (key: string) => string | null, changes: Pick<ChangeRow, "object_type" | "object_id" | "field_name">[] = []): MutationRequest[] {
  const configured = new Set(changes.filter((change) => change.object_type === "contentUnit" && change.field_name === "creative_settings_json").map((change) => change.object_id));
  return units.flatMap((unit) => {
    // Undo may restore the original empty string; it must not trigger another migration.
    if (unit.creative_settings_json || configured.has(unit.id)) return [];
    let draft: unknown;
    try { draft = JSON.parse(read(studioStorageKey(unit.project_id, unit.id)) ?? "null")?.draft; } catch { return []; }
    const settings = normalizeCreativeSettings(draft);
    return settings.contentType !== "auto" || settings.style ? [creativeSettingsMutation(unit.id, settings)] : [];
  });
}
