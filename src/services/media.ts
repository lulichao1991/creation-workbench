import { api } from "../api";

const mediaCache = new Map<string, Promise<string>>();
const MAX_MEDIA_CACHE_ENTRIES = 256;

export function loadProjectMediaDataUrl(projectPath: string, relativePath: string): Promise<string> {
  const key = `${projectPath}\n${relativePath}`;
  const cached = mediaCache.get(key);
  if (cached) return cached;
  if (mediaCache.size >= MAX_MEDIA_CACHE_ENTRIES) {
    const oldest = mediaCache.keys().next().value;
    if (oldest) mediaCache.delete(oldest);
  }
  const request = api.readProjectMedia(projectPath, relativePath)
    .then((result) => `data:${result.mimeType};base64,${result.data}`)
    .catch((error) => { mediaCache.delete(key); throw error; });
  mediaCache.set(key, request);
  return request;
}
