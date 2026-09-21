/** Pure GUI snapshot description and logical action dispatch.
 *
 * Everything here runs at render time and performs no transport. The commit
 * phase consumes these snapshots through the generated GUI client.
 */
import type { GuiAssetSource, GuiNodeContent, GuiNodeStyle } from "@ipp/client";
import type {
  GuiActionEvent,
  GuiActionListener,
  GuiDeclarationStyle,
  GuiHostType,
  GuiNodeRef,
} from "./components.js";
import type { GuiControlTheme } from "./theme.js";

/** One declared GUI node in visit order (parents before children). */
export interface GuiDescribedNode {
  /** Retained reconciler instance identity; stable across keyed reorders. */
  readonly identity: number;
  /** Parent instance identity, or undefined for the root node. */
  readonly parent: number | undefined;
  readonly type: GuiHostType;
  readonly content: GuiNodeContent;
  /** Complete style with lane defaults filled, including asset and enabled. */
  readonly style: GuiDeclarationStyle;
  readonly nodeRef: GuiNodeRef | null;
  readonly onAction: GuiActionListener | undefined;
  readonly onActionCapture: GuiActionListener | undefined;
  readonly theme?: GuiControlTheme | undefined;
}

/** One declared GuiRoot and its node subtree. */
export interface GuiDescribedRoot {
  /** GuiRoot instance identity. */
  readonly identity: number;
  /** Enclosing Entity instance identity. */
  readonly entity: number;
  /** Resolves to the acknowledged root-node handle after acknowledgement. */
  readonly nodeRef: GuiNodeRef | null;
  readonly nodes: readonly GuiDescribedNode[];
  readonly signature: string;
}

/** Fill lane defaults the runtime applies, so sparse reads compare equal. */
export function normalizeGuiStyle(
  style: GuiNodeStyle | GuiDeclarationStyle,
): GuiDeclarationStyle {
  return {
    ...style,
    color: style.color === undefined ? [1, 1, 1, 1] : [...style.color],
    opacity: style.opacity ?? 1,
    fontSize: style.fontSize ?? 0.1,
    enabled: (style as GuiDeclarationStyle).enabled ?? true,
  };
}

/** Commit signature for one asset lane: absent, cleared and bound differ. */
function assetSignature(asset: GuiAssetSource | null | undefined): unknown {
  if (asset === undefined) return null;
  if (asset === null) return [null];
  return [asset.kind, asset.source, asset.variant ?? 0];
}

function styleSignature(input: GuiNodeStyle | GuiDeclarationStyle): unknown {
  const style = normalizeGuiStyle(input);
  const color = style.color ?? [1, 1, 1, 1];
  return [
    style.width ?? null,
    style.height ?? null,
    style.minWidth ?? null,
    style.minHeight ?? null,
    style.maxWidth ?? null,
    style.maxHeight ?? null,
    style.padding === undefined ? null : [...style.padding],
    style.margin === undefined ? null : [...style.margin],
    style.flex ?? null,
    style.alignX ?? null,
    style.alignY ?? null,
    [...color],
    style.backgroundColor === undefined ? null : [...style.backgroundColor],
    style.opacity ?? 1,
    style.fontSize ?? 0.1,
    assetSignature(style.asset),
    style.enabled ?? true,
  ];
}

/** Commit signature for one GUI root. Refs and callbacks are local-only and excluded. */
export function guiRootSignature(nodes: readonly GuiDescribedNode[]): string {
  return JSON.stringify(
    nodes.map((node) => [
      node.identity,
      node.parent ?? null,
      node.type,
      guiContentSignature(node.content),
      styleSignature(node.style),
      node.theme ?? null,
    ]),
  );
}

/** Transport signature for node content. Runtime-owned control values are
 * insertion-only and therefore excluded from ordinary rerender equality. */
export function guiContentSignature(content: GuiNodeContent): unknown {
  switch (content.kind) {
    case "checkbox":
      return [content.kind];
    case "slider":
      return [content.kind, content.min, content.max, content.step];
    case "textInput":
      return [content.kind, content.placeholder];
    default:
      return content;
  }
}

function equalTuples(
  a: readonly number[] | undefined,
  b: readonly number[] | undefined,
): boolean {
  if (a === undefined || b === undefined)
    return a === undefined && b === undefined;
  return (
    a.length === b.length &&
    a.every((value, index) => Object.is(value, b[index]))
  );
}

export function equalGuiContent(a: GuiNodeContent, b: GuiNodeContent): boolean {
  if (a.kind !== b.kind) return false;
  switch (a.kind) {
    case "container":
      return b.kind === "container" && a.containerKind === b.containerKind;
    case "text":
      return b.kind === "text" && a.text === b.text;
    case "drawing":
      return true;
    case "image":
      return b.kind === "image" && equalTuples(a.size, b.size);
    case "button":
      return b.kind === "button" && a.label === b.label;
    case "checkbox":
      return b.kind === "checkbox";
    case "slider":
      return (
        b.kind === "slider" &&
        Object.is(a.min, b.min) &&
        Object.is(a.max, b.max) &&
        Object.is(a.step, b.step)
      );
    case "textInput":
      return b.kind === "textInput" && a.placeholder === b.placeholder;
  }
}

export function equalGuiStyle(
  inputA: GuiNodeStyle | GuiDeclarationStyle,
  inputB: GuiNodeStyle | GuiDeclarationStyle,
): boolean {
  const a = normalizeGuiStyle(inputA);
  const b = normalizeGuiStyle(inputB);
  return (
    Object.is(a.width, b.width) &&
    Object.is(a.height, b.height) &&
    Object.is(a.minWidth, b.minWidth) &&
    Object.is(a.minHeight, b.minHeight) &&
    Object.is(a.maxWidth, b.maxWidth) &&
    Object.is(a.maxHeight, b.maxHeight) &&
    equalTuples(a.padding, b.padding) &&
    equalTuples(a.margin, b.margin) &&
    Object.is(a.flex, b.flex) &&
    Object.is(a.alignX, b.alignX) &&
    Object.is(a.alignY, b.alignY) &&
    equalTuples(a.color, b.color) &&
    equalTuples(a.backgroundColor, b.backgroundColor) &&
    Object.is(a.opacity, b.opacity) &&
    Object.is(a.fontSize, b.fontSize) &&
    equalGuiAsset(a.asset, b.asset) &&
    Object.is(a.enabled ?? true, b.enabled ?? true)
  );
}

/** Whether two asset lanes bind the same source. */
export function equalGuiAsset(
  a: GuiAssetSource | null | undefined,
  b: GuiAssetSource | null | undefined,
): boolean {
  if (a === undefined || b === undefined) return a === b;
  if (a === null || b === null) return a === b;
  return (
    a.kind === b.kind &&
    a.source === b.source &&
    (a.variant ?? 0) === (b.variant ?? 0)
  );
}

class ActionEvent implements GuiActionEvent {
  private stopped = false;

  constructor(
    readonly target: number,
    readonly path: readonly number[],
    readonly phase: "capture" | "bubble",
  ) {}

  stopPropagation(): void {
    this.stopped = true;
  }

  get propagationStopped(): boolean {
    return this.stopped;
  }
}

export interface GuiActionListeners {
  readonly capture: GuiActionListener | undefined;
  readonly bubble: GuiActionListener | undefined;
}

/** Logical ancestor path from the root to the target, inclusive. */
export function resolveGuiActionPath(
  parentOf: ReadonlyMap<number, number | undefined>,
  target: number,
): readonly number[] {
  const path: number[] = [target];
  let current = parentOf.get(target);
  while (current !== undefined) {
    path.unshift(current);
    current = parentOf.get(current);
  }
  return path;
}

/** Dispatch along the logical ancestor path: capture root-first, then bubble.
 *
 * JavaScript `stopPropagation` controls callbacks only. This helper never
 * touches transport; the runtime owns defaults and committed effects.
 */
export function dispatchGuiAction(
  path: readonly number[],
  listeners: (identity: number) => GuiActionListeners,
  target: number,
  onError?: (error: Error) => void,
): void {
  const invoke = (
    listener: GuiActionListener | undefined,
    event: ActionEvent,
  ) => {
    try {
      listener?.(event);
    } catch (error) {
      onError?.(error instanceof Error ? error : new Error(String(error)));
    }
  };
  const capture = new ActionEvent(target, path, "capture");
  for (const identity of path) {
    invoke(listeners(identity).capture, capture);
    if (capture.propagationStopped) return;
  }
  const bubble = new ActionEvent(target, path, "bubble");
  for (let index = path.length - 1; index >= 0; index -= 1) {
    invoke(listeners(path[index]!).bubble, bubble);
    if (bubble.propagationStopped) return;
  }
}
