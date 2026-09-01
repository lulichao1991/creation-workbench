import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fauxAssistantMessage, fauxProvider, fauxToolCall, type Provider } from "@earendil-works/pi-ai";

import { EncryptedFileCredentialStore } from "./credentials.js";
import { WorkbenchAgentHost } from "./runtime.js";

test("creates an isolated real Pi SDK session without builtin tools", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-agent-host-"));
  const host = await WorkbenchAgentHost.create(dataDir, () => {});
  try {
    const doctor = host.doctor();
    assert.equal(doctor.healthy, true);
    assert.equal(doctor.agentHostHealthy, true);
    assert.equal(doctor.modelRuntimeHealthy, true);
    assert.equal(doctor.toolGatewayHealthy, false);
    assert.match(String(doctor.sdkVersion), /^0\.84\./);
    const created = await host.handle({ id: "create", type: "create_session", sessionId: "00000000-0000-4000-8000-000000000001" });
    assert.equal((created as Record<string, unknown>).runtimeSessionId, "00000000-0000-4000-8000-000000000001");
    assert.deepEqual(host.doctor().sessionHealth, { active: 1, busy: 0 });
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

test("Goal34 main Agent reads 30-episode structure and continues episode-10 discussion with memory", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-goal34-story-"));
  const faux = fauxProvider({ provider: "workbench-goal34-story-test", tokensPerSecond: 0 });
  faux.setResponses([
    fauxAssistantMessage([
      fauxToolCall("read_story_structure", { scopeType: "project", limit: 30 }, { id: "read-30-episodes" }),
    ]),
    (context) => {
      assert.match(JSON.stringify(context.messages), /第30集/);
      return fauxAssistantMessage([
        fauxToolCall("read_active_memories", {}, { id: "read-project-memory" }),
      ]);
    },
    (context) => {
      assert.match(JSON.stringify(context.messages), /保持第10集线索提前但不改变结局/);
      return fauxAssistantMessage("已核对30集结构：第10集的线索可以提前到第8集铺垫，结局不变。");
    },
    (context) => {
      const transcript = JSON.stringify(context.messages);
      assert.match(transcript, /已核对30集结构/);
      assert.match(transcript, /把第10集提前一点/);
      return fauxAssistantMessage("沿用上一轮：将第10集关键事件提前至第9集，保留第8集铺垫和原结局。");
    },
  ]);
  const tools: string[] = [];
  const events: Record<string, unknown>[] = [];
  const host = await WorkbenchAgentHost.create(
    dataDir,
    (event) => events.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
    async (request) => {
      tools.push(request.toolName);
      if (request.toolName === "read_story_structure") {
        return { projectRevision: 12, data: { episodes: Array.from({ length: 30 }, (_, index) => ({ id: `ep-${index + 1}`, name: `第${index + 1}集` })) } };
      }
      return { projectRevision: 12, data: [{ content: "保持第10集线索提前但不改变结局" }] };
    },
  );
  const sessionId = "00000000-0000-4000-8000-000000000034";
  try {
    await host.handle({ id: "goal34-create", type: "create_session", sessionId, provider: faux.provider.id, model: faux.getModel().id });
    await host.handle({ id: "goal34-first", type: "send_message", sessionId, taskId: "goal34-turn-1", message: "讨论《智斗游戏》30集结构" });
    await waitForEvent(events, "task_completed", "goal34-turn-1");
    await host.handle({ id: "goal34-second", type: "send_message", sessionId, taskId: "goal34-turn-2", message: "把第10集提前一点" });
    await waitForEvent(events, "task_completed", "goal34-turn-2");
    assert.deepEqual(tools, ["read_story_structure", "read_active_memories"]);
    assert.equal(faux.state.callCount, 4);
  } finally {
    host.dispose();
    await rm(dataDir, { recursive: true, force: true });
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

test("uses TypeBox submit_agent_result even when final model text is not JSON", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-structured-result-"));
  const faux = fauxProvider({ provider: "workbench-structured-result-test", tokensPerSecond: 0 });
  faux.setResponses([
    (context) => {
      assert.match(JSON.stringify(context.tools), /submit_agent_result/);
      return fauxAssistantMessage([fauxToolCall("submit_agent_result", {
        summary: "结构化修改提案",
        findings: ["镜头主体过小"],
        patchProposal: {
          title: "调整构图",
          items: [{
            objectType: "shot",
            objectId: "shot04",
            fieldName: "composition",
            oldValue: "平视中景",
            newValue: "低机位近景",
            reason: "增强压迫感",
          }],
        },
        relatedImpacts: [],
        permissionRequests: [],
        questions: [],
        risks: [],
        expertTeamSuggestion: null,
      }, { id: "submit-main-result" })], { stopReason: "toolUse" });
    },
    fauxAssistantMessage("下面是结果，已经提交；这段自由文本故意不是 JSON。"),
  ]);
  const events: Record<string, unknown>[] = [];
  const host = await WorkbenchAgentHost.create(
    dataDir,
    (event) => events.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
  );
  try {
    await host.handle({ id: "create", type: "create_session", sessionId: "structured-session", provider: faux.provider.id, model: faux.getModel().id });
    await host.handle({ id: "send", type: "send_message", sessionId: "structured-session", taskId: "structured-task", message: "提出修改" });
    await waitForEvent(events, "task_completed", "structured-task");
    const structured = events.find((event) => event.event === "structured_result" && event.taskId === "structured-task");
    assert.equal((structured?.result as { summary: string }).summary, "结构化修改提案");
    assert.equal(((structured?.result as { patchProposal: { items: unknown[] } }).patchProposal.items).length, 1);
  } finally {
    host.dispose();
    await rm(dataDir, { recursive: true, force: true });
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

test("prompt AgentSession calls deterministic compile_prompt_preview without video tools", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-prompt-preview-"));
  const faux = fauxProvider({ provider: "workbench-prompt-preview-test", tokensPerSecond: 0 });
  faux.setResponses([
    fauxAssistantMessage([
      fauxToolCall("compile_prompt_preview", { generationTaskId: "generation" }, { id: "compile-preview" }),
    ]),
    fauxAssistantMessage([{ type: "text", text: "{\"summary\":\"提示词预览已分析\",\"patchProposal\":null}" }]),
  ]);
  const calls: string[] = [];
  const events: Record<string, unknown>[] = [];
  const host = await WorkbenchAgentHost.create(
    dataDir,
    (event) => events.push(event),
    (runtime) => runtime.registerNativeProvider(faux.provider),
    async (request) => {
      calls.push(request.toolName);
      return { projectRevision: 11, data: { compiledPrompt: "镜头04提示词", persisted: false, videoGenerationCalled: false } };
    },
  );
  try {
    await host.handle({
      id: "prompt-session",
      type: "create_session",
      sessionId: "prompt-agent",
      provider: faux.provider.id,
      model: faux.getModel().id,
      allowedTools: ["compile_prompt_preview"],
      allowCallExpert: false,
      thinkingLevel: "off",
      systemPrompt: "你是提示词 Agent，只能调用确定性编译预览，不得调用视频生成。",
    });
    await host.handle({ id: "prompt-message", type: "send_message", sessionId: "prompt-agent", taskId: "prompt-task", message: "编译并分析预览" });
    await waitForEvent(events, "task_completed", "prompt-task");
    assert.deepEqual(calls, ["compile_prompt_preview"]);
    assert.doesNotMatch(JSON.stringify(events), /video_generate|generate_video|video_job/);
  } finally {
    host.dispose();
    await rm(dataDir, { recursive: true, force: true });
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

test("maps Pi OAuth notify and prompt channels without mixing them", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-oauth-flow-"));
  const faux = fauxProvider({ provider: "workbench-oauth", tokensPerSecond: 0 });
  const provider: Provider = {
    ...faux.provider,
    auth: {
      oauth: {
        name: "Workbench subscription",
        loginLabel: "登录 Workbench",
        isSubscription: true,
        async login(interaction) {
          interaction.notify({ type: "auth_url", url: "https://example.com/oauth", instructions: "在浏览器中继续" });
          const code = await interaction.prompt({ type: "manual_code", message: "粘贴授权码" });
          assert.equal(code, "approved-code");
          return { type: "oauth", access: "access", refresh: "refresh", expires: Date.now() + 3_600_000 };
        },
        async refresh(credential) { return credential; },
        async toAuth(credential) { return { apiKey: credential.access }; },
      },
    },
  };
  const host = await WorkbenchAgentHost.create(dataDir, () => {}, (runtime) => runtime.registerNativeProvider(provider));
  try {
    const catalog = await host.handle({ id: "catalog", type: "get_models" }) as { providers: Array<{ id: string; authMethods: Array<{ type: string; interactive: boolean; subscription?: boolean }> }> };
    assert.deepEqual(catalog.providers.find((item) => item.id === provider.id)?.authMethods, [{
      type: "oauth",
      interactive: true,
      label: "登录 Workbench",
      subscription: true,
    }]);
    const started = await host.handle({ id: "start", type: "auth_start", providerId: provider.id, authType: "oauth" }) as { flowId: string };
    const waiting = await waitForAuth(host, started.flowId, (flow) => Boolean(flow.prompt));
    assert.equal(waiting.notifications[0]?.type, "auth_url");
    assert.equal(waiting.prompt?.type, "manual_code");
    await host.handle({ id: "respond", type: "auth_respond", flowId: started.flowId, promptId: waiting.prompt!.id, value: "approved-code" });
    const completed = await waitForAuth(host, started.flowId, (flow) => flow.status === "completed");
    assert.equal(completed.status, "completed");
  } finally {
    host.dispose();
    await rm(dataDir, { recursive: true, force: true });
  }
});

test("closes an OAuth prompt when Pi cancels that prompt after an out-of-band callback", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-oauth-prompt-cancel-"));
  const faux = fauxProvider({ provider: "workbench-oauth-prompt-cancel", tokensPerSecond: 0 });
  const provider: Provider = {
    ...faux.provider,
    auth: {
      oauth: {
        name: "Workbench callback race",
        async login(interaction) {
          const promptAbort = new AbortController();
          setTimeout(() => promptAbort.abort(new Error("浏览器回调已完成")), 20);
          await assert.rejects(
            interaction.prompt({ type: "manual_code", message: "粘贴授权码", signal: promptAbort.signal }),
            { name: "AbortError" },
          );
          return { type: "oauth", access: "callback-access", refresh: "callback-refresh", expires: Date.now() + 3_600_000 };
        },
        async refresh(credential) { return credential; },
        async toAuth(credential) { return { apiKey: credential.access }; },
      },
    },
  };
  const host = await WorkbenchAgentHost.create(dataDir, () => {}, (runtime) => runtime.registerNativeProvider(provider));
  try {
    const started = await host.handle({ id: "start", type: "auth_start", providerId: provider.id, authType: "oauth" }) as { flowId: string };
    const waiting = await waitForAuth(host, started.flowId, (flow) => Boolean(flow.prompt));
    const promptId = waiting.prompt!.id;
    const completed = await waitForAuth(host, started.flowId, (flow) => flow.status === "completed");
    assert.equal(completed.prompt, null);
    assert.deepEqual(completed.cancelledPromptIds, [promptId]);
  } finally {
    host.dispose();
    await rm(dataDir, { recursive: true, force: true });
  }
});

test("cancels an OAuth login and closes its pending prompt", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-oauth-user-cancel-"));
  const faux = fauxProvider({ provider: "workbench-oauth-user-cancel", tokensPerSecond: 0 });
  const provider: Provider = {
    ...faux.provider,
    auth: {
      oauth: {
        name: "Workbench cancellable login",
        async login(interaction) {
          await interaction.prompt({ type: "manual_code", message: "粘贴授权码" });
          return { type: "oauth", access: "unused", refresh: "unused", expires: Date.now() + 3_600_000 };
        },
        async refresh(credential) { return credential; },
        async toAuth(credential) { return { apiKey: credential.access }; },
      },
    },
  };
  const host = await WorkbenchAgentHost.create(dataDir, () => {}, (runtime) => runtime.registerNativeProvider(provider));
  try {
    const started = await host.handle({ id: "start", type: "auth_start", providerId: provider.id, authType: "oauth" }) as { flowId: string };
    const waiting = await waitForAuth(host, started.flowId, (flow) => Boolean(flow.prompt));
    const promptId = waiting.prompt!.id;
    await host.handle({ id: "cancel", type: "auth_cancel", flowId: started.flowId });
    const cancelled = await waitForAuth(host, started.flowId, (flow) => flow.status === "cancelled");
    assert.equal(cancelled.prompt, null);
    assert.deepEqual(cancelled.cancelledPromptIds, [promptId]);
  } finally {
    host.dispose();
    await rm(dataDir, { recursive: true, force: true });
  }
});

test("refreshes and persists an expired OAuth credential", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-oauth-refresh-"));
  const faux = fauxProvider({ provider: "workbench-oauth-refresh", tokensPerSecond: 0 });
  faux.setResponses([fauxAssistantMessage("first"), fauxAssistantMessage("second")]);
  const credentialKey = Buffer.alloc(32, 9).toString("base64");
  let refreshCount = 0;
  const provider: Provider = {
    ...faux.provider,
    auth: {
      oauth: {
        name: "Workbench expiring login",
        async login() {
          return { type: "oauth", access: "expired", refresh: "rotate-me", expires: Date.now() - 1 };
        },
        async refresh() {
          refreshCount += 1;
          return { type: "oauth", access: "fresh", refresh: "rotated", expires: Date.now() + 3_600_000 };
        },
        async toAuth(credential) { return { apiKey: credential.access }; },
      },
    },
  };
  const firstEvents: Record<string, unknown>[] = [];
  let host = await WorkbenchAgentHost.create(dataDir, (event) => firstEvents.push(event), (runtime) => runtime.registerNativeProvider(provider), undefined, EncryptedFileCredentialStore.fromBase64(dataDir, credentialKey));
  try {
    const started = await host.handle({ id: "start", type: "auth_start", providerId: provider.id, authType: "oauth" }) as { flowId: string };
    await waitForAuth(host, started.flowId, (flow) => flow.status === "completed");
    await host.handle({ id: "create-first", type: "create_session", sessionId: "oauth-refresh-first", provider: provider.id, model: faux.getModel().id });
    await host.handle({ id: "send-first", type: "send_message", sessionId: "oauth-refresh-first", taskId: "oauth-refresh-task-first", message: "test" });
    await waitForEvent(firstEvents, "task_completed", "oauth-refresh-task-first");
    assert.equal(refreshCount, 1);

    host.dispose();
    const secondEvents: Record<string, unknown>[] = [];
    host = await WorkbenchAgentHost.create(dataDir, (event) => secondEvents.push(event), (runtime) => runtime.registerNativeProvider(provider), undefined, EncryptedFileCredentialStore.fromBase64(dataDir, credentialKey));
    await host.handle({ id: "create-second", type: "create_session", sessionId: "oauth-refresh-second", provider: provider.id, model: faux.getModel().id });
    await host.handle({ id: "send-second", type: "send_message", sessionId: "oauth-refresh-second", taskId: "oauth-refresh-task-second", message: "test again" });
    await waitForEvent(secondEvents, "task_completed", "oauth-refresh-task-second");
    assert.equal(refreshCount, 1);
  } finally {
    host.dispose();
    await rm(dataDir, { recursive: true, force: true });
  }
});

test("reports wrong and correct interactive API keys", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-api-key-check-"));
  const faux = fauxProvider({ provider: "workbench-api-key-check", tokensPerSecond: 0 });
  const provider: Provider = {
    ...faux.provider,
    auth: {
      apiKey: {
        name: "Workbench API key",
        async login(interaction) {
          return { type: "api_key", key: await interaction.prompt({ type: "secret", message: "输入 API Key" }) };
        },
        async check({ credential }) {
          return credential?.key === "correct-key" ? { type: "api_key", source: "Stored API key" } : undefined;
        },
        async resolve({ credential }) {
          return credential?.key === "correct-key" ? { auth: { apiKey: credential.key }, source: "Stored API key" } : undefined;
        },
      },
    },
  };
  const host = await WorkbenchAgentHost.create(dataDir, () => {}, (runtime) => runtime.registerNativeProvider(provider));
  try {
    for (const [key, healthy] of [["wrong-key", false], ["correct-key", true]] as const) {
      const started = await host.handle({ id: `start-${key}`, type: "auth_start", providerId: provider.id, authType: "api_key" }) as { flowId: string };
      const waiting = await waitForAuth(host, started.flowId, (flow) => Boolean(flow.prompt));
      await host.handle({ id: `respond-${key}`, type: "auth_respond", flowId: started.flowId, promptId: waiting.prompt!.id, value: key });
      await waitForAuth(host, started.flowId, (flow) => flow.status === "completed");
      const tested = await host.handle({ id: `test-${key}`, type: "test_provider", providerId: provider.id }) as { healthy: boolean };
      assert.equal(tested.healthy, healthy);
    }
  } finally {
    host.dispose();
    await rm(dataDir, { recursive: true, force: true });
  }
});

test("writes private Pi models.json and reloads custom providers locally", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-models-json-"));
  let host = await WorkbenchAgentHost.create(dataDir, () => {});
  try {
    await host.handle({
      id: "save-custom",
      type: "save_custom_provider",
      providerId: "custom-local-test",
      provider: {
        name: "Local Test",
        baseUrl: "http://127.0.0.1:11434/v1",
        api: "openai-completions",
        apiKey: "workbench-local",
        authHeader: false,
        models: [{ id: "local-model", name: "Local Model", reasoning: false, input: ["text"] }],
      },
    });
    const config = JSON.parse(await readFile(path.join(dataDir, "models.json"), "utf8"));
    assert.equal(config.providers["custom-local-test"].apiKey, "workbench-local");
    const catalog = await host.handle({ id: "models", type: "get_models" }) as { providers: Array<{ id: string; custom: boolean; models: Array<{ supportedThinkingLevels: string[] }> }> };
    const custom = catalog.providers.find((provider) => provider.id === "custom-local-test");
    assert.equal(custom?.custom, true);
    assert.deepEqual(custom?.models[0]?.supportedThinkingLevels, ["off"]);

    host.dispose();
    host = await WorkbenchAgentHost.create(dataDir, () => {});
    const afterRestart = await host.handle({ id: "models-after-restart", type: "get_models" }) as { providers: Array<{ id: string; authConfigured: boolean }> };
    assert.equal(afterRestart.providers.find((provider) => provider.id === "custom-local-test")?.authConfigured, true);

    await host.handle({ id: "delete-custom", type: "delete_custom_provider", providerId: "custom-local-test" });
    const after = await host.handle({ id: "models-after-delete", type: "get_models" }) as { providers: Array<{ id: string }> };
    assert.equal(after.providers.some((provider) => provider.id === "custom-local-test"), false);
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

interface AuthSnapshot {
  status: string;
  notifications: Array<{ type: string }>;
  prompt: { id: string; type: string } | null;
  cancelledPromptIds: string[];
}

async function waitForAuth(
  host: WorkbenchAgentHost,
  flowId: string,
  predicate: (flow: AuthSnapshot) => boolean,
): Promise<AuthSnapshot> {
  const deadline = Date.now() + 2_000;
  while (Date.now() < deadline) {
    const flow = await host.handle({ id: crypto.randomUUID(), type: "auth_flow_get", flowId }) as AuthSnapshot;
    if (predicate(flow)) return flow;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.fail("认证流程未进入预期状态");
}
