/**
 * The workbench's CONTROLS tab: the projection's value and selection
 * controls in a frameless scrolling column, so each keeps its natural size.
 *
 * - BEAM, a knob with its paired numeric stepper, scales the beam's energy.
 * - LIGHT, a vertical slider with a scale, sets the studio key and fill.
 * - OFFSET, a bipolar slider with a scale round zero, moves the scope's
 *   traces up or down.
 * - SWEEP, a range slider, bounds the band that crosses the scope while SCAN
 *   runs, and RATE, a dropdown, sets the scan's speed.
 * - PRESET, a searchable dropdown, sets all of those at once, and CHANNELS,
 *   a multi-select, switches the beam, the dust and the studio lights.
 *
 * Every control is controlled by the tuning, so a preset moves them all.
 */
import type { ReactNode } from "react";
import { Children, Entity } from "@ipp/react";
import { Behavior, ScrollView, Skin } from "@ipp/react/gui";
import {
  Dropdown,
  Knob,
  LabelledSeparator,
  LabelledSlider,
  MultiSelect,
  NumericStepper,
  RangeSlider,
  Row as KitRow,
  SearchableDropdown,
  TextLine,
} from "@ipp/react/gui-kit";
import {
  BoxLayout,
  COLUMN,
  LEAF,
  ROW,
  TOKENS,
  WORKBENCH_WIDTH,
} from "./presentation.js";
import type { GuiSceneState } from "./scene.js";
import { FRAMELESS_SCROLL_THEME } from "./telemetry.js";
import {
  CHANNELS,
  PRESETS,
  SCAN_RATES,
  type Channel,
  type ScanRate,
} from "./tuning.js";

/** Symbolic IDs of the tab's controls. */
export const TUNING_CONTROLS = {
  scroll: "gui-tuning",
  beam: "gui-beam",
  beamInput: "gui-beam/input",
  light: "gui-light",
  offset: "gui-offset",
  sweep: "gui-sweep",
  rate: "gui-rate",
  preset: "gui-preset",
  channels: "gui-channels",
} as const;

/** LIGHT's rail length: the knob's housing and stepper beside it. */
const RAIL_LENGTH = 176;

/** The knob's column, wide enough for the stepper's error message. */
const KNOB_WIDTH = 104;

/** The scroll bar's column at the tab's right edge. */
const BAR_COLUMN = 3 * TOKENS.bar;

/** The column's width: the tab's content at the inset, less the bar column. */
const CONTENT_WIDTH =
  WORKBENCH_WIDTH - 2 * TOKENS.lineWidth - 2 * TOKENS.inset - BAR_COLUMN;

/** Captions of the selection rows, and the controls after them. */
const CAPTION_WIDTH = 80;
const SELECT_WIDTH = CONTENT_WIDTH - CAPTION_WIDTH;

export function TuningTab({ scene }: { readonly scene: GuiSceneState }) {
  const tuning = scene.tuning;
  const values = tuning.tuning;
  return (
    <Entity id={TUNING_CONTROLS.scroll}>
      {/* The scroll view fills the tab's content, its bar in the last column. */}
      <BoxLayout kind={LEAF} width={CONTENT_WIDTH + BAR_COLUMN} flex={1} />
      <Skin theme={FRAMELESS_SCROLL_THEME} />
      <Behavior semantic_label="CONTROLS" />
      <ScrollView />
      <Children>
        <Entity id="gui-tuning-body">
          <BoxLayout kind={COLUMN} padding={[0, BAR_COLUMN, TOKENS.inset, 0]} />
          <Children>
            <Entity id="gui-tuning-dials">
              <BoxLayout kind={ROW} />
              <Children>
                <Knob
                  id={TUNING_CONTROLS.beam}
                  label="BEAM"
                  min={0}
                  max={200}
                  step={5}
                  units="%"
                  value={values.beam}
                  onChange={tuning.setBeam}
                  layout={{ width: KNOB_WIDTH }}
                >
                  <NumericStepper
                    id={TUNING_CONTROLS.beamInput}
                    min={0}
                    max={200}
                    step={5}
                    units="%"
                    stepParts={false}
                    bounds={false}
                    value={values.beam}
                    onChange={tuning.setBeam}
                  />
                </Knob>
                <LabelledSlider
                  id={TUNING_CONTROLS.light}
                  label="LIGHT"
                  min={0}
                  max={100}
                  step={5}
                  units="%"
                  vertical
                  length={RAIL_LENGTH}
                  value={values.light}
                  onChange={tuning.setLight}
                  scale={{ count: 5 }}
                  layout={{ flex: 1 }}
                />
              </Children>
            </Entity>
            <LabelledSeparator
              id="gui-tuning-scope"
              label="SCOPE"
              layout={{
                margin_top: TOKENS.inset,
                margin_bottom: TOKENS.inset / 2,
              }}
            />
            <LabelledSlider
              id={TUNING_CONTROLS.offset}
              label="OFFSET"
              min={-50}
              max={50}
              step={5}
              units="%"
              origin={0}
              value={values.offset}
              onChange={tuning.setOffset}
              scale={{ count: 5, origin: 0 }}
            />
            <RangeSlider
              id={TUNING_CONTROLS.sweep}
              label="SWEEP"
              min={0}
              max={100}
              step={5}
              units="%"
              value={values.sweep}
              onChange={(range) => tuning.setSweep([range[0], range[1]])}
            />
            <Caption id="gui-rate-row" text="RATE">
              <Dropdown
                id={TUNING_CONTROLS.rate}
                label="RATE"
                options={SCAN_RATES.map(({ key, label }) => ({ key, label }))}
                value={values.rate}
                onChange={(key) => tuning.setRate(key as ScanRate)}
                layout={{ width: SELECT_WIDTH }}
              />
            </Caption>
            <LabelledSeparator
              id="gui-tuning-setup"
              label="SETUP"
              layout={{
                margin_top: TOKENS.inset,
                margin_bottom: TOKENS.inset / 2,
              }}
            />
            <Caption id="gui-preset-row" text="PRESET">
              <SearchableDropdown
                id={TUNING_CONTROLS.preset}
                label="PRESET"
                placeholder="CHOOSE"
                options={PRESETS.map(({ key, label }) => ({ key, label }))}
                {...(values.preset === undefined
                  ? {}
                  : { value: values.preset })}
                onChange={tuning.setPreset}
                layout={{ width: SELECT_WIDTH }}
              />
            </Caption>
            <Caption id="gui-channels-row" text="CHANNELS">
              <MultiSelect
                id={TUNING_CONTROLS.channels}
                label="CHANNELS"
                placeholder="NONE"
                options={CHANNELS.map(({ key, label }) => ({ key, label }))}
                value={values.channels}
                onChange={(keys) => tuning.setChannels(keys as Channel[])}
                layout={{ width: SELECT_WIDTH }}
              />
            </Caption>
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}

/** A selection row: its caption in the accent, then the control. */
function Caption({
  id,
  text,
  children,
}: {
  readonly id: string;
  readonly text: string;
  readonly children: ReactNode;
}) {
  return (
    <KitRow
      id={id}
      height={TOKENS.controlHeight}
      layout={{ margin_top: TOKENS.inset / 2 }}
    >
      <TextLine
        id={`${id}/caption`}
        text={text}
        tone="accent"
        size="small"
        layout={{ width: CAPTION_WIDTH }}
      />
      {children}
    </KitRow>
  );
}
