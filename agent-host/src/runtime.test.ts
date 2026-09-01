import assert from "node:assert/strict";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fauxAssistantMessage, fauxProvider } from "@earendil-works/pi-ai";

import { WorkbenchAgentHost } from "./runtime.js";

test("creates an isolated real Pi SDK session without builtin tools", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-agent-host-"));
  const host = await WorkbenchAgentHost.create(dataDir, () => {});
  try {
    const doctor = host.doctor();
    assert.equal(doctor.healthy, true);
    assert.match(String(doctor.sdkVersion), /^0\.84\./);
    const created = await host.handle({ id: "create", type: "create_session", sessionId: "00000000-0000-4000-8000-000000000001" });
    assert.equal((created as Record<string, unknown>).runtimeSessionId, "00000000-0000-4000-8000-000000000001");
    assert.equal(host.doctor().sessionCount, 1);
  } finally {
    host.dispose();
  }
});

test("keeps two real Pi SDK turns in the same AgentSession", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-agent-host-turns-"));
  const faux = fauxProvider({ provider: "workbench-test", tokensPerSecond: 0 });
  faux.setResponses([
    fauxAssistantMessage("方案一、方案二、方案三"),
    (context) => {
      const transcript = JSON.stringify(context.messages);
      assert.match(transcript, /方案二/);
      assert.match(transcript, /方案一、方案二、方案三/);
      return fauxAssistantMessage("已沿用第二个方案继续调整");
    },
  ]);
  const events: Record<string, unknown>[] = [];
  const host = await WorkbenchAgentHost.create(
    dataDir,
    (event) => events.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
  );
  const sessionId = "00000000-0000-4000-8000-000000000002";
  try {
    await host.handle({
      id: "create",
      type: "create_session",
      sessionId,
      provider: faux.provider.id,
      model: faux.getModel().id,
    });
    await host.handle({ id: "first", type: "send_message", sessionId, taskId: "turn-1", message: "给三个方案" });
    await waitForEvent(events, "task_completed", "turn-1");
    await host.handle({ id: "second", type: "send_message", sessionId, taskId: "turn-2", message: "用方案二继续" });
    await waitForEvent(events, "task_completed", "turn-2");
    const text = events.filter((event) => event.event === "message_delta").map((event) => event.delta).join("");
    assert.match(text, /方案一、方案二、方案三/);
    assert.match(text, /已沿用第二个方案继续调整/);
    assert.equal(faux.state.callCount, 2);
  } finally {
    host.dispose();
  }
});

async function waitForEvent(events: Record<string, unknown>[], eventName: string, taskId: string): Promise<void> {
  const deadline = Date.now() + 2000;
  while (Date.now() < deadline) {
    if (events.some((event) => event.event === eventName && event.taskId === taskId)) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.fail(`未收到 ${taskId} 的 ${eventName}`);
}
