import fs from "node:fs";

const marker = process.argv[2];
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
    if (request.type === "doctor" && !fs.existsSync(marker)) {
      fs.writeFileSync(marker, "crashed-once");
      process.stderr.write("fixture crash before response\n");
      process.exit(7);
    } else if (request.type === "doctor") {
      write({ id: request.id, type: "response", success: true, result: { healthy: true, agentHostHealthy: true, sdkVersion: "recovered-sdk", modelRuntimeHealthy: true, providerCount: 1, modelCount: 1, providerAuth: [], sessionHealth: { active: 0, busy: 0 }, toolGatewayHealthy: true } });
    } else if (request.type === "shutdown") {
      write({ id: request.id, type: "response", success: true, result: { stopped: true } });
      process.exitCode = 0;
    }
  }
});
