export interface ModelProfile {
  key: string;
  displayName: string;
  provider: string;
  promptFormat: string;
  maxDurationHint: number | null;
  maxShotsHint: number | null;
  imageReferenceRules: string;
  supportsStartEndFrame: boolean;
  recommendedConstraints: string[];
  prohibitedPatterns: string[];
  version: string;
  createdAt: string;
  updatedAt: string;
}

export type SaveModelProfileInput = Omit<ModelProfile, "createdAt" | "updatedAt">;

export interface PromptTemplate {
  id: string;
  scope: "global" | "project";
  projectId: string | null;
  modelProfileKey: string;
  name: string;
  version: string;
  templateBody: string;
  conditionalRules: Record<string, unknown>;
  active: boolean;
  createdAt: string;
  updatedAt: string;
}

export type SavePromptTemplateInput = Omit<PromptTemplate, "createdAt" | "updatedAt">;

export interface PromptWarning {
  code: string;
  severity: "info" | "warning" | "error";
  message: string;
  sourceId: string | null;
}

export interface PromptSourceMapEntry {
  start: number;
  end: number;
  sourceType: string;
  sourceId: string;
  label: string;
}

export interface PromptCompilation {
  id: string;
  generationTaskId: string;
  modelProfileKey: string;
  modelProfileVersion: string;
  templateId: string;
  templateVersion: string;
  sourceRevision: number;
  compiledPrompt: string;
  userOverride: string | null;
  currentPrompt: string | null;
  sourceMap: PromptSourceMapEntry[];
  warnings: PromptWarning[];
  status: "compiled" | "current" | "stale";
  createdAt: string;
  updatedAt: string;
}

export const defaultModelProfile: SaveModelProfileInput = {
  key: "generic-video-v1",
  displayName: "通用视频模型",
  provider: "local-profile",
  promptFormat: "plain_text",
  maxDurationHint: 15,
  maxShotsHint: 8,
  imageReferenceRules: "正式资产与关键帧只作为视觉事实引用，不虚构缺失的参考图。",
  supportsStartEndFrame: true,
  recommendedConstraints: ["保持角色、场景与道具连续", "严格遵循镜头顺序与已写明的状态变化"],
  prohibitedPatterns: [],
  version: "1.0",
};

export const defaultPromptTemplate: SavePromptTemplateInput = {
  id: "generic-video-v1-default",
  scope: "global",
  projectId: null,
  modelProfileKey: defaultModelProfile.key,
  name: "通用结构化模板",
  version: "1.0",
  templateBody: "{{header}}\n\n视觉规则\n{{visual_rules}}\n\n镜头清单\n{{shots}}\n\n约束\n{{constraints}}",
  conditionalRules: {},
  active: true,
};

export function promptForEditing(compilation: PromptCompilation): string {
  return compilation.currentPrompt ?? compilation.userOverride ?? compilation.compiledPrompt;
}
