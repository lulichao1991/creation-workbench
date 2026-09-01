import assert from "node:assert/strict";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fauxAssistantMessage, fauxProvider, fauxToolCall } from "@earendil-works/pi-ai";

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

test("restores a persisted Pi SDK session after host restart", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-agent-host-resume-"));
  const faux = fauxProvider({ provider: "workbench-resume-test", tokensPerSecond: 0 });
  faux.setResponses([
    fauxAssistantMessage("方案一、方案二、方案三"),
    (context) => {
      const transcript = JSON.stringify(context.messages);
      assert.match(transcript, /方案一、方案二、方案三/);
      assert.match(transcript, /用第二个/);
      return fauxAssistantMessage("已恢复讨论并沿用第二个方案");
    },
    (context) => {
      const transcript = JSON.stringify(context.messages);
      assert.match(transcript, /已恢复讨论并沿用第二个方案/);
      assert.match(transcript, /再往后一点/);
      return fauxAssistantMessage("已将第二个方案继续后移");
    },
  ]);
  const sessionId = "00000000-0000-4000-8000-000000000003";
  const firstEvents: Record<string, unknown>[] = [];
  const firstHost = await WorkbenchAgentHost.create(
    dataDir,
    (event) => firstEvents.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
  );
  await firstHost.handle({
    id: "create-first",
    type: "create_session",
    sessionId,
    provider: faux.provider.id,
    model: faux.getModel().id,
  });
  await firstHost.handle({ id: "turn-first", type: "send_message", sessionId, taskId: "turn-1", message: "给三个方案" });
  await waitForEvent(firstEvents, "task_completed", "turn-1");
  firstHost.dispose();

  const secondEvents: Record<string, unknown>[] = [];
  const secondHost = await WorkbenchAgentHost.create(
    dataDir,
    (event) => secondEvents.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
  );
  try {
    const resumed = await secondHost.handle({
      id: "create-second",
      type: "create_session",
      sessionId,
      runtimeSessionId: sessionId,
      provider: faux.provider.id,
      model: faux.getModel().id,
    });
    assert.equal((resumed as Record<string, unknown>).resumed, true);
    await secondHost.handle({ id: "turn-second", type: "send_message", sessionId, taskId: "turn-2", message: "用第二个" });
    await waitForEvent(secondEvents, "task_completed", "turn-2");
    await secondHost.handle({ id: "turn-third", type: "send_message", sessionId, taskId: "turn-3", message: "再往后一点" });
    await waitForEvent(secondEvents, "task_completed", "turn-3");
    assert.equal(faux.state.callCount, 3);
  } finally {
    secondHost.dispose();
  }
});

test("runs multiple Workbench tools inside the real Pi Tool Loop", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-agent-host-tools-"));
  const faux = fauxProvider({ provider: "workbench-tools-test", tokensPerSecond: 0 });
  faux.setResponses([
    fauxAssistantMessage([
      fauxToolCall("get_selection", {}, { id: "tool-selection" }),
      fauxToolCall("read_shot_context", { shotId: "shot04" }, { id: "tool-shot" }),
    ], { stopReason: "toolUse" }),
    (context) => {
      const transcript = JSON.stringify(context.messages);
      assert.match(transcript, /projectRevision/);
      assert.match(transcript, /shot04/);
      return fauxAssistantMessage("压迫感不足来自主体尺度和前景遮挡不足");
    },
  ]);
  const calls: Array<{ toolName: string; arguments: Record<string, unknown> }> = [];
  const events: Record<string, unknown>[] = [];
  const host = await WorkbenchAgentHost.create(
    dataDir,
    (event) => events.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
    async (request) => {
      calls.push({ toolName: request.toolName, arguments: request.arguments });
      return request.toolName === "get_selection"
        ? { projectRevision: 7, data: { center: { objectType: "shot", objectId: "shot04" } } }
        : { projectRevision: 7, data: { shot: { id: "shot04", composition: "平视中景" } } };
    },
  );
  const sessionId = "00000000-0000-4000-8000-000000000004";
  try {
    await host.handle({
      id: "create-tools",
      type: "create_session",
      sessionId,
      provider: faux.provider.id,
      model: faux.getModel().id,
    });
    await host.handle({ id: "tool-turn", type: "send_message", sessionId, taskId: "tool-task", message: "这个镜头为什么不够有压迫感？" });
    await waitForEvent(events, "task_completed", "tool-task");
    assert.deepEqual(calls.map((call) => call.toolName).sort(), ["get_selection", "read_shot_context"]);
    assert.equal(events.filter((event) => event.event === "tool_call_requested").length, 2);
    assert.equal(events.filter((event) => event.event === "tool_call_completed").length, 2);
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
