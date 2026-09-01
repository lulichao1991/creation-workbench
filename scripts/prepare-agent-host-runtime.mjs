import { copyFile, mkdir, stat } from "node:fs/promises";
import { createReadStream } from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const hostEntry = path.join(root, "agent-host", "dist", "index.js");
const hostModules = path.join(root, "agent-host", "node_modules", "@earendil-works", "pi-coding-agent");
const runtimeDir = path.join(root, "src-tauri", "resources", "agent-host");
const runtimeExecutable = path.join(runtimeDir, process.platform === "win32" ? "node.exe" : "node");

await Promise.all([stat(hostEntry), stat(hostModules)]);
await mkdir(runtimeDir, { recursive: true });

const hashFile = (file) => new Promise((resolve, reject) => {
  const hash = createHash("sha256");
  createReadStream(file).on("error", reject).on("data", (chunk) => hash.update(chunk)).on("end", () => resolve(hash.digest("hex")));
});
const sourceStat = await stat(process.execPath);
const targetStat = await stat(runtimeExecutable).catch(() => null);
const unchanged = targetStat?.size === sourceStat.size && await hashFile(runtimeExecutable) === await hashFile(process.execPath);
if (!unchanged) await copyFile(process.execPath, runtimeExecutable);

process.stdout.write(`${unchanged ? "Bundled Agent Host runtime unchanged" : "Prepared bundled Agent Host runtime"}: ${runtimeExecutable}\n`);
