export function toUserErrorMessage(reason: unknown): string {
  const message = reason instanceof Error ? reason.message : String(reason);
  if (/Pi SDK Agent Host.*(?:stopped|已停止)|broken pipe|pipe.*closed/i.test(message)) {
    return "Agent 服务刚刚中断，工作台会在下次请求时自动重启服务。请重试当前操作。";
  }
  if (/ModelRuntime/i.test(message)) {
    return "模型配置暂时无法读取。请检查 Agent 服务后重试。";
  }
  if (/Cannot read properties of undefined.*invoke|__TAURI__/i.test(message)) {
    return "当前页面未连接到桌面服务，请在创作工作台桌面应用中重试。";
  }
  if (/^[\u3400-\u9fff]/.test(message) && !/(Runtime|AgentSession|contentUnit|UUID|stack|invoke)/i.test(message)) {
    return message;
  }
  return "操作失败，请重试。若问题持续，请重新打开工作台。";
}
