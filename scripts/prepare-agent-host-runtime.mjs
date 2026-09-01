import { copyFile, cp, mkdir, rm, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const hostEntry = path.join(root, "agent-host", "dist", "index.js");
const hostModules = path.join(root, "agent-host", "node_modules", "@earendil-works", "pi-coding-agent");
const runtimeDir = path.join(root, "agent-host-runtime");
const runtimeExecutable = path.join(runtimeDir, process.platform === "win32" ? "node.exe" : "node");
const runtimeModules = path.join(runtimeDir, "node_modules");
const sourceModules = path.join(root, "agent-host", "node_modules");
const sourceLock = path.join(sourceModules, ".package-lock.json");
const runtimeLock = path.join(runtimeModules, ".package-lock.json");

const [major, minor] = process.versions.node.split(".").map(Number);
if (major < 22 || (major === 22 && minor < 19)) {
  throw new Error(`Agent Host requires Node >=22.19.0, current build runtime is ${process.versions.node}`);
}

await Promise.all([stat(hostEntry), stat(hostModules)]);
await mkdir(runtimeDir, { recursive: true });

const sourceStat = await stat(process.execPath);
const targetStat = await stat(runtimeExecutable).catch(() => null);
const unchanged = targetStat?.size === sourceStat.size && targetStat.mtimeMs >= sourceStat.mtimeMs;
if (!unchanged) await copyFile(process.execPath, runtimeExecutable);

const sourceLockStat = await stat(sourceLock);
const runtimeLockStat = await stat(runtimeLock).catch(() => null);
const modulesUnchanged = runtimeLockStat?.size === sourceLockStat.size && runtimeLockStat.mtimeMs === sourceLockStat.mtimeMs;
if (!modulesUnchanged) {
  await rm(runtimeModules, { recursive: true, force: true });
  await cp(sourceModules, runtimeModules, {
    recursive: true,
    preserveTimestamps: true,
    filter(source) {
      const relative = path.relative(sourceModules, source);
      const first = relative.split(path.sep)[0];
      return ![".bin", "typescript", "@types"].includes(first);
    },
  });
}
await cp(path.join(root, "agent-host", "dist"), path.join(runtimeDir, "dist"), { recursive: true, force: true });
await copyFile(path.join(root, "agent-host", "package.json"), path.join(runtimeDir, "package.json"));

process.stdout.write(`${unchanged ? "Bundled Agent Host runtime unchanged" : "Prepared bundled Agent Host runtime"}; ${modulesUnchanged ? "production modules unchanged" : "prepared production-only modules"}: ${runtimeDir}\n`);
