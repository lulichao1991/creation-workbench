import { afterEach, describe, expect, it, vi } from "vitest";
import { withTimeout } from "./async";

describe("withTimeout", () => {
  afterEach(() => vi.useRealTimers());

  it("turns a stalled request into a retryable error", async () => {
    vi.useFakeTimers();
    const result = withTimeout(new Promise<string>(() => undefined), 1000, "读取超时");
    const assertion = expect(result).rejects.toThrow("读取超时");
    await vi.advanceTimersByTimeAsync(1000);
    await assertion;
  });
});
