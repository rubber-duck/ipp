/**
 * Sheet d, the segmented control, drawn with the GUI kit's
 * `SegmentedControl`: the sheet's World and Local at rest and with keyboard
 * focus on the selected World; then, below the crop, states the sheet omits:
 * a hovered segment, Right moving focus and selection together, a pressed
 * segment, and three segments with a middle one selected beside a disabled
 * one, which shows the separators and the cut only at the ends.
 *
 * The kit's control is the control height, taller than the sheet draws it.
 * Drawn at the value sheet's scale through a nested `GuiKit`.
 */
import { GuiKit, SegmentedControl } from "@ipp/react/gui-kit";
import {
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
  around,
  placed,
} from "../kit.js";
import { SHEET_D_SCALE as K } from "../scale.js";
import { defineSpecimen, type Point, type Rect } from "../specimen.js";
import { CONTROL_HEIGHT, TEXT_BODY } from "../themes/geometry.js";

/** The d04 crop is 2x of sheet d (1022, 284) to (1522, 490); rows added below. */
const EXTENT = [500, 640] as const;

const u = (value: number) => value * K;
const HEIGHT = u(CONTROL_HEIGHT);

const TWO = [
  { value: "world", label: "World" },
  { value: "local", label: "Local" },
] as const;
const THREE = [...TWO, { value: "view", label: "View", disabled: true }];

/** Each control's top-left, centred on the sheet's rows where it has them. */
const CONTROLS = {
  idle: [112, 41 - HEIGHT / 2],
  focus: [112, 133 - HEIGHT / 2],
  hover: [112, 240],
  arrow: [112, 330],
  pressed: [112, 420],
  three: [112, 510],
} as const satisfies Record<string, Point>;
type Control = keyof typeof CONTROLS;
const WIDTH = 310;

const rect = (control: Control): Rect => [...CONTROLS[control], WIDTH, HEIGHT];

/** The centre of segment `index` of `count` in `control`. */
const segment = (control: Control, index: number, count = 2): Point => [
  CONTROLS[control][0] + (WIDTH / count) * (index + 0.5),
  CONTROLS[control][1] + HEIGHT / 2,
];

const CAPTIONS: Readonly<Partial<Record<Control, string>>> = {
  hover: "Hover",
  arrow: "Right moves and selects",
  pressed: "Pressed",
  three: "Middle selected, last disabled",
};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "d04-segmented-control.png", origin: [0, 0] },
  states: [
    { name: "idle", cell: around(rect("idle"), 12) },
    {
      // A press focuses World without the ring; Home lights it as the
      // keyboard target, and it keeps its fill.
      name: "focus",
      cell: around(rect("focus"), 12),
      pin: [
        { kind: "click", at: segment("focus", 0) },
        { kind: "key", key: "home" },
      ],
    },
    {
      name: "hover",
      cell: around(rect("hover"), 12),
      pin: [{ kind: "hover", at: segment("hover", 1) }],
    },
    {
      // The press on World selects it again, so the pin repeats.
      name: "arrow",
      cell: around(rect("arrow"), 12),
      pin: [
        { kind: "click", at: segment("arrow", 0) },
        { kind: "key", key: "right" },
      ],
    },
    {
      name: "pressed",
      cell: around(rect("pressed"), 12),
      pin: [{ kind: "press", at: segment("pressed", 1) }],
    },
    { name: "three", cell: around(rect("three"), 12) },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      <GuiKit fontSize={u(TEXT_BODY)}>
        {(Object.keys(CONTROLS) as Control[]).map((control) => (
          <SegmentedControl
            key={control}
            id={`segmented-${control}`}
            options={control === "three" ? THREE : TWO}
            defaultValue={control === "three" ? "local" : "world"}
            layout={placed(CONTROLS[control], WIDTH)}
          />
        ))}
      </GuiKit>
      {(Object.keys(CAPTIONS) as Control[]).map((control) => (
        <Label
          key={control}
          id={`caption-${control}`}
          at={[CONTROLS[control][0], CONTROLS[control][1] + HEIGHT + 8]}
          text={CAPTIONS[control]!}
          font={lab.font}
          size={u(12.5)}
          color={SHEET_CAPTION}
        />
      ))}
    </>
  ),
});
