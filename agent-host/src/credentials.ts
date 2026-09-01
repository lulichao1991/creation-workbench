import { createCipheriv, createDecipheriv, randomBytes } from "node:crypto";
import { mkdir, open, readFile, rename, stat, unlink, writeFile } from "node:fs/promises";
import path from "node:path";

import type {
  AuthOperationOptions,
  Credential,
  CredentialInfo,
  CredentialStore,
} from "@earendil-works/pi-ai";

const FORMAT_VERSION = 1;
const AAD = Buffer.from("creation-workbench.credentials.v1", "utf8");
const LOCK_TIMEOUT_MS = 5_000;
const STALE_LOCK_MS = 30_000;

interface EncryptedEnvelope {
  version: 1;
  iv: string;
  tag: string;
  ciphertext: string;
}

type CredentialMap = Record<string, Credential>;

export class EncryptedFileCredentialStore implements CredentialStore {
  private queue: Promise<void> = Promise.resolve();

  constructor(
    private readonly filePath: string,
    private readonly key: Buffer,
  ) {
    if (key.length !== 32) throw new Error("Workbench 凭据主密钥必须为 32 字节");
  }

  static fromBase64(dataDir: string, encodedKey: string): EncryptedFileCredentialStore {
    return new EncryptedFileCredentialStore(
      path.join(dataDir, "credentials.enc"),
      Buffer.from(encodedKey, "base64"),
    );
  }

  async read(providerId: string, options?: AuthOperationOptions): Promise<Credential | undefined> {
    options?.signal?.throwIfAborted();
    const credential = (await this.readCredentials())[providerId];
    return credential ? structuredClone(credential) : undefined;
  }

  async list(options?: AuthOperationOptions): Promise<readonly CredentialInfo[]> {
    options?.signal?.throwIfAborted();
    return Object.entries(await this.readCredentials()).map(([providerId, credential]) => ({
      providerId,
      type: credential.type,
    }));
  }

  modify(
    providerId: string,
    update: (current: Credential | undefined) => Promise<Credential | undefined>,
    options?: AuthOperationOptions,
  ): Promise<Credential | undefined> {
    return this.serialized(async () => this.withFileLock(options?.signal, async () => {
      const credentials = await this.readCredentials();
      const current = credentials[providerId];
      const next = await update(current ? structuredClone(current) : undefined);
      options?.signal?.throwIfAborted();
      if (next !== undefined) {
        credentials[providerId] = structuredClone(next);
        await this.writeCredentials(credentials);
        return structuredClone(next);
      }
      return current ? structuredClone(current) : undefined;
    }));
  }

  delete(providerId: string, options?: AuthOperationOptions): Promise<void> {
    return this.serialized(async () => this.withFileLock(options?.signal, async () => {
      const credentials = await this.readCredentials();
      if (!(providerId in credentials)) return;
      delete credentials[providerId];
      await this.writeCredentials(credentials);
    }));
  }

  private serialized<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.queue.then(operation, operation);
    this.queue = result.then(() => {}, () => {});
    return result;
  }

  private async readCredentials(): Promise<CredentialMap> {
    let contents: string;
    try {
      contents = await readFile(this.filePath, "utf8");
    } catch (error) {
      if (isNodeError(error, "ENOENT")) return {};
      throw error;
    }
    const envelope = JSON.parse(contents) as EncryptedEnvelope;
    if (envelope.version !== FORMAT_VERSION) throw new Error("Workbench 凭据文件版本不受支持");
    const decipher = createDecipheriv("aes-256-gcm", this.key, Buffer.from(envelope.iv, "base64"));
    decipher.setAAD(AAD);
    decipher.setAuthTag(Buffer.from(envelope.tag, "base64"));
    const plaintext = Buffer.concat([
      decipher.update(Buffer.from(envelope.ciphertext, "base64")),
      decipher.final(),
    ]).toString("utf8");
    return parseCredentialMap(JSON.parse(plaintext));
  }

  private async writeCredentials(credentials: CredentialMap): Promise<void> {
    await mkdir(path.dirname(this.filePath), { recursive: true });
    const iv = randomBytes(12);
    const cipher = createCipheriv("aes-256-gcm", this.key, iv);
    cipher.setAAD(AAD);
    const ciphertext = Buffer.concat([
      cipher.update(JSON.stringify(credentials), "utf8"),
      cipher.final(),
    ]);
    const envelope: EncryptedEnvelope = {
      version: FORMAT_VERSION,
      iv: iv.toString("base64"),
      tag: cipher.getAuthTag().toString("base64"),
      ciphertext: ciphertext.toString("base64"),
    };
    const temporaryPath = `${this.filePath}.${process.pid}.${randomBytes(6).toString("hex")}.tmp`;
    try {
      await writeFile(temporaryPath, JSON.stringify(envelope), { encoding: "utf8", mode: 0o600 });
      await rename(temporaryPath, this.filePath);
    } finally {
      await unlink(temporaryPath).catch(() => {});
    }
  }

  private async withFileLock<T>(signal: AbortSignal | undefined, operation: () => Promise<T>): Promise<T> {
    const lockPath = `${this.filePath}.lock`;
    const startedAt = Date.now();
    let lock: Awaited<ReturnType<typeof open>> | undefined;
    while (!lock) {
      signal?.throwIfAborted();
      try {
        await mkdir(path.dirname(this.filePath), { recursive: true });
        lock = await open(lockPath, "wx", 0o600);
        await lock.writeFile(`${process.pid}\n${Date.now()}\n`);
      } catch (error) {
        if (!isNodeError(error, "EEXIST")) throw error;
        const age = await stat(lockPath).then((value) => Date.now() - value.mtimeMs).catch(() => 0);
        if (age > STALE_LOCK_MS) {
          await unlink(lockPath).catch(() => {});
          continue;
        }
        if (Date.now() - startedAt >= LOCK_TIMEOUT_MS) throw new Error("Workbench 凭据文件正被另一个进程使用");
        await delay(25, signal);
      }
    }
    try {
      return await operation();
    } finally {
      await lock.close().catch(() => {});
      await unlink(lockPath).catch(() => {});
    }
  }
}

function parseCredentialMap(value: unknown): CredentialMap {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Workbench 凭据文件内容无效");
  const credentials: CredentialMap = {};
  for (const [providerId, credential] of Object.entries(value)) {
    if (!credential || typeof credential !== "object" || Array.isArray(credential)) throw new Error(`Provider ${providerId} 的凭据无效`);
    const type = (credential as Record<string, unknown>).type;
    if (type !== "api_key" && type !== "oauth") throw new Error(`Provider ${providerId} 的凭据类型无效`);
    credentials[providerId] = credential as Credential;
  }
  return credentials;
}

function isNodeError(error: unknown, code: string): boolean {
  return error instanceof Error && "code" in error && (error as NodeJS.ErrnoException).code === code;
}

function delay(milliseconds: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, milliseconds);
    signal?.addEventListener("abort", () => {
      clearTimeout(timer);
      reject(signal.reason);
    }, { once: true });
  });
}
