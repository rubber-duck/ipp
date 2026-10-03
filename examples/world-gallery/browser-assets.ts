import type { GalleryAssets } from "./shared/scene.js";

/** Browser fetch owns platform reads; shared scene modules consume asset capabilities. */
export function browserGalleryAssets(base: URL): GalleryAssets {
  const url = (path: string) => new URL(path, base).href;
  async function response(path: string, signal?: AbortSignal) {
    const result = await fetch(url(path), signal ? { signal } : {});
    if (!result.ok)
      throw new Error(`Gallery asset ${path}: HTTP ${result.status}`);
    return result;
  }
  return {
    url,
    async prepare(_resources, signal) {
      signal?.throwIfAborted();
    },
    async readBytes(path, signal) {
      return new Uint8Array(await (await response(path, signal)).arrayBuffer());
    },
    async readJson<T>(path: string, signal?: AbortSignal): Promise<T> {
      return (await response(path, signal)).json() as Promise<T>;
    },
  };
}
