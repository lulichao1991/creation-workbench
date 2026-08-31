export const featureFlagKeys = [
  "agent_core",
  "expert_agents",
  "change_analysis",
  "story_graph",
  "memory",
  "image_generation",
  "prompt_compiler",
  "expert_team",
] as const;

export type FeatureFlagKey = (typeof featureFlagKeys)[number];
export type FeatureFlags = Record<FeatureFlagKey, boolean>;
