export const memoryStatuses = ["candidate", "active", "superseded", "invalidated"] as const;
export type MemoryStatus = (typeof memoryStatuses)[number];

export interface MemoryRef {
  id: string;
  scope: "global" | "project";
  status: MemoryStatus;
}
