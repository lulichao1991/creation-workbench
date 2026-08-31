let buffer = "";
let timer;

function send(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

function answer(prompt) {
  if (prompt.includes("独立专家结果")) {
    return JSON.stringify({
      summary: "主 Agent 已综合三位专家的独立意见",
      consensus: ["场景目标清楚，但信息揭示节奏需要收紧"],
      disagreements: [{ topic: "镜头停留时长", positions: ["缩短", "保留用于建立空间"] }],
      recommendations: ["先确认叙事优先级，再另行建立修改提案"],
      questions: ["这一场更优先悬念还是空间建立？"],
      risks: [],
    });
  }
  return JSON.stringify({
    summary: "已完成本专业独立只读分析",
    findings: [{ topic: "节奏", position: "需要收紧信息揭示" }],
    patchProposal: null,
    relatedImpacts: [],
    permissionRequests: [],
    questions: [],
    risks: [],
  });
}

function handle(command) {
  if (command.type === "prompt") {
    send({ id: command.id, type: "response", command: "prompt", success: true });
    timer = setTimeout(() => {
      send({
        type: "message_update",
        assistantMessageEvent: { type: "text_delta", delta: answer(command.message) },
      });
      send({ type: "agent_end" });
    }, 80);
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
