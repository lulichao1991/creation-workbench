export const memoryStatuses = ["candidate", "active", "superseded", "invalidated"] as const;
export type MemoryStatus = (typeof memoryStatuses)[number];

export interface MemoryRef {
  id: string;
  scope: "global" | "project";
  status: MemoryStatus;
}

export type MemoryStorage = "project" | "global";

export interface MemorySource {
  id: string;
  sourceType: string;
  sourceId: string | null;
  excerpt: string;
  createdAt: string;
}

export interface MemoryRecord {
  id: string;
  storage: MemoryStorage;
  scopeType: "project" | "contentUnit" | "global";
  scopeId: string | null;
  category: string;
  memoryKey: string | null;
  content: string;
  status: MemoryStatus;
  confidence: number;
  priority: number;
  sourceType: string;
  sourceId: string | null;
  supersedesId: string | null;
  createdAt: string;
  updatedAt: string;
  sources: MemorySource[];
  usedByTaskIds: string[];
  conflictIds: string[];
}

export interface CreateMemoryInput {
  requestId: string;
  storage: MemoryStorage;
  scopeType: "project" | "contentUnit" | "global";
  scopeId?: string;
  category: string;
  memoryKey?: string;
  content: string;
  status: MemoryStatus;
  confidence?: number;
  priority?: number;
  sourceType?: string;
  sourceId?: string;
  excerpt?: string;
  supersedesId?: string;
  confirmed?: boolean;
}

export interface UpdateMemoryInput {
  storage: MemoryStorage;
  memoryId: string;
  content?: string;
  category?: string;
  memoryKey?: string;
  scopeType?: "project" | "contentUnit" | "global";
  scopeId?: string;
  status?: MemoryStatus;
  confidence?: number;
  priority?: number;
  supersedesId?: string;
  confirmed?: boolean;
}
