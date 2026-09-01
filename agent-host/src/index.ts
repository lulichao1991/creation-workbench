import path from "node:path";
import os from "node:os";

import { encodeJsonLine, failure, parseRequest, response, splitJsonLines, type HostEvent } from "./protocol.js";
import { WorkbenchAgentHost } from "./runtime.js";
import type { ToolGateway } from "./tools.js";

const systemDataDir = process.env.LOCALAPPDATA
  || process.env.XDG_DATA_HOME
  || path.join(os.homedir(), ".local", "share");
const dataDir = process.env.WORKBENCH_AGENT_DATA_DIR
  || path.join(systemDataDir, "creation-workbench", "agent-host");

function write(value: unknown): void {
  process.stdout.write(encodeJsonLine(value));
}

let gatewayRequestId = 0;
const pendingGateway = new Map<string, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();
const gateway: ToolGateway = (request, signal) => new Promise((resolve, reject) => {
  const id = `tool-${++gatewayRequestId}`;
  if (signal?.aborted) {
    reject(new Error("Tool Call 已取消"));
    return;
  }
  pendingGateway.set(id, { resolve, reject });
  signal?.addEventListener("abort", () => {
    if (pendingGateway.delete(id)) reject(new Error("Tool Call 已取消"));
  }, { once: true });
  write({ id, type: "tool_request", ...request });
});

const host = await WorkbenchAgentHost.create(dataDir, (event: HostEvent) => write(event), undefined, gateway);
let buffer = "";
let queue = Promise.resolve();

process.stdin.on("data", (chunk: Buffer) => {
  const parsed = splitJsonLines(buffer, chunk);
  buffer = parsed.rest;
  for (const line of parsed.lines) {
    queue = queue.then(async () => {
      let id = "unknown";
      try {
        const request = parseRequest(line);
        id = request.id;
        if (request.type === "tool_response") {
          const pending = pendingGateway.get(id);
          if (!pending) return;
          pendingGateway.delete(id);
          if (request.success === true) pending.resolve(request.result);
          else pending.reject(new Error(typeof request.error === "string" ? request.error : "Tool Gateway 请求失败"));
          return;
        }
        const result = await host.handle(request);
        write(response(id, result));
        if (request.type === "shutdown") process.exitCode = 0;
      } catch (error) {
        write(failure(id, error));
      }
    });
  }
});

process.stdin.on("end", () => {
  for (const pending of pendingGateway.values()) pending.reject(new Error("工作台已断开"));
  pendingGateway.clear();
  host.dispose();
});

process.on("SIGTERM", () => {
  host.dispose();
  process.exit(0);
});
