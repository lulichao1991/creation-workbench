let buffer = "";

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
    if (request.type === "doctor") {
      write({ id: request.id, type: "response", success: true, result: { healthy: true, sdkVersion: "mock-sdk" } });
    } else if (request.type === "create_session") {
      write({ id: request.id, type: "response", success: true, result: { runtimeSessionId: request.sessionId } });
    } else if (request.type === "send_message") {
      write({ type: "event", event: "task_started", sessionId: request.sessionId, taskId: request.taskId });
      write({ type: "event", event: "message_delta", sessionId: request.sessionId, taskId: request.taskId, delta: "Pi SDK " });
      write({ type: "event", event: "message_delta", sessionId: request.sessionId, taskId: request.taskId, delta: "原生会话" });
      write({ type: "event", event: "task_completed", sessionId: request.sessionId, taskId: request.taskId });
      write({ id: request.id, type: "response", success: true, result: { accepted: true } });
    } else if (request.type === "shutdown") {
      write({ id: request.id, type: "response", success: true, result: { stopped: true } });
      process.exitCode = 0;
    } else {
      write({ id: request.id, type: "response", success: true, result: {} });
    }
  }
});
