export type JsonObject = Record<string, unknown>;

export interface HostRequest extends JsonObject {
  id: string;
  type: string;
}

export interface HostResponse extends JsonObject {
  id: string;
  type: "response";
  success: boolean;
  result?: unknown;
  error?: string;
}

export interface HostEvent extends JsonObject {
  type: "event";
  event: string;
  sessionId?: string;
  taskId?: string;
}

export function parseRequest(line: string): HostRequest {
  const value: unknown = JSON.parse(line);
  if (!isObject(value) || typeof value.id !== "string" || typeof value.type !== "string") {
    throw new Error("请求必须包含字符串 id 和 type");
  }
  return value as HostRequest;
}

export function response(id: string, result?: unknown): HostResponse {
  return { id, type: "response", success: true, result };
}

export function failure(id: string, error: unknown): HostResponse {
  return {
    id,
    type: "response",
    success: false,
    error: error instanceof Error ? error.message : String(error),
  };
}

export function encodeJsonLine(value: unknown): string {
  return `${JSON.stringify(value)}\n`;
}

export function splitJsonLines(buffer: string, chunk: Buffer): { lines: string[]; rest: string } {
  const combined = buffer + chunk.toString("utf8");
  const records = combined.split("\n");
  const rest = records.pop() ?? "";
  return {
    lines: records.map((line) => line.endsWith("\r") ? line.slice(0, -1) : line).filter(Boolean),
    rest,
  };
}

function isObject(value: unknown): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
