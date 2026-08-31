import type { ObjectRef } from "../context";

export type PermissionState = "allowed" | "requires_confirmation" | "denied" | "stale";

export interface WriteScope {
  refs: ObjectRef[];
  protectedRefs: ObjectRef[];
}
