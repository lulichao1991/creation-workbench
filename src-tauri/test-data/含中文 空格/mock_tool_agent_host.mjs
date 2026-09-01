let buffer = "";
let active = null;

function write(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

process.stdin.on("data", (chunk) => {
  buffer += chunk.toString("utf8");
  const lines = buffer.split("\n");
  buffer = lines.pop() ?? "";
  for (const line of lines) {
    if (!line.trim()) continue;
    const request = JSON.parse(line);
    if (request.type === "create_session") {
      write({ id: request.id, type: "response", success: true, result: { runtimeSessionId: request.sessionId } });
    } else if (request.type === "send_message") {
      active = request;
      write({ type: "event", event: "task_started", sessionId: request.sessionId, taskId: request.taskId });
      write({ type: "event", event: "tool_call_requested", sessionId: request.sessionId, taskId: request.taskId, toolCallId: "fixture-tool", toolName: "get_selection", arguments: {} });
      write({ id: "fixture-gateway", type: "tool_request", sessionId: request.sessionId, taskId: request.taskId, toolCallId: "fixture-tool", toolName: "get_selection", arguments: {} });
      write({ id: request.id, type: "response", success: true, result: { accepted: true } });
    } else if (request.type === "tool_response" && request.id === "fixture-gateway" && active) {
      write({ type: "event", event: "tool_call_completed", sessionId: active.sessionId, taskId: active.taskId, toolCallId: "fixture-tool", toolName: "get_selection", result: request.result, isError: !request.success });
      write({ type: "event", event: "message_delta", sessionId: active.sessionId, taskId: active.taskId, delta: JSON.stringify(request.result) });
      write({ type: "event", event: "task_completed", sessionId: active.sessionId, taskId: active.taskId });
    } else if (request.type === "shutdown") {
      write({ id: request.id, type: "response", success: true, result: { stopped: true } });
      process.exitCode = 0;
    } else {
      write({ id: request.id, type: "response", success: true, result: {} });
    }
  }
});
