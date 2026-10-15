import { Children, Entity } from "@ipp/react";
import { Box, Layout, Style } from "@ipp/react/gui";
import {
  LabelledSlider,
  RangeSlider,
  Knob,
  NumericStepper,
  ColorPicker,
  ProgressBar,
  Spinner,
  CircularProgress,
  StatusBadge,
  InlineAlert,
  EmptyState,
} from "@ipp/react/gui-kit";
import { GlassGuiKit } from "@ipp/react/gui-glass";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import { resolveGlassPalette } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import type {
  GlassPaletteInput,
  GlassLight,
} from "../../../../packages/ipp-react/src/gui-glass/types.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";

export const VALUES_EXTENT = [740, 660] as const;

/** Ordinary composites: shared appearance changes no value/input behavior. */
export function valuesSpecimen(
  contract: GlassKitContract & ThemeContract,
  palette: Partial<GlassPaletteInput> = {},
  light: Partial<GlassLight> = {},
  disabled = false,
) {
  const background = resolveGlassPalette(palette).background;
  return defineSpecimen({
    extent: VALUES_EXTENT,
    states: [
      { name: "values-feedback", cell: [0, 0, ...VALUES_EXTENT] },
      {
        name: "slider-hover",
        cell: [0, 0, ...VALUES_EXTENT],
        pin: [{ kind: "hover", at: [341, 40] }],
      },
      {
        name: "slider-focus",
        cell: [0, 0, ...VALUES_EXTENT],
        pin: [
          {
            kind: "action",
            control: "labelled",
            action: { kind: "focus", part: 0 },
          },
        ],
      },
      {
        name: "dial-focus",
        cell: [0, 0, ...VALUES_EXTENT],
        pin: [
          {
            kind: "action",
            control: "dial",
            action: { kind: "focus", part: 0 },
          },
        ],
      },
      {
        name: "numeric-focus",
        cell: [0, 0, ...VALUES_EXTENT],
        pin: [
          {
            kind: "action",
            control: "number",
            action: { kind: "focus", part: 0 },
          },
        ],
      },
    ],
    render: (lab) => (
      <GlassGuiKit
        contract={contract}
        font={lab.font}
        fontSize={12}
        reducedMotion
        palette={palette}
        light={light}
      >
        <Entity id="values-page">
          <Layout kind={0} width={740} height={660} align_x={-1} align_y={-1} />
          <Style
            red={background[0]}
            green={background[1]}
            blue={background[2]}
            alpha={1}
          />
          <Box width={740} height={660} />
        </Entity>
        <Entity id="values-content">
          <Layout kind={3} width={740} height={660} align_x={-1} align_y={-1} />
          <Children>
            <Entity id="values-controls">
              <Layout
                kind={2}
                width={330}
                height={640}
                align_x={-1}
                align_y={-1}
                margin_left={18}
                margin_top={14}
              />
              <Children>
                <LabelledSlider
                  disabled={disabled}
                  id="value-labelled"
                  label="GAIN"
                  min={0}
                  max={100}
                  step={1}
                  defaultValue={62}
                  units="%"
                  ref={lab.control("labelled")}
                  scale={{ count: 5 }}
                />
                <RangeSlider
                  disabled={disabled}
                  id="value-range"
                  label="DISTANCE"
                  min={0}
                  max={100}
                  step={1}
                  defaultValue={[20, 80]}
                  units="m"
                  ref={lab.control("range")}
                />
                <Knob
                  disabled={disabled}
                  id="value-dial"
                  label="BALANCE"
                  min={-100}
                  max={100}
                  step={1}
                  origin={0}
                  defaultValue={35}
                  size={88}
                  ref={lab.control("dial")}
                />
                <NumericStepper
                  disabled={disabled}
                  id="value-number"
                  label="EXPOSURE"
                  min={-4}
                  max={4}
                  step={0.25}
                  precision={2}
                  defaultValue={1.25}
                  units="EV"
                  ref={lab.control("number")}
                />
                <ColorPicker
                  disabled={disabled}
                  id="value-color"
                  label="COLOR"
                  defaultValue={{
                    hue: 0.55,
                    saturation: 0.7,
                    value: 0.8,
                    alpha: 0.5,
                  }}
                  ref={lab.control("color")}
                  presets={[
                    {
                      value: { hue: 0, saturation: 1, value: 1, alpha: 1 },
                      label: "Red",
                    },
                    {
                      value: { hue: 0.6, saturation: 1, value: 1, alpha: 0.5 },
                      label: "Transparent blue",
                    },
                  ]}
                />
              </Children>
            </Entity>
            <Entity id="values-feedback">
              <Layout
                kind={2}
                width={345}
                height={640}
                align_x={-1}
                align_y={-1}
                margin_left={375}
                margin_top={14}
              />
              <Children>
                <ProgressBar
                  id="progress-basic"
                  label="UPLOADING"
                  value={0.62}
                />
                <ProgressBar
                  id="progress-segments"
                  label="STORAGE"
                  segments={[
                    { value: 0.3 },
                    { value: 0.2, tone: "amber" },
                    { value: 0.12, tone: "error" },
                  ]}
                />
                <ProgressBar
                  id="progress-complete"
                  label="SYNC"
                  value={1}
                  status="complete"
                />
                <ProgressBar
                  id="progress-failed"
                  label="CONNECTION"
                  value={0.45}
                  status="failed"
                />
                <ProgressBar id="progress-unknown" label="WAITING FOR TOTAL" />
                <ProgressBar
                  id="progress-cancelled"
                  label="STOPPED"
                  value={0.3}
                  status="cancelled"
                />
                <Spinner id="spinner" label="Waiting" />
                <Entity id="rings">
                  <Layout kind={1} width={345} height={90} />
                  <Children>
                    <CircularProgress
                      id="ring-active"
                      label="Upload"
                      value={0.62}
                      size="small"
                    />
                    <CircularProgress
                      layout={{ margin_left: 14 }}
                      id="ring-complete"
                      label="Ready"
                      status="complete"
                      size="small"
                    />
                    <CircularProgress
                      layout={{ margin_left: 14 }}
                      id="ring-failed"
                      value={1}
                      label="Upload"
                      status="failed"
                      size="small"
                    />
                  </Children>
                </Entity>
                <Entity id="badges">
                  <Layout kind={1} width={345} height={30} />
                  <Children>
                    <StatusBadge
                      id="badge-active"
                      status="active"
                      label="Online"
                    />
                    <StatusBadge
                      layout={{ margin_left: 8 }}
                      id="badge-inactive"
                      status="inactive"
                      label="Offline"
                    />
                    <StatusBadge
                      layout={{ margin_left: 8 }}
                      id="badge-error"
                      status="error"
                      label="Error"
                    />
                  </Children>
                </Entity>
                <InlineAlert
                  id="alert-info"
                  severity="information"
                  text="Changes saved"
                />
                <InlineAlert
                  id="alert-warning"
                  severity="warning"
                  text="Storage almost full"
                />
                <InlineAlert
                  id="alert-error"
                  severity="error"
                  text="Connection failed"
                />
                <EmptyState id="empty" text="No records" />
              </Children>
            </Entity>
          </Children>
        </Entity>
      </GlassGuiKit>
    ),
  });
}
