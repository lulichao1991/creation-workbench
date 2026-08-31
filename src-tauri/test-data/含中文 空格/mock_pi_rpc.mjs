let buffer = "";
let timer;

function send(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

function handle(command) {
  if (command.type === "prompt") {
    send({ id: command.id, type: "response", command: "prompt", success: true });
    const delay = command.message === "slow" ? 2000 : 10;
    timer = setTimeout(() => {
      send({
        type: "message_update",
        assistantMessageEvent: { type: "text_delta", delta: `围绕${command.message}的只读回答` },
      });
      send({ type: "agent_end" });
    }, delay);
  } else if (command.type === "abort") {
    clearTimeout(timer);
    send({ id: command.id, type: "response", command: "abort", success: true });
    send({ type: "agent_end" });
  }
}

process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buffer += chunk;
  for (;;) {
    const newline = buffer.indexOf("\n");
    if (newline < 0) break;
    let line = buffer.slice(0, newline);
    buffer = buffer.slice(newline + 1);
    if (line.endsWith("\r")) line = line.slice(0, -1);
    if (line) handle(JSON.parse(line));
  }
});
