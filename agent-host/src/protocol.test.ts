import assert from "node:assert/strict";
import test from "node:test";

import { encodeJsonLine, parseRequest, splitJsonLines } from "./protocol.js";

test("strict LF framing preserves unicode separators and partial records", () => {
  const first = Buffer.from('{"id":"1","type":"ping","text":"a\u2028b"}\n{"id":"2"');
  const parsed = splitJsonLines("", first);
  assert.equal(parsed.lines.length, 1);
  assert.equal(parseRequest(parsed.lines[0]).id, "1");
  const completed = splitJsonLines(parsed.rest, Buffer.from(',"type":"doctor"}\r\n'));
  assert.equal(parseRequest(completed.lines[0]).type, "doctor");
  assert.equal(completed.rest, "");
});

test("responses are one JSON object per line", () => {
  const encoded = encodeJsonLine({ type: "response", success: true });
  assert.equal(encoded.endsWith("\n"), true);
  assert.deepEqual(JSON.parse(encoded), { type: "response", success: true });
});
