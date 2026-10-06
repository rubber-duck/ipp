/**
 * Sheet g, the popover, drawn with the GUI kit's `Popover` and opened by a
 * real click on its trigger: the sheet's Options popover with a caption, a
 * text field, a checked checkbox and Apply and Cancel as the application's
 * content. Opening moves focus into the text field. On the right the same
 * popover after Tab twice, with keyboard focus on Apply's own edge. Each
 * state opens its own popover, which closes when the state's input is
 * released.
 *
 * The kit's popover has no attachment pointer, sits a quarter inset below
 * its trigger and is drawn at the language's sizes, so it is taller than the
 * sheet's. Drawn at the context sheet's scale through a nested `GuiKit`.
 */
import { Children, Entity, type AssetReference } from "@ipp/react";
import { Button, Checkbox, Font, Layout, TextInput } from "@ipp/react/gui";
import { GuiKit, Popover, Row, TextLine } from "@ipp/react/gui-kit";
import {
  FONT_ADVANCE,
  Fill,
  Label,
  SHEET_CAPTION,
  SHEET_PAGE,
} from "../kit.js";
import { SHEET_G_SCALE as K } from "../scale.js";
import {
  defineSpecimen,
  type PinStep,
  type Point,
  type Rect,
} from "../specimen.js";
import {
  CONTROL_HEIGHT,
  DENSE_ROW,
  INSET,
  LINE,
  SMALL_HEIGHT,
  TEXT_BODY,
  TEXT_SMALL,
} from "../themes/geometry.js";

/** The g03 crop is 2x of sheet g; the canvas starts 160 units into it. */
const EXTENT = [1000, 438] as const;

const u = (value: number) => value * K;

/** The popovers' centres under the sheet's triggers. */
const CENTRES = { open: 161.75, keyboard: 518 } as const;
type Name = keyof typeof CENTRES;

/** The trigger: the label and caret in small text, centred at the sheet's height. */
const TRIGGER = [
  u(9 * FONT_ADVANCE * TEXT_SMALL + 2 * INSET),
  u(SMALL_HEIGHT),
] as const;
const TRIGGER_TOP = 30 - TRIGGER[1] / 2;
const trigger = (name: Name): Rect => [
  CENTRES[name] - TRIGGER[0] / 2,
  TRIGGER_TOP,
  ...TRIGGER,
];

/** The popover's width and height: header, division and content. */
const WIDTH = u(240 + 2 * LINE);
/** A field across the content, which a leaf does not fill by itself. */
const FIELD = u(240 - 2 * INSET);
const CONTENT =
  2 * INSET +
  DENSE_ROW +
  CONTROL_HEIGHT +
  INSET / 2 +
  CONTROL_HEIGHT +
  INSET +
  CONTROL_HEIGHT;
const HEIGHT = u(CONTROL_HEIGHT + LINE + CONTENT + 2 * LINE);

/** A state's cell: the trigger and the popover below it, with their glow. */
const cell = (name: Name): Rect => [
  CENTRES[name] - WIDTH / 2 - 16,
  0,
  WIDTH + 32,
  TRIGGER_TOP + TRIGGER[1] + u(INSET / 4) + HEIGHT + 16,
];

const centre = ([x, y, width, height]: Rect): Point => [
  x + width / 2,
  y + height / 2,
];

/** Click the trigger and let the popover open. */
const open = (name: Name): PinStep[] => [
  { kind: "click", at: centre(trigger(name)) },
  { kind: "wait", seconds: 0.25 },
];

/** The application's content: a draft label, a setting and its buttons. */
function Content({
  name,
  font,
}: {
  readonly name: Name;
  readonly font: AssetReference;
}) {
  // Apply and Cancel in the default button look, as the sheet draws them.
  const button = (id: string, label: string, margin: number) => (
    <Entity id={`${name}-${id}`}>
      <Layout
        kind={0}
        flex={1}
        height={u(CONTROL_HEIGHT)}
        margin_left={margin}
      />
      <Button label={label} />
    </Entity>
  );
  return (
    <>
      <Row id={`${name}-caption`} height={DENSE_ROW}>
        <TextLine id={`${name}-caption/text`} text="Label" />
      </Row>
      <Entity id={`${name}-field`}>
        <Layout kind={0} width={FIELD} height={u(CONTROL_HEIGHT)} />
        <TextInput text="NIGHT-07" />
      </Entity>
      <Row
        id={`${name}-scan`}
        height={CONTROL_HEIGHT}
        layout={{ margin_top: u(INSET / 2) }}
      >
        <Entity id={`${name}-scan/box`}>
          <Layout
            kind={0}
            width={u(SMALL_HEIGHT)}
            height={u(SMALL_HEIGHT)}
            align_y={0}
          />
          <Font source={font} font_size={u(TEXT_BODY)} />
          <Checkbox label="" checked />
        </Entity>
        <TextLine
          id={`${name}-scan/label`}
          text="Scan enabled"
          layout={{ margin_left: u(INSET / 2) }}
        />
      </Row>
      <Entity id={`${name}-buttons`}>
        <Layout kind={1} height={u(CONTROL_HEIGHT)} margin_top={u(INSET)} />
        <Children>
          {button("apply", "Apply", 0)}
          {button("cancel", "Cancel", u(INSET))}
        </Children>
      </Entity>
    </>
  );
}

const CAPTIONS: Readonly<Record<Name, readonly [string, string]>> = {
  open: ["Open", "Interactive controls / explicit Apply"],
  keyboard: ["Keyboard focus", "Single-border focus / Escape closes"],
};

export default defineSpecimen({
  extent: EXTENT,
  reference: { image: "g03-popover.png", origin: [320, 0] },
  states: [
    { name: "open", cell: cell("open"), pin: open("open") },
    {
      name: "keyboard",
      cell: cell("keyboard"),
      pin: [
        ...open("keyboard"),
        { kind: "key", key: "tab" },
        { kind: "key", key: "tab" },
      ],
    },
  ],
  render: (lab) => (
    <>
      <Fill id="page" rect={[0, 0, ...EXTENT]} color={SHEET_PAGE} />
      {(Object.keys(CENTRES) as Name[]).flatMap((name) =>
        CAPTIONS[name].map((text, index) => (
          <Label
            key={`${name}-${index}`}
            id={`caption-${name}-${index}`}
            at={[CENTRES[name] - 200, 368 + index * 30]}
            width={400}
            centred
            text={text}
            font={lab.font}
            size={u(12.5)}
            color={SHEET_CAPTION}
          />
        )),
      )}
      <GuiKit fontSize={u(TEXT_BODY)}>
        {(Object.keys(CENTRES) as Name[]).map((name) => (
          <Popover
            key={name}
            id={`popover-${name}`}
            label="Options"
            title="Options"
            layout={{
              margin_left: trigger(name)[0],
              margin_top: TRIGGER_TOP,
              align_x: -1,
              align_y: -1,
            }}
          >
            <Content name={name} font={lab.font} />
          </Popover>
        ))}
      </GuiKit>
    </>
  ),
});
