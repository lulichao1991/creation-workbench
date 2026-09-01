import { copyFile, mkdir, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const hostEntry = path.join(root, "agent-host", "dist", "index.js");
const hostModules = path.join(root, "agent-host", "node_modules", "@earendil-works", "pi-coding-agent");
const runtimeDir = path.join(root, "src-tauri", "resources", "agent-host");
const runtimeExecutable = path.join(runtimeDir, process.platform === "win32" ? "node.exe" : "node");

await Promise.all([stat(hostEntry), stat(hostModules)]);
await mkdir(runtimeDir, { recursive: true });
await copyFile(process.execPath, runtimeExecutable);

process.stdout.write(`Prepared bundled Agent Host runtime: ${runtimeExecutable}\n`);
