export interface ImageGenerationRequest {
  targetType: "asset" | "keyframe";
  targetId: string;
  prompt: string;
  referenceImages: string[];
  provider?: string;
  model?: string;
  generationOptions?: Record<string, unknown>;
}

export interface ImageGenerationJob {
  id: string;
  status: "unconfigured" | "queued" | "running" | "completed" | "failed";
}

export interface ImageGenerationProvider {
  generate(request: ImageGenerationRequest): Promise<ImageGenerationJob>;
  getJob(id: string): Promise<ImageGenerationJob>;
  getResults(id: string): Promise<string[]>;
  cancelJob(id: string): Promise<void>;
}

export class ImageGenerationSystem implements ImageGenerationProvider {
  async generate(_request: ImageGenerationRequest): Promise<ImageGenerationJob> {
    throw new Error("尚未配置图像生成服务");
  }

  async getJob(_id: string): Promise<ImageGenerationJob> {
    return { id: _id, status: "unconfigured" };
  }

  async getResults(_id: string): Promise<string[]> {
    return [];
  }

  async cancelJob(_id: string): Promise<void> {
    return undefined;
  }
}
