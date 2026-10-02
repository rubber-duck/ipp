/**
 * The 3D scene's nodes as the TELEMETRY panel's SCENE view lists them: the
 * gallery World's own entities, grouped as the projector, the stage and the
 * lights, beside the panel and its input shield. Selecting a node names it
 * in the readouts and brightens it in the scene where that is one material
 * or light: the projector's parts, the stage's baked surfaces and the
 * lights. The panel and the shield are named only.
 */
import type { TreeNode } from "@ipp/react/gui-kit";

/** Material Design glyphs of the GUI font for the tree's rows. */
const CUBE = "\u{f01a7}";
const LIGHTBULB = "\u{f0335}";
const LAYERS = "\u{f0328}";
const MONITOR = "\u{f0379}";
const SHIELD = "\u{f0498}";

export const SCENE_TREE: readonly TreeNode[] = [
  {
    key: "projector",
    label: "PROJECTOR",
    icon: CUBE,
    children: [
      { key: "core", label: "CORE" },
      { key: "lens", label: "LENS" },
      { key: "beam", label: "BEAM" },
      { key: "dust", label: "DUST" },
    ],
  },
  {
    key: "stage",
    label: "STAGE",
    icon: LAYERS,
    children: [
      { key: "base", label: "BASE" },
      { key: "floor", label: "FLOOR" },
    ],
  },
  {
    key: "lights",
    label: "LIGHTS",
    icon: LIGHTBULB,
    children: [
      { key: "glow", label: "PROJECTOR LIGHT" },
      { key: "key", label: "KEY LIGHT" },
      { key: "fill", label: "FILL LIGHT" },
    ],
  },
  { key: "panel", label: "PANEL", icon: MONITOR },
  { key: "shield", label: "SHIELD", icon: SHIELD },
];

/** Branches open at first show. */
export const SCENE_TREE_EXPANDED: readonly string[] = ["projector"];

/** A node's label by key, searching the whole tree. */
export function sceneNodeLabel(key: string | undefined): string | undefined {
  const visit = (nodes: readonly TreeNode[]): string | undefined => {
    for (const node of nodes) {
      if (node.key === key) return node.label;
      const found = node.children && visit(node.children);
      if (found) return found;
    }
    return undefined;
  };
  return key === undefined ? undefined : visit(SCENE_TREE);
}

/**
 * How much a node brightens while selected: its own part, or every part of
 * a selected branch.
 */
export function focusGain(focus: string | undefined, part: string): number {
  if (focus === undefined) return 1;
  const branch = SCENE_TREE.find((node) => node.key === focus);
  const selected =
    focus === part ||
    (branch?.children?.some((child) => child.key === part) ?? false);
  return selected ? 1.8 : 1;
}
