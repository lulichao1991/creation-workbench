import { describe, expect, it } from "vitest";
import { generationCostNotice, type ProviderConfig } from "./imageGeneration";

const provider = (providerType: ProviderConfig["providerType"]): ProviderConfig => ({
  id: "provider",
  providerType,
  displayName: providerType === "mock" ? "Mock Provider" : "OpenAI",
  baseUrl: "https://api.openai.com/v1",
  textToImagePath: "/images/generations",
  imageEditPath: "/images/edits",
  defaultModel: "gpt-image-1",
  capabilities: {},
  timeoutSeconds: 120,
  maxConcurrency: 1,
  allowImageUpload: false,
  status: "configured",
  hasSecret: providerType !== "mock",
  createdAt: "",
  updatedAt: "",
});

describe("image generation cost confirmation", () => {
  it("distinguishes free mock validation from billable providers", () => {
    expect(generationCostNotice(provider("mock"), { count: 2 })).toContain("不产生服务商费用");
    expect(generationCostNotice(provider("openai_compatible"), { count: 2, size: "1024x1536" })).toContain("费用由 OpenAI");
  });
});
