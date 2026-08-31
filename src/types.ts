export type Maturity = "exploring" | "usable" | "stable";
export type SyncStatus = "normal" | "needs_review" | "affected";
export type Workspace =
  | "overview"
  | "script"
  | "shots"
  | "assets"
  | "keyframes"
  | "generation"
  | "history";

export interface ProjectDescriptor {
  id: string;
  name: string;
  description: string;
  structureType: string;
  maturity: Maturity;
  syncStatus: SyncStatus;
  revision: number;
  path: string;
  updatedAt: string;
}

export interface ProjectRow {
  id: string;
  name: string;
  description: string;
  structure_type: string;
  maturity: Maturity;
  sync_status: SyncStatus;
  revision: number;
  created_at: string;
  updated_at: string;
}

export interface ContentUnitRow {
  id: string;
  project_id: string;
  parent_id: string | null;
  type: "season" | "episode" | "short" | "act" | "custom";
  name: string;
  summary: string;
  sort_order: number;
  maturity: Maturity;
  sync_status: SyncStatus;
  created_at: string;
  updated_at: string;
}

export interface ScriptRow {
  id: string;
  content_unit_id: string;
  title: string;
  summary: string;
  maturity: Maturity;
  sync_status: SyncStatus;
  created_at: string;
  updated_at: string;
}

export interface SceneRow {
  id: string;
  script_id: string;
  title: string;
  sort_order: number;
  location_text: string;
  time_text: string;
  summary: string;
  content: string;
  maturity: Maturity;
  sync_status: SyncStatus;
  created_at: string;
  updated_at: string;
}

export interface ShotRow {
  id: string;
  scene_id: string;
  sort_order: number;
  title: string;
  duration: number;
  narrative_purpose: string;
  new_information: string;
  shot_size: string;
  camera_height: string;
  camera_direction: string;
  composition: string;
  camera_movement: string;
  subjects: string;
  action: string;
  dialogue: string;
  environment: string;
  start_state: string;
  end_state: string;
  maturity: Maturity;
  sync_status: SyncStatus;
  created_at: string;
  updated_at: string;
}

export interface AssetRow {
  id: string;
  project_id: string;
  type: "character" | "location" | "prop";
  name: string;
  description: string;
  scope_unit_id: string | null;
  maturity: Maturity;
  sync_status: SyncStatus;
  created_at: string;
  updated_at: string;
}

export interface AssetMediaRow {
  id: string;
  asset_id: string;
  media_type: string;
  file_path: string;
  label: string;
  description: string;
  sort_order: number;
  is_primary: number;
  source_type: "manual" | "generated";
  created_at: string;
  updated_at: string;
}

export interface AssetRequirementRow {
  id: string;
  content_unit_id: string | null;
  asset_id: string | null;
  asset_type: string;
  requirement_type: string;
  description: string;
  prompt_draft: string;
  status: string;
  created_from_type: string | null;
  created_from_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface AssetRequirementSourceRow {
  id: string;
  asset_requirement_id: string;
  source_type: string;
  source_id: string;
  created_at: string;
  updated_at: string;
}

export interface AssetMediaRequirementRow {
  id: string;
  asset_media_id: string;
  asset_requirement_id: string;
  created_at: string;
  updated_at: string;
}

export interface ShotAssetRow {
  id: string;
  shot_id: string;
  asset_id: string;
  role: string;
  created_at: string;
  updated_at: string;
}

export interface KeyframeRow {
  id: string;
  shot_id: string;
  type: "single" | "start" | "middle" | "end";
  file_path: string | null;
  description: string;
  prompt_draft: string;
  status: "planned" | "ready";
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface GenerationTaskRow {
  id: string;
  content_unit_id: string;
  name: string;
  target_model: string;
  duration: number;
  prompt: string;
  status: string;
  created_at: string;
  updated_at: string;
}

export interface GenerationTaskShotRow {
  generation_task_id: string;
  shot_id: string;
  sort_order: number;
}

export interface RelationRow {
  id: string;
  project_id: string;
  source_type: string;
  source_id: string;
  relation_type: string;
  target_type: string;
  target_id: string;
  description: string;
  importance: number;
  status: string;
  created_at: string;
  updated_at: string;
}

export type StoryElementType = "mainline" | "character_arc" | "foreshadow" | "event" | "theme" | "custom";

export interface StoryElementRow {
  id: string;
  project_id: string;
  type: StoryElementType;
  name: string;
  description: string;
  scope_unit_id: string | null;
  maturity: Maturity;
  status: string;
  created_at: string;
  updated_at: string;
}

export interface StoryElementOccurrenceRow {
  id: string;
  story_element_id: string;
  content_unit_id: string;
  occurrence_type: string;
  description: string;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface GraphLayoutRow {
  id: string;
  scope_type: string;
  scope_id: string;
  view_type: string;
  filter_json: string;
  layout_json: string;
  updated_at: string;
}

export interface ChangeSetRow {
  id: string;
  project_id: string;
  name: string;
  source_type: string;
  source_id: string | null;
  status: "closed" | "undone";
  created_at: string;
  closed_at: string | null;
}

export interface ChangeRow {
  id: string;
  change_set_id: string;
  object_type: string;
  object_id: string;
  field_name: string;
  old_value: string | null;
  new_value: string | null;
  source_type: string;
  source_id: string | null;
  created_at: string;
}

export interface SnapshotRow {
  id: string;
  project_id: string;
  scope_type: string;
  scope_id: string | null;
  name: string;
  description: string;
  revision: number;
  snapshot_json: string;
  created_at: string;
}

export interface ProjectState {
  projects: ProjectRow[];
  contentUnits: ContentUnitRow[];
  scripts: ScriptRow[];
  scenes: SceneRow[];
  shots: ShotRow[];
  assets: AssetRow[];
  assetMedia: AssetMediaRow[];
  assetRequirements: AssetRequirementRow[];
  assetRequirementSources: AssetRequirementSourceRow[];
  assetMediaRequirements: AssetMediaRequirementRow[];
  shotAssets: ShotAssetRow[];
  keyframes: KeyframeRow[];
  generationTasks: GenerationTaskRow[];
  generationTaskShots: GenerationTaskShotRow[];
  relations: RelationRow[];
  storyElements: StoryElementRow[];
  storyElementOccurrences: StoryElementOccurrenceRow[];
  graphLayouts: GraphLayoutRow[];
  changeSets: ChangeSetRow[];
  changes: ChangeRow[];
  snapshots: SnapshotRow[];
}

export interface MutationRequest {
  action: "create" | "patch" | "delete" | "move";
  entityType: string;
  objectId?: string;
  values?: Record<string, unknown>;
  changeSetId?: string;
  changeSetName?: string;
  sourceType?: string;
  sourceId?: string;
}

export interface MutationResponse {
  objectId: string;
  changeSetId: string;
  revision: number;
}

export interface BatchMutationRequest {
  mutations: MutationRequest[];
  changeSetId?: string;
  changeSetName?: string;
  sourceType?: string;
  sourceId?: string;
}

export interface BatchMutationResponse {
  objectIds: string[];
  changeSetId: string;
  revision: number;
}

export interface SaveGraphLayoutInput {
  scopeType: "project" | "contentUnit";
  scopeId: string;
  viewType: "timeline" | "graph" | "episodes";
  filterJson: string;
  layoutJson: string;
}
