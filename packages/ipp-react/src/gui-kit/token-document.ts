/** A stable application identity occupies one indivisible editing position. */
export type TokenPart =
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "token"; readonly id: string; readonly label: string };
export type TokenDocument = readonly TokenPart[];
export type TokenSelection = readonly [anchor: number, caret: number];
export interface TokenClipboardData {
  readonly text: string;
  readonly structured?: string;
}

const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });

/** Validate external application data, drop empty text and coalesce neighbouring runs. */
export function normalizeTokenDocument(document: TokenDocument): TokenDocument {
  if (!Array.isArray(document))
    throw new TypeError("Expected a token document");
  const normalized: TokenPart[] = [];
  const ids = new Set<string>();
  for (const part of document) {
    if (part?.kind === "text" && typeof part.text === "string") {
      if (!part.text) continue;
      if (/[\r\n]/.test(part.text))
        throw new TypeError("Token input accepts a single line");
      const last = normalized.at(-1);
      if (last?.kind === "text")
        normalized[normalized.length - 1] = {
          kind: "text",
          text: last.text + part.text,
        };
      else normalized.push({ kind: "text", text: part.text });
    } else if (
      part?.kind === "token" &&
      typeof part.id === "string" &&
      part.id.length &&
      typeof part.label === "string" &&
      !/[\r\n]/.test(part.label) &&
      !ids.has(part.id)
    ) {
      ids.add(part.id);
      normalized.push({ kind: "token", id: part.id, label: part.label });
    } else
      throw new TypeError(
        "Expected a text run or a single token with a unique nonempty id",
      );
  }
  return normalized;
}

/** UTF-16 and UTF-8 offsets do not escape the JS editor. Selection uses grapheme/token units. */
export function tokenUnits(document: TokenDocument): TokenPart[] {
  return document.flatMap((part): TokenPart[] =>
    part.kind === "token"
      ? [part]
      : Array.from(segmenter.segment(part.text), ({ segment }) => ({
          kind: "text",
          text: segment,
        })),
  );
}

function plainTokenText(parts: TokenDocument): string {
  return parts
    .map((part) => (part.kind === "text" ? part.text : part.label))
    .join("");
}

export function serializeTokenDocument(
  document: TokenDocument,
): TokenClipboardData {
  const parts = normalizeTokenDocument(document);
  return {
    text: plainTokenText(parts),
    structured: JSON.stringify({ version: 1, parts }),
  };
}

/** Structured tokens must also satisfy the importing application's policy. */
export function importTokenDocument(
  data: TokenClipboardData,
  acceptsToken?: (token: Extract<TokenPart, { kind: "token" }>) => boolean,
): TokenDocument {
  if (data.structured && acceptsToken) {
    try {
      const decoded = JSON.parse(data.structured);
      if (decoded?.version === 1) {
        const parts = normalizeTokenDocument(decoded.parts);
        if (
          plainTokenText(parts) === data.text &&
          parts.every((part) => part.kind === "text" || acceptsToken(part))
        )
          return parts;
      }
    } catch {
      /* Unknown formats use their ordinary clipboard text. */
    }
  }
  return normalizeTokenDocument([
    { kind: "text", text: data.text.replace(/[\r\n]+/g, " ") },
  ]);
}

export function replaceTokenSelection(
  document: TokenDocument,
  selection: TokenSelection,
  inserted: TokenDocument,
) {
  const units = tokenUnits(document);
  const [start, end] = [Math.min(...selection), Math.max(...selection)].map(
    (offset) => Math.min(units.length, Math.max(0, offset)),
  );
  const before = units.slice(0, start);
  const after = units.slice(end);
  // Refuse duplicate identities atomically. A same-document cut still preserves its id.
  const value = normalizeTokenDocument([...before, ...inserted, ...after]);
  const caret = tokenUnits(
    normalizeTokenDocument([...before, ...inserted]),
  ).length;
  return { value, selection: [caret, caret] as TokenSelection };
}
