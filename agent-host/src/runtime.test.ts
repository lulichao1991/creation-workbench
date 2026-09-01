import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
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

test("main AgentSession calls an independent cinematography AgentSession and synthesizes its result", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-agent-host-expert-"));
  const faux = fauxProvider({ provider: "workbench-expert-test", tokensPerSecond: 0 });
  faux.setResponses([
    fauxAssistantMessage([
      fauxToolCall("call_expert", {
        expertType: "cinematography",
        task: "判断镜头04为什么缺少压迫感",
        focusRefs: [{ objectType: "shot", objectId: "shot04" }],
      }, { id: "call-cinematography" }),
    ], { stopReason: "toolUse" }),
    (context) => {
      const transcript = JSON.stringify(context.messages);
      assert.match(transcript, /判断镜头04为什么缺少压迫感/);
      assert.match(JSON.stringify(context.tools), /read_shot_context/);
      assert.doesNotMatch(JSON.stringify(context.tools), /call_expert/);
      return fauxAssistantMessage([
        fauxToolCall("read_shot_context", { shotId: "shot04" }, { id: "expert-read-shot" }),
      ], { stopReason: "toolUse" });
    },
    (context) => {
      assert.match(JSON.stringify(context.messages), /平视中景/);
      return fauxAssistantMessage('{"summary":"摄影专业结论：降低机位并增加前景遮挡","findings":["主体尺度不足"],"patchProposal":null,"questions":[],"risks":[]}');
    },
    (context) => {
      const transcript = JSON.stringify(context.messages);
      assert.match(transcript, /摄影专业结论/);
      assert.match(transcript, /expertSessionId/);
      return fauxAssistantMessage("主 Agent 综合：压迫感不足来自平视机位、主体尺度和前景层次不足。");
    },
  ]);
  const calls: Array<{ toolName: string; sessionId: string; taskId: string; parentTaskId?: string }> = [];
  const events: Record<string, unknown>[] = [];
  const expertSessionId = "00000000-0000-4000-8000-000000000030";
  const expertTaskId = "expert-task-cinematography";
  const host = await WorkbenchAgentHost.create(
    dataDir,
    (event) => events.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
    async (request) => {
      calls.push({
        toolName: request.toolName,
        sessionId: request.sessionId,
        taskId: request.taskId,
        parentTaskId: request.parentTaskId,
      });
      if (request.toolName === "call_expert") {
        return {
          projectRevision: 7,
          data: {
            expertType: "cinematography",
            expertSessionId,
            expertTaskId,
            runtimeSessionId: expertSessionId,
            systemPrompt: "你是摄影 Agent，只从摄影语言和空间关系提出建议。",
            allowedTools: ["get_selection", "read_shot_context", "read_asset", "read_neighbors"],
            provider: faux.provider.id,
            model: faux.getModel().id,
            thinkingLevel: "off",
            allowImages: false,
          },
        };
      }
      if (request.toolName === "read_shot_context") {
        return { projectRevision: 7, data: { shot: { id: "shot04", composition: "平视中景" } } };
      }
      if (request.toolName === "complete_expert") return { completed: true };
      throw new Error(`意外工具：${request.toolName}`);
    },
  );
  const mainSessionId = "00000000-0000-4000-8000-000000000005";
  try {
    await host.handle({
      id: "create-main",
      type: "create_session",
      sessionId: mainSessionId,
      provider: faux.provider.id,
      model: faux.getModel().id,
    });
    await host.handle({
      id: "ask-main",
      type: "send_message",
      sessionId: mainSessionId,
      taskId: "main-expert-task",
      message: "这个镜头为什么不够有压迫感？",
    });
    await waitForEvent(events, "task_completed", "main-expert-task");
    assert.deepEqual(calls.map((call) => call.toolName), [
      "call_expert",
      "read_shot_context",
      "complete_expert",
    ]);
    assert.equal(calls[1].sessionId, expertSessionId);
    assert.equal(calls[1].taskId, expertTaskId);
    assert.equal(calls[1].parentTaskId, "main-expert-task");
    assert.equal(faux.state.callCount, 4);
    const text = events.filter((event) => event.event === "message_delta").map((event) => event.delta).join("");
    assert.match(text, /主 Agent 综合/);
    assert.doesNotMatch(text, /摄影专业结论：降低机位/);
  } finally {
    host.dispose();
  }
});

test("records a failed professional task when its Pi AgentSession cannot start", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-agent-host-expert-failure-"));
  const faux = fauxProvider({ provider: "workbench-expert-failure-test", tokensPerSecond: 0 });
  faux.setResponses([
    fauxAssistantMessage([
      fauxToolCall("call_expert", {
        expertType: "cinematography",
        task: "检查镜头04",
        focusRefs: [{ objectType: "shot", objectId: "shot04" }],
      }, { id: "call-broken-expert" }),
    ], { stopReason: "toolUse" }),
    fauxAssistantMessage("专业 Agent 启动失败，主 Agent 已明确报告。"),
  ]);
  const calls: string[] = [];
  const events: Record<string, unknown>[] = [];
  const host = await WorkbenchAgentHost.create(
    dataDir,
    (event) => events.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
    async (request) => {
      calls.push(request.toolName);
      if (request.toolName === "fail_expert") return { failed: true };
      return {
        data: {
          expertType: "cinematography",
          expertSessionId: "00000000-0000-4000-8000-000000000031",
          expertTaskId: "broken-expert-task",
          runtimeSessionId: "00000000-0000-4000-8000-000000000031",
          systemPrompt: "你是摄影 Agent。",
          allowedTools: ["read_shot_context"],
          provider: "missing-provider",
          model: "missing-model",
          thinkingLevel: "off",
          allowImages: false,
        },
      };
    },
  );
  const sessionId = "00000000-0000-4000-8000-000000000006";
  try {
    await host.handle({
      id: "create-failure-main",
      type: "create_session",
      sessionId,
      provider: faux.provider.id,
      model: faux.getModel().id,
    });
    await host.handle({
      id: "ask-failure-main",
      type: "send_message",
      sessionId,
      taskId: "main-failure-task",
      message: "请咨询摄影 Agent",
    });
    await waitForEvent(events, "task_completed", "main-failure-task");
    assert.deepEqual(calls, ["call_expert", "fail_expert"]);
  } finally {
    host.dispose();
  }
});

test("runs three independent professional AgentSessions in parallel with distinct tool loops", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-agent-host-team-"));
  const faux = fauxProvider({ provider: "workbench-team-test", tokensPerSecond: 0 });
  const respond = (context: any) => {
    const transcript = JSON.stringify(context.messages);
    const role = transcript.includes("writer-task")
      ? "writer"
      : transcript.includes("director-task")
        ? "director"
        : "cinematography";
    const tool = role === "writer"
      ? "read_story_structure"
      : role === "director"
        ? "read_scene"
        : "read_shot_context";
    assert.match(JSON.stringify(context.tools), new RegExp(tool));
    assert.doesNotMatch(JSON.stringify(context.tools), /call_expert/);
    if (!transcript.includes("projectRevision")) {
      const args = tool === "read_story_structure"
        ? { scopeType: "project" }
        : tool === "read_scene"
          ? { sceneId: "scene01" }
          : { shotId: "shot04" };
      return fauxAssistantMessage([fauxToolCall(tool, args, { id: `${role}-read` })], { stopReason: "toolUse" });
    }
    return fauxAssistantMessage(`${role} 独立专业意见`);
  };
  faux.setResponses([respond, respond, respond, respond, respond, respond]);
  const events: Record<string, unknown>[] = [];
  const calls: Array<{ sessionId: string; taskId: string; toolName: string }> = [];
  const host = await WorkbenchAgentHost.create(
    dataDir,
    (event) => events.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
    async (request) => {
      calls.push({ sessionId: request.sessionId, taskId: request.taskId, toolName: request.toolName });
      return { projectRevision: 9, data: { verifiedBy: request.toolName } };
    },
  );
  const roles = [
    { role: "writer", tool: "read_story_structure" },
    { role: "director", tool: "read_scene" },
    { role: "cinematography", tool: "read_shot_context" },
  ];
  try {
    for (const { role, tool } of roles) {
      await host.handle({
        id: `create-${role}`,
        type: "create_session",
        sessionId: `team-${role}-session`,
        provider: faux.provider.id,
        model: faux.getModel().id,
        systemPrompt: `你是 ${role} 专业 Agent，只能独立分析。`,
        allowedTools: [tool],
        allowCallExpert: false,
      });
    }
    await Promise.all(roles.map(({ role }) => host.handle({
      id: `send-${role}`,
      type: "send_message",
      sessionId: `team-${role}-session`,
      taskId: `team-${role}-task`,
      message: `${role}-task：独立分析同一场戏`,
    })));
    await Promise.all(roles.map(({ role }) => waitForEvent(events, "task_completed", `team-${role}-task`)));
    assert.equal(new Set(calls.map((call) => call.sessionId)).size, 3);
    assert.deepEqual(calls.map((call) => call.toolName).sort(), [
      "read_scene",
      "read_shot_context",
      "read_story_structure",
    ]);
    for (const { role } of roles) {
      const text = events
        .filter((event) => event.event === "message_delta" && event.taskId === `team-${role}-task`)
        .map((event) => event.delta)
        .join("");
      assert.match(text, new RegExp(`${role} 独立专业意见`));
    }
    assert.equal(faux.state.callCount, 6);
  } finally {
    host.dispose();
  }
});

test("lists ModelRuntime capabilities and manages provider API-key login", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-model-runtime-"));
  const host = await WorkbenchAgentHost.create(dataDir, () => {});
  try {
    const before = await host.handle({ id: "models-before", type: "get_models" });
    const providers = (before as { providers: Array<{ id: string; models: Array<{ supportsVision: boolean }> }> }).providers;
    assert.ok(providers.length > 0);
    assert.ok(providers.some((provider) => provider.models.length > 0));
    assert.ok(providers.flatMap((provider) => provider.models).every((model) => typeof model.supportsVision === "boolean"));

    const provider = providers.find((candidate) => candidate.id === "anthropic") ?? providers[0];
    await host.handle({ id: "login", type: "login_provider", providerId: provider.id, apiKey: "test-api-key" });
    const loggedIn = await host.handle({ id: "models-after", type: "get_models" });
    assert.equal(
      (loggedIn as { providers: Array<{ id: string; authConfigured: boolean }> }).providers.find((candidate) => candidate.id === provider.id)?.authConfigured,
      true,
    );
    await host.handle({ id: "logout", type: "logout_provider", providerId: provider.id });
  } finally {
    host.dispose();
    await rm(dataDir, { recursive: true, force: true });
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
