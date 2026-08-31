export type ImageTargetType = "assetRequirement" | "keyframe";
export type ImageJobStatus =
  | "created"
  | "queued"
  | "running"
  | "completed"
  | "partial"
  | "cancelled"
  | "failed"
  | "interrupted";
export type ImageSelectionState = "available" | "rejected" | "selected" | "archived" | "deleted";

export interface ProviderConfig {
  id: string;
  providerType: "openai_compatible" | "mock";
  displayName: string;
  baseUrl: string;
  defaultModel: string;
  capabilities: Record<string, boolean>;
  timeoutSeconds: number;
  maxConcurrency: number;
  allowImageUpload: boolean;
  status: string;
  hasSecret: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface SaveProviderInput {
  requestId: string;
  providerType: ProviderConfig["providerType"];
  displayName: string;
  baseUrl: string;
  defaultModel: string;
  apiKey?: string;
  timeoutSeconds?: number;
  maxConcurrency?: number;
  allowImageUpload?: boolean;
}

export interface ImageOptions {
  size?: "auto" | "1024x1024" | "1024x1536" | "1536x1024";
  quality?: string;
  count?: number;
  background?: string;
  mockMode?: "partial" | "fail" | "cancel";
}

export interface GenerateImageInput {
  requestId: string;
  targetType: ImageTargetType;
  targetId: string;
  providerId: string;
  model?: string;
  prompt: string;
  referenceImages: string[];
  options: ImageOptions;
}

export interface ImageResult {
  id: string;
  jobId: string;
  filePath: string;
  previewPath: string | null;
  metadata: Record<string, unknown>;
  sortOrder: number;
  selectionState: ImageSelectionState;
  createdAt: string;
}

export interface ImageJob {
  id: string;
  targetType: ImageTargetType;
  targetId: string;
  provider: string;
  model: string;
  prompt: string;
  promptRevision: number;
  referenceImages: string[];
  options: ImageOptions;
  status: ImageJobStatus;
  usage: Record<string, unknown>;
  error: { message?: string; retryable?: boolean } | null;
  createdAt: string;
  startedAt: string | null;
  completedAt: string | null;
  results: ImageResult[];
}

export interface SelectImageResult {
  formalPath: string;
  formalObjectId: string;
  revision: number;
}

export function generationCostNotice(provider: ProviderConfig, options: ImageOptions): string {
  const count = options.count ?? 1;
  const size = options.size ?? "1024x1024";
  const quality = options.quality ?? "auto";
  if (provider.providerType === "mock") return `Mock 验收模式 · ${count} 张 · 不产生服务商费用`;
  return `预计请求 ${count} 张 ${size}（${quality}）；费用由 ${provider.displayName} 按实际用量收取`;
}

export const terminalImageStatuses = new Set<ImageJobStatus>([
  "completed",
  "partial",
  "cancelled",
  "failed",
]);
