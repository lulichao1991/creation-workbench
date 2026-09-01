import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { mkdtemp } from "node:fs/promises";
import test from "node:test";

import { EncryptedFileCredentialStore } from "./credentials.js";

test("persists Pi credentials encrypted and serializes read-modify-write", async () => {
  const dataDir = await mkdtemp(path.join(tmpdir(), "workbench-credentials-"));
  const key = Buffer.alloc(32, 7).toString("base64");
  const store = EncryptedFileCredentialStore.fromBase64(dataDir, key);
  const secondProcessStore = EncryptedFileCredentialStore.fromBase64(dataDir, key);
  await store.modify("openai-codex", async () => ({
    type: "oauth",
    access: "secret-access-token",
    refresh: "secret-refresh-token",
    expires: Date.now() + 60_000,
  }));
  const stored = await store.read("openai-codex");
  assert.equal(stored?.type, "oauth");
  assert.deepEqual(await store.list(), [{ providerId: "openai-codex", type: "oauth" }]);
  const encrypted = await readFile(path.join(dataDir, "credentials.enc"), "utf8");
  assert.equal(encrypted.includes("secret-access-token"), false);
  assert.equal((await secondProcessStore.read("openai-codex"))?.type, "oauth");

  await Promise.all([
    store.modify("openai", async () => ({ type: "api_key", key: "first" })),
    secondProcessStore.modify("anthropic", async () => ({ type: "api_key", key: "second" })),
  ]);
  assert.equal((await store.read("openai"))?.type, "api_key");
  assert.equal((await store.read("anthropic"))?.type, "api_key");
  await store.delete("openai-codex");
  assert.equal(await store.read("openai-codex"), undefined);
});
