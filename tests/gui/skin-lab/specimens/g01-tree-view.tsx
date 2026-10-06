/**
 * Sheet g, the tree view, drawn with the GUI kit's `TreeView`: the sheet's
 * scene with Camera collapsed and Cube selected, its icons from the shared
 * font's Material outline set; beside it the same tree with keyboard focus
 * moved from the selected Cube to Sphere, so selection and focus show apart
 * as the sheet's separate rows do; below, a hovered row.
 *
 * The kit's rows are the language's row height, taller against their text
 * than the sheet draws them; it draws no hierarchy lines. Drawn at the
 * context sheet's scale through a nested `GuiKit`.
 */
import { GuiKit, TreeView, type TreeNode } from "@ipp/react/gui-kit";
import {
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  around,
  placed,
} from "../kit.js";
import { SHEET_G_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import { INSET, ROW, TEXT_BODY } from "../themes/geometry.js";

/** The g01 crop is 2x of sheet g (14, 106) to (768, 500); a tree added below. */
const EXTENT = [754, 940] as const;

const u = (value: number) => value * K;

/** Material outline glyphs of the shared font. */
const GLYPH = {
  folder: "\u{f0256}",
  camera: "\u{f0bdc}",
  light: "\u{f0336}",
  cube: "\u{f01a7}",
  sphere: "\u{f1954}",
} as const;

const NODES: readonly TreeNode[] = [
  {
    key: "scene",
    label: "Scene",
    icon: GLYPH.folder,
    children: [
      {
        key: "camera",
        label: "Camera",
        icon: GLYPH.camera,
        children: [{ key: "lens", label: "Lens" }],
      },
      {
        key: "lighting",
        label: "Lighting",
        icon: GLYPH.light,
        children: [
          { key: "key", label: "Key", icon: GLYPH.light },
          { key: "fill", label: "Fill", icon: GLYPH.light },
        ],
      },
      {
        key: "geometry",
        label: "Geometry",
        icon: GLYPH.cube,
        children: [
          { key: "cube", label: "Cube", icon: GLYPH.cube },
          { key: "sphere", label: "Sphere", icon: GLYPH.sphere },
        ],
      },
    ],
  },
];
const EXPANDED = ["scene", "lighting", "geometry"];
const ROWS = 8;

/** Each tree's top-left and width, in tree and so Tab order. */
const TREES = {
  tree: { at: [27.5, 105], width: 336 },
  focus: { at: [406, 105], width: 311 },
  hover: { at: [27.5, 530], width: 336 },
} as const satisfies Record<string, { at: Point; width: number }>;
type Tree = keyof typeof TREES;

const HEIGHT = u(ROWS * ROW + INSET);

const rect = (tree: Tree): Rect => [
  ...TREES[tree].at,
  TREES[tree].width,
  HEIGHT,
];

/** The centre of visible row `index` of `tree`. */
const row = (tree: Tree, index: number): Point => [
  TREES[tree].at[0] + TREES[tree].width / 2,
  TREES[tree].at[1] + u(INSET / 2 + (index + 0.5) * ROW),
];

const CAPTIONS: Readonly<Partial<Record<Tree, string>>> = {
  focus: "Selected is not focused",
  hover: "Hover",
};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "g01-tree-view.png", origin: [0, 0] },
  states: [
    { name: "tree", cell: around(rect("tree"), 12) },
    {
      // Tab enters each tree at its selected Cube; Down moves focus to
      // Sphere and selects nothing.
      name: "focus",
      cell: around(rect("focus"), 12),
      pin: [
        { kind: "key", key: "tab" },
        { kind: "key", key: "tab" },
        { kind: "key", key: "down" },
      ],
    },
    {
      name: "hover",
      cell: around(rect("hover"), 12),
      pin: [{ kind: "hover", at: row("hover", 2) }],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={u(TEXT_BODY)}>
        {(Object.keys(TREES) as Tree[]).map((tree) => (
          <TreeView
            key={tree}
            id={`tree-${tree}`}
            nodes={NODES}
            defaultExpanded={EXPANDED}
            defaultValue="cube"
            layout={{
              ...placed(TREES[tree].at, TREES[tree].width),
              height: HEIGHT,
            }}
          />
        ))}
      </GuiKit>
      {(Object.keys(CAPTIONS) as Tree[]).map((tree) => (
        <Label
          key={tree}
          id={`caption-${tree}`}
          at={[TREES[tree].at[0], TREES[tree].at[1] + HEIGHT + 8]}
          text={CAPTIONS[tree]!}
          font={lab.font}
          size={u(12.5)}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
