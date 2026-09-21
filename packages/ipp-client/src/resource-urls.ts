/** Browser HTTP locations are connection configuration, separate from source identity. */
export interface ResourceUrlMapping {
  readonly prefix: string;
  readonly baseUrl: string;
}

/** Normalize and copy configuration before creating a worker or starting I/O. */
export function resourceUrlMappings(
  value: unknown,
): readonly ResourceUrlMapping[] {
  if (value === undefined) return [];
  if (!Array.isArray(value))
    throw new TypeError("resourceUrls must be an array");
  const mappings: ResourceUrlMapping[] = [];
  for (const entry of value) {
    if (!entry || typeof entry !== "object")
      throw new TypeError("resourceUrls entries require prefix and baseUrl");
    const prefix = directoryUrl(entry.prefix);
    const baseUrl = directoryUrl(entry.baseUrl);
    if (
      mappings.some(
        (mapping) =>
          prefix.startsWith(mapping.prefix) ||
          mapping.prefix.startsWith(prefix),
      )
    )
      throw new TypeError("resourceUrls prefixes must not overlap");
    mappings.push(Object.freeze({ prefix, baseUrl }));
  }
  return Object.freeze(mappings);
}

function directoryUrl(value: unknown): string {
  if (typeof value !== "string")
    throw new TypeError("resourceUrls URLs must be strings");
  const url = new URL(value);
  if (
    (url.protocol !== "http:" && url.protocol !== "https:") ||
    url.username ||
    url.password ||
    url.search ||
    url.hash ||
    !url.pathname.endsWith("/")
  )
    throw new TypeError(
      "resourceUrls URLs must be absolute HTTP(S) directories ending in / without credentials, query, or fragment",
    );
  return url.href;
}

export function resourceFetchUrl(
  source: URL,
  mappings: readonly ResourceUrlMapping[],
): string {
  const original = source.href;
  const mapping = mappings.find(({ prefix }) => original.startsWith(prefix));
  return mapping
    ? mapping.baseUrl + original.slice(mapping.prefix.length)
    : original;
}
