import type { ObjectRef } from "../context";

export type PermissionState = "allowed" | "requires_confirmation" | "denied" | "stale";

export interface WriteScope {
  refs: ObjectRef[];
  protectedRefs: ObjectRef[];
}

export type PatchProposalStatus =
  | "draft"
  | "pending"
  | "approved"
  | "applied"
  | "rejected"
  | "stale";

export type PatchApplyState = "pending" | "applied" | "rejected" | "denied" | "stale";

export interface PatchItemInput {
  objectType: ObjectRef["objectType"];
  objectId: string;
  fieldName: string;
  oldValue: unknown;
  newValue: unknown;
  reason?: string;
}

export interface ProposePatchInput {
  requestId: string;
  taskId: string;
  baseRevision: number;
  title: string;
  items: PatchItemInput[];
}

export interface PatchItem extends Omit<PatchItemInput, "reason"> {
  id: string;
  reason: string;
  permissionState: PermissionState;
  applyState: PatchApplyState;
  sortOrder: number;
}

export interface PatchProposal {
  id: string;
  taskId: string;
  baseRevision: number;
  title: string;
  status: PatchProposalStatus;
  items: PatchItem[];
  permissionCardId: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ApplyPatchInput {
  proposalId: string;
  approvedItemIds?: string[];
  rejectedItemIds?: string[];
  permissionCardId?: string | null;
}

export interface ApplyPatchResponse {
  proposalId: string;
  status: "applied" | "rejected";
  appliedItemIds: string[];
  rejectedItemIds: string[];
  changeSetId: string | null;
  revision: number;
}

export type AICardType =
  | "problem"
  | "question"
  | "permission"
  | "suggestion"
  | "expert_team"
  | "cost"
  | "stale";

export type AICardStatus = "open" | "resolved" | "dismissed";

export interface AICard {
  id: string;
  taskId: string;
  cardType: AICardType;
  relatedRef: ObjectRef | null;
  title: string;
  body: string;
  options: unknown;
  status: AICardStatus;
  resolution: unknown | null;
  createdAt: string;
  resolvedAt: string | null;
}

export interface CreateCardInput {
  requestId: string;
  taskId: string;
  cardType: AICardType;
  relatedRef?: ObjectRef | null;
  title: string;
  body: string;
  options?: unknown;
}

export interface ResolveCardInput {
  cardId: string;
  status: "resolved" | "dismissed";
  resolution: unknown;
}
