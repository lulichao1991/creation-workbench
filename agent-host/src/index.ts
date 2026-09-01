import path from "node:path";
import os from "node:os";

import { encodeJsonLine, failure, parseRequest, response, splitJsonLines, type HostEvent } from "./protocol.js";
import { WorkbenchAgentHost } from "./runtime.js";

const systemDataDir = process.env.LOCALAPPDATA
  || process.env.XDG_DATA_HOME
  || path.join(os.homedir(), ".local", "share");
const dataDir = process.env.WORKBENCH_AGENT_DATA_DIR
  || path.join(systemDataDir, "creation-workbench", "agent-host");

function write(value: unknown): void {
  process.stdout.write(encodeJsonLine(value));
}

const host = await WorkbenchAgentHost.create(dataDir, (event: HostEvent) => write(event));
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
  host.dispose();
});

process.on("SIGTERM", () => {
  host.dispose();
  process.exit(0);
});
