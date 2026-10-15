/** Read-only mock terminal; all glyphs use the bundled Nerd Font. */
import { useMemo } from "react";
import {
  Animation,
  AnimationAsset,
  assetRef,
  Children,
  Resources,
  Entity,
} from "@ipp/react";
import { Box, Layout, Style, Text } from "@ipp/react/gui";
import { Column, Row, TextLine, useGuiKit } from "@ipp/react/gui-kit";
import type { CockpitOptions } from "./options.js";

export function TerminalScreen({ state }: { state: CockpitOptions }) {
  const kit = useGuiKit();
  const style = kit.contract.components.CanvasStyle;
  const blink = useMemo(
    () => ({
      duration: 1.1,
      tracks: [
        {
          property: {
            component: style.id,
            offsets: [style.fields.opacity.offset],
          },
          keys: [
            { time: 0, value: { kind: "f32" as const, value: 0 } },
            { time: 0.54, value: { kind: "f32" as const, value: 0 } },
            { time: 0.55, value: { kind: "f32" as const, value: -1 } },
            { time: 1.09, value: { kind: "f32" as const, value: -1 } },
            { time: 1.1, value: { kind: "f32" as const, value: 0 } },
          ],
        },
      ],
    }),
    [style],
  );
  const lines = [
    ["rafael@arch ~ $ fastfetch", true],
    ["", false],
    ["                  -`                     rafael@arch", true],
    ["                 .o+`                    -----------", true],
    ["                `ooo/                    OS: Arch Linux x86_64", false],
    ["               `+oooo:                   Host: IPP Explorer", false],
    ["              `+oooooo:                  Kernel: 6.12.9-arch1-1", false],
    ["              -+oooooo+:                 Shell: zsh 5.9", false],
    ["            `/:-:++oooo+:                Terminal: IPP Canvas", false],
    [
      "           `/++++/+++++++:               Session: bridge simulation",
      false,
    ],
    [
      "          `/+++++++++++++:              Font: ShureTechMono Nerd Font",
      false,
    ],
    ["         `/+++oooooooooooo/`            ", true],
    ["        ./ooosssso++osssssso+`           ", true],
    ["       .oossssso-````/ossssss+`          ", true],
    ["      -osssssso.      :ssssssso.         ", true],
    ["     :osssssss/        osssso+++.        ", true],
    ["    /ossssssss/        +ssssooo/-        ", true],
    ["  `/ossssso+/:-        -:/+osssso+-      ", true],
    [" `+sso+:-`                 `.-/+oso:     ", true],
    ["`++:.                           `-/+/   ", true],
    [".`                                 `/   ", true],
    ["", false],
    ["rafael@arch ~/bridge $ sensors --status", true],
    [
      `scanner     ${state.scanning ? "active" : "paused"}     range ${state.range.toFixed(1)} km     gain ${Math.round(state.gain * 100)}%`,
      false,
    ],
    [`transmitter ready      pulses sent ${state.pulse}`, false],
    ["", false],
  ] as const;
  return (
    <Column id="cockpit-terminal-transcript" gap={0}>
      {lines.map(([text, accent], index) => (
        <TextLine
          id={`cockpit-terminal-line-${index}`}
          key={index}
          text={text || " "}
          tone={accent ? "accent" : "text"}
          size="small"
          layout={{ height: 20 }}
        />
      ))}
      <Row gap={0} height={24}>
        <PowerlineSegment
          label={"\uf303  rafael"}
          width={102}
          color={[0.025, 0.24, 0.36]}
          next={[0.055, 0.095, 0.14]}
        />
        <PowerlineSegment
          label="~/bridge"
          width={102}
          color={[0.055, 0.095, 0.14]}
          next={[0.025, 0.06, 0.07]}
        />
        <PowerlineSegment
          label={"\ue0a0 main"}
          width={86}
          color={[0.025, 0.06, 0.07]}
        />
        <Entity id="cockpit-terminal-cursor">
          <Layout width={8} height={16} margin_left={8} align_y={0} />
          <Style opacity={1} red={0.72} green={0.86} blue={0.91} />
          <Box width={8} height={16} />
          {!kit.reducedMotion && (
            <Animation
              source={assetRef("cockpit-terminal-blink")}
              target="cockpit-terminal-cursor"
              looping
              autoPlay
            />
          )}
        </Entity>
      </Row>
      <Resources>
        <AnimationAsset id="cockpit-terminal-blink" clip={blink} />
      </Resources>
    </Column>
  );
}
function PowerlineSegment({
  label,
  width,
  color,
  next,
}: {
  label: string;
  width: number;
  color: readonly [number, number, number];
  next?: readonly [number, number, number];
}) {
  const kit = useGuiKit();
  return (
    <Entity>
      <Layout kind={3} width={width} height={24} />
      <Children>
        <Entity>
          <Layout width={width - 12} height={24} />
          <Style red={color[0]} green={color[1]} blue={color[2]} />
          <Box width={width - 12} height={24} />
        </Entity>
        {next && (
          <Entity>
            <Layout width={12} height={24} align_x={1} />
            <Style red={next[0]} green={next[1]} blue={next[2]} />
            <Box width={12} height={24} />
          </Entity>
        )}
        <Entity>
          <Layout width={12} height={24} align_x={1} />
          <Style red={color[0]} green={color[1]} blue={color[2]} />
          <Text text={"\uE0B0"} source={kit.font} font_size={24} />
        </Entity>
        <TextLine
          text={label}
          size="small"
          layout={{ margin_left: 6, align_y: 0 }}
        />
      </Children>
    </Entity>
  );
}
