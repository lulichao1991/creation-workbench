import { describe, expect, it } from "vitest";
import { ImageGenerationSystem } from "./imageGeneration";

describe("ImageGenerationSystem phase-one stub", () => {
  it("keeps the provider boundary without generating images", async () => {
    const system = new ImageGenerationSystem();
    await expect(
      system.generate({
        targetType: "asset",
        targetId: "asset-1",
        prompt: "测试",
        referenceImages: [],
      }),
    ).rejects.toThrow("尚未配置图像生成服务");
  });
});
