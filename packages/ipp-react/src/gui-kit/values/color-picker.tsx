/**
 * A colour picker: the runtime's colour control, its saturation-value field,
 * hue rail, optional alpha rail and swatch, with the caption and the rails'
 * labels above it, the numeric R, G and B channels, 0 to 255, and A, in
 * percent, beside it, a row of preset swatches, a hex entry with the colour
 * space it states, and an error line.
 *
 * The one colour is the control's HSVA (`color-value.ts`), which the runtime
 * holds and changes as the field and rails are dragged or stepped; every other
 * presentation is derived from it and declared again when it changes. An
 * entry converts to HSVA and sets the control with its value action, and the
 * picker reports the committed colour through `onChange`, as it reports a
 * drag: a hex on Enter, or on blur when typed since the last entry or
 * colour; a channel when its field commits a number, on Enter, blur or a
 * step; a preset when pressed. A field's text or number counts as an entry
 * only while that field holds focus, so its own updates from a drag, which
 * arrive later, never write the colour back over the drag.
 *
 * Text that is no hex, or a channel entry that is no number, leaves the
 * colour unchanged and shows an error alert in the line the picker reserves
 * at its bottom, so nothing moves; it clears on the next committed colour or
 * valid entry, or when the channel field it came from discards its edit, as
 * on Escape, or loses focus, which ends the edit. A hex that does not parse
 * stays in its field until corrected or replaced by the next colour; the hex
 * field is a plain text input, whose Escape blurs it and so validates it
 * again.
 *
 * The picker is a fixed-size column: the caption row, the control and the
 * channels, the presets, the hex row and the error line.
 */
import { useRef, useState, type ReactNode } from "react";
import { Children, Entity } from "../../components.js";
import type {
  GuiFocusChangeEvent,
  GuiHsva,
  GuiScalarEvent,
} from "../../gui/callbacks.js";
import { Style, Behavior, Font, Group, Layout } from "../../gui/components.js";
import type { GuiControlHandle, GuiControlRef } from "../../gui/control-ref.js";
import { Button, Color, TextInput } from "../../gui/controls.js";
import { Skin } from "../../gui/theme.js";
import {
  formatHex,
  hsvaToRgba,
  linearColor,
  parseHex,
  rgbaToHsva,
  useColorValue,
  type ColorRgba,
} from "./color-value.js";
import { InlineAlert } from "../feedback/inline-alert.js";
import { useGuiKit, type GuiKitScope, type GuiKitTone } from "../kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  LAYOUT_ROW,
  LAYOUT_STACK,
  Row,
  type GuiKitLayout,
} from "../layout.js";
import { useOwnRow } from "../row-motion.js";
import { TextLine, lineWidth } from "../text.js";

/** One explicit choice of colour. */
export interface ColorPreset {
  readonly value: GuiHsva;
  /** Its name for assistive reading; its hex by default. */
  readonly label?: string;
}

export interface ColorPickerProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the picker's root entity; its parts extend it. */
  readonly id: string;
  /** The caption naming the colour, such as COLOR, in the accent. */
  readonly label?: string;
  /** The colour, controlled; pair it with `onChange`. */
  readonly value?: GuiHsva;
  /** The initial colour while `value` is omitted; opaque white by default. */
  readonly defaultValue?: GuiHsva;
  /** The runtime committed a new colour: a drag, a key or an entry. */
  readonly onChange?: (value: GuiHsva) => void;
  /** Show the alpha rail and the A field; true by default. */
  readonly alpha?: boolean;
  /** Colours the picker offers as swatches. */
  readonly presets?: readonly ColorPreset[];
  /** Keep the colour and look but refuse input; every text turns neutral. */
  readonly disabled?: boolean;
  readonly layout?: GuiKitLayout;
  /** The colour control's handle. */
  readonly ref?: GuiControlRef;
}

/** The runtime's control arrangement, in ems of its font. */
const CONTROL = {
  inset: 0.5,
  gap: 1,
  rail: 1.5,
  field: 9,
  swatch: 1.5,
} as const;

/** Width of the channels column, at the tokens' em. */
const CHANNELS = 112;

const INVALID_HEX = "Use #RRGGBB or #RRGGBBAA";
const NOT_A_NUMBER = "Not a number";

const WHITE: GuiHsva = { hue: 0, saturation: 0, value: 1, alpha: 1 };

type Channel = "red" | "green" | "blue" | "alpha";

const CHANNEL_ROWS: readonly {
  readonly channel: Channel;
  readonly letter: string;
  readonly max: number;
  readonly units?: string;
}[] = [
  { channel: "red", letter: "R", max: 255 },
  { channel: "green", letter: "G", max: 255 },
  { channel: "blue", letter: "B", max: 255 },
  { channel: "alpha", letter: "A", max: 100, units: "%" },
];

/** The picker's geometry in the World's units. */
function geometry(kit: GuiKitScope, alpha: boolean) {
  const t = kit.tokens;
  const em = kit.fontSize;
  const side = 2 * CONTROL.inset + CONTROL.field;
  const rails = alpha ? 2 : 1;
  const control = [
    (side + rails * (CONTROL.gap + CONTROL.rail)) * em,
    (side + CONTROL.gap + CONTROL.swatch) * em,
  ] as const;
  // A rail's centre across the control: past the field and the gaps before it.
  const rail = (index: number) =>
    (CONTROL.inset +
      CONTROL.field +
      (index + 1) * CONTROL.gap +
      index * CONTROL.rail +
      CONTROL.rail / 2) *
    em;
  const gap = kit.unit(t.inset / 2);
  return {
    control,
    rail,
    gap,
    width: control[0] + kit.unit(t.inset) + kit.unit(CHANNELS),
    row: kit.unit(t.denseRow),
    field: kit.unit(t.controlHeight),
    swatch: kit.unit(t.smallHeight),
    labelInset: CONTROL.inset * em,
  };
}

export function ColorPicker({
  id,
  layer = 0,
  label,
  value,
  defaultValue = WHITE,
  onChange,
  alpha = true,
  presets,
  disabled = false,
  layout,
  ref,
}: ColorPickerProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const g = geometry(kit, alpha);
  const color = useColorValue(value, defaultValue, onChange, ref);
  const current = color.current;
  const rgba = hsvaToRgba(current);
  const tone = (role: GuiKitTone): GuiKitTone => (disabled ? "neutral" : role);

  const [error, setError] = useState<
    { readonly text: string; readonly source: string } | undefined
  >(undefined);
  // The tick of the last committed colour or valid entry: an error reported
  // from that frame or earlier was superseded by it.
  const settled = useRef(-1n);
  const settle = (tick: bigint, source?: string) => {
    if (tick > settled.current) settled.current = tick;
    setError((shown) =>
      source === undefined || shown?.source === source ? undefined : shown,
    );
  };
  const fail = (tick: bigint, source: string, text: string) => {
    if (tick > settled.current) setError({ text, source });
  };

  // Each entry field's focus from and until a tick: its numbers and text
  // count as entries only from inside it, so the field's own updates as the
  // colour is dragged, which arrive later, are never taken for entries.
  const focus = useRef(
    new Map<Channel | "hex", { from: bigint; until?: bigint }>(),
  );
  const focusChange = (field: Channel | "hex", event: GuiFocusChangeEvent) => {
    if (event.focused) focus.current.set(field, { from: event.tick });
    else {
      const held = focus.current.get(field);
      if (held) focus.current.set(field, { ...held, until: event.tick });
      if (field !== "hex") settle(event.tick, field);
    }
  };
  const entered = (field: Channel | "hex", tick: bigint) => {
    const held = focus.current.get(field);
    return (
      held !== undefined &&
      held.from <= tick &&
      (held.until === undefined || tick <= held.until)
    );
  };
  const channelCommit = (channel: Channel, event: GuiScalarEvent) => {
    if (!entered(channel, event.tick)) return;
    settle(event.tick);
    if (channel === "alpha") {
      color.enter({ ...current, alpha: event.value / 100 });
      return;
    }
    const next: ColorRgba = { ...rgba, [channel]: Math.round(event.value) };
    color.enter(rgbaToHsva(next, current));
  };

  // The hex the picker declares, and text typed into the hex field since it
  // last applied an entry or the colour last changed, which blur applies.
  const hex = formatHex(current, alpha);
  const declaredHex = useRef(hex);
  declaredHex.current = hex;
  const typedHex = useRef<string | undefined>(undefined);
  const hexHandle = useRef<GuiControlHandle | null>(null);
  const hexEntry = (text: string, tick: bigint) => {
    const parsed = parseHex(text, current);
    if (!parsed) {
      typedHex.current = text;
      fail(tick, "hex", INVALID_HEX);
      return;
    }
    typedHex.current = undefined;
    settle(tick);
    color.enter(parsed);
    const shown = formatHex(parsed, alpha);
    if (shown !== text)
      void hexHandle.current
        ?.compareAndSet("text", text, shown)
        .catch(() => false);
  };

  const presetsRow = presets !== undefined && presets.length > 0;
  const height =
    g.row +
    g.control[1] +
    (presetsRow ? g.gap + g.swatch : 0) +
    2 * (g.gap + g.field);
  const small = kit.typeSize("small");

  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        width={g.width}
        height={height}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        <Entity id={`${id}/caption`}>
          <Layout kind={LAYOUT_STACK} height={g.row} />
          <Children>
            {label !== undefined && (
              <Placed id={`${id}/label`} x={g.labelInset} height={g.row}>
                <TextLine
                  id={`${id}/label/text`}
                  text={label}
                  tone={tone("accent")}
                />
              </Placed>
            )}
            {(alpha ? ["Hue", "Opacity"] : ["Hue"]).map((name, index) => {
              const width = lineWidth(name, small);
              return (
                <Placed
                  key={name}
                  id={`${id}/${name.toLowerCase()}`}
                  x={g.rail(index) - width / 2}
                  height={g.row}
                >
                  <TextLine
                    id={`${id}/${name.toLowerCase()}/text`}
                    text={name}
                    tone="neutral"
                    size="small"
                  />
                </Placed>
              );
            })}
          </Children>
        </Entity>
        <Entity id={`${id}/main`}>
          <Layout kind={LAYOUT_ROW} height={g.control[1]} />
          <Children>
            <Entity id={`${id}/control`}>
              <Layout
                kind={LAYOUT_LEAF}
                width={g.control[0]}
                height={g.control[1]}
              />
              <Behavior
                enabled={!disabled}
                {...(label === undefined ? {} : { semantic_label: label })}
              />
              <Color
                {...color.control}
                alpha_rail={alpha}
                onColorCommit={(event) => {
                  color.control.onColorCommit(event);
                  // The next colour replaces any typed hex.
                  typedHex.current = undefined;
                  settle(event.tick);
                }}
              />
            </Entity>
            <Entity id={`${id}/channels`}>
              <Layout
                kind={LAYOUT_COLUMN}
                width={kit.unit(CHANNELS)}
                margin_left={kit.unit(t.inset)}
              />
              <Children>
                {CHANNEL_ROWS.filter(
                  ({ channel }) => alpha || channel !== "alpha",
                ).map(({ channel, letter, max, units }, index) => (
                  <ChannelField
                    key={channel}
                    id={`${id}/${channel}`}
                    letter={letter}
                    max={max}
                    units={units}
                    number={
                      channel === "alpha"
                        ? Math.round(rgba.alpha * 100)
                        : rgba[channel]
                    }
                    disabled={disabled}
                    first={index === 0}
                    onCommit={(event) => channelCommit(channel, event)}
                    onReject={(event) =>
                      fail(event.tick, channel, `${letter}: ${NOT_A_NUMBER}`)
                    }
                    onDiscard={(event) => settle(event.tick, channel)}
                    onFocusChange={(event) => focusChange(channel, event)}
                  />
                ))}
              </Children>
            </Entity>
          </Children>
        </Entity>
        {presetsRow && (
          <Entity id={`${id}/presets`}>
            <Layout
              kind={LAYOUT_ROW}
              height={g.swatch}
              margin_top={g.gap}
              padding_left={g.labelInset}
            />
            <Group axis={0} selection={0} />
            <Children>
              <Row
                id={`${id}/presets/caption`}
                height={t.smallHeight}
                layout={{
                  width: lineWidth("Presets", kit.typeSize("body")),
                }}
              >
                <TextLine
                  id={`${id}/presets/caption/text`}
                  text="Presets"
                  tone={tone("text")}
                />
              </Row>
              {presets.map((preset, index) => (
                <Swatch
                  key={index}
                  id={`${id}/preset/${index}`}
                  preset={preset}
                  disabled={disabled}
                  onPress={() => color.enter(preset.value)}
                />
              ))}
            </Children>
          </Entity>
        )}
        <Row
          id={`${id}/hex`}
          height={t.controlHeight}
          layout={{ margin_top: g.gap }}
        >
          <TextLine
            id={`${id}/hex/label`}
            text="Hex"
            tone={tone("text")}
            layout={{ margin_left: g.labelInset }}
          />
          <Entity id={`${id}/hex/field`}>
            <Layout
              kind={LAYOUT_LEAF}
              height={g.field}
              flex={1}
              margin_left={g.gap}
            />
            <Behavior enabled={!disabled} semantic_label="Hex" />
            <TextInput
              text={hex}
              ref={(handle) => {
                hexHandle.current = handle;
              }}
              onTextCommit={(event) => {
                if (
                  entered("hex", event.tick) &&
                  event.value !== declaredHex.current
                )
                  typedHex.current = event.value;
              }}
              onSubmit={(event) => hexEntry(event.value, event.tick)}
              onFocusChange={(event) => {
                focusChange("hex", event);
                const typed = typedHex.current;
                if (!event.focused && typed !== undefined)
                  hexEntry(typed, event.tick);
              }}
            />
          </Entity>
          <TextLine
            id={`${id}/hex/space`}
            text="sRGB"
            tone={tone("text")}
            layout={{
              width: lineWidth("sRGB", kit.typeSize("body")),
              margin_left: g.gap,
            }}
          />
        </Row>
        {error !== undefined && (
          <InlineAlert
            id={`${id}/error`}
            severity="error"
            text={error.text}
            layout={{ margin_top: g.gap }}
          />
        )}
      </Children>
    </Entity>
  );
}

/** A box of a stack at `x` across, `height` high, centring its content. */
function Placed({
  id,
  x,
  height,
  children,
}: {
  readonly id: string;
  readonly x: number;
  readonly height: number;
  readonly children: ReactNode;
}) {
  return (
    <Row id={id} layout={{ height, margin_left: x, align_x: -1 }}>
      {children}
    </Row>
  );
}

/**
 * One channel: its letter, the runtime's numeric input without step parts,
 * the number derived from the colour and declared again when it changes, and
 * the unit beside it, or the unit's room.
 */
function ChannelField({
  id,
  letter,
  max,
  units,
  number,
  disabled,
  first,
  onCommit,
  onReject,
  onDiscard,
  onFocusChange,
}: {
  readonly id: string;
  readonly letter: string;
  readonly max: number;
  readonly units: string | undefined;
  readonly number: number;
  readonly disabled: boolean;
  readonly first: boolean;
  readonly onCommit: (event: GuiScalarEvent) => void;
  readonly onReject: (event: { readonly tick: bigint }) => void;
  readonly onDiscard: (event: { readonly tick: bigint }) => void;
  readonly onFocusChange: (event: GuiFocusChangeEvent) => void;
}) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const body = kit.typeSize("body");
  const gap = kit.unit(t.inset / 2);
  const tone: GuiKitTone = disabled ? "neutral" : "text";
  return (
    <Row
      id={id}
      height={t.controlHeight}
      {...(first ? {} : { layout: { margin_top: gap } })}
    >
      <TextLine
        id={`${id}/letter`}
        text={letter}
        tone={tone}
        layout={{ width: lineWidth(letter, body) }}
      />
      <Entity id={`${id}/field`}>
        <Layout
          kind={LAYOUT_LEAF}
          height={kit.unit(t.controlHeight)}
          flex={1}
          margin_left={gap}
        />
        <Behavior enabled={!disabled} semantic_label={letter} />
        <TextInput
          numeric
          value={number}
          min={0}
          max={max}
          step={1}
          precision={0}
          step_parts={false}
          onScalarCommit={onCommit}
          onReject={onReject}
          onDiscard={onDiscard}
          onFocusChange={onFocusChange}
        />
      </Entity>
      <TextLine
        id={`${id}/units`}
        text={units ?? ""}
        tone={tone}
        layout={{ width: lineWidth("%", body), margin_left: gap }}
      />
    </Row>
  );
}

/**
 * A preset: a Button the small height square painted in its colour over the
 * secondary look's states, so hover and focus light its edge.
 */
function Swatch({
  id,
  preset,
  disabled,
  onPress,
}: {
  readonly id: string;
  readonly preset: ColorPreset;
  readonly disabled: boolean;
  readonly onPress: () => void;
}) {
  const kit = useGuiKit();
  const side = kit.unit(kit.tokens.smallHeight);
  const parts = useOwnRow({ color: linearColor(preset.value) });
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_LEAF}
        width={side}
        height={side}
        margin_left={kit.unit(kit.tokens.inset / 2)}
        align_y={0}
      />
      <Skin theme={kit.theme("secondary")} parts={parts} />
      <Behavior
        enabled={!disabled}
        semantic_label={preset.label ?? formatHex(preset.value)}
      />
      <Button label="" onPress={onPress} />
    </Entity>
  );
}
