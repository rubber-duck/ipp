import type { ReactNode } from "react";
import { Asset, Entity, PaintShader, ShaderAsset, assetRef } from "@ipp/react";
import {
  Behavior,
  Box,
  Button,
  Checkbox,
  Font,
  Layout,
  Paint,
  Skin,
  Slider,
  Style,
  Text,
  TextInput,
  type GuiControlHandle,
} from "@ipp/react/gui";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import { COMPONENT_PARAMETERS } from "./component-material.js";
import { ComponentPaint, ComponentThemes } from "./component-themes.js";
import { LIQUID_BACKDROP, LIQUID_PAINT } from "./liquid-material.js";

function Caption({
  id,
  x,
  y,
  text,
  size = 22,
  muted = false,
}: {
  id: string;
  x: number;
  y: number;
  text: string;
  size?: number;
  muted?: boolean;
}) {
  return (
    <Entity id={id}>
      <Style
        x={x}
        y={y}
        layer={1}
        red={muted ? 0.37 : 0.82}
        green={muted ? 0.44 : 0.88}
        blue={muted ? 0.5 : 0.94}
      />
      <Text source={assetRef("font")} font_size={size} text={text} />
    </Entity>
  );
}

function Surface({
  id,
  x,
  y,
  width,
  height,
  radius = 40,
}: {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  radius?: number;
}) {
  return (
    <Entity id={id}>
      <Style x={x} y={y} />
      <Box width={width} height={height} />
      <ComponentPaint radius={radius} roughness={0.42} />
    </Entity>
  );
}

function Control({
  id,
  x,
  y,
  width,
  height,
  theme = "button",
  children,
}: {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  theme?: string;
  children: ReactNode;
}) {
  return (
    <Entity id={id}>
      <Layout
        kind={3}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      <Behavior enabled />
      <Font source={assetRef("font")} font_size={23} />
      <Skin theme={`component-theme-${theme}`} />
      {children}
    </Entity>
  );
}

/** Separate material experiment; production defaults and control metrics are untouched. */
export function LiquidSamples({
  font,
  contract,
  controls,
}: {
  font: Uint8Array<ArrayBuffer>;
  contract: GuiKitContract;
  controls?: Map<string, GuiControlHandle>;
}) {
  const bind = (name: string) => (handle: GuiControlHandle | null) => {
    if (handle) controls?.set(name, handle);
    else controls?.delete(name);
  };
  return (
    <>
      <Asset id="font" kind={17} data={font} encode={(v) => v} />
      <ShaderAsset
        id="component-glass"
        recipe={{ presentation: true, lighting: true }}
        parameters={COMPONENT_PARAMETERS}
      >
        <PaintShader>{LIQUID_PAINT}</PaintShader>
      </ShaderAsset>
      <ShaderAsset id="liquid-backdrop" recipe={{}} parameters={{}}>
        <PaintShader>{LIQUID_BACKDROP}</PaintShader>
      </ShaderAsset>
      <Entity id="sheet-background">
        <Box width={1536} height={1024} />
        <Style />
        <Paint source={assetRef("liquid-backdrop")} />
      </Entity>
      <ComponentThemes contract={contract} liquid />
      <Caption
        id="title"
        x={80}
        y={48}
        text="LIQUID / MATERIAL STUDY"
        size={34}
      />
      <Caption
        id="subtitle"
        x={80}
        y={101}
        text="Smoky body. Soft edges. Light in motion."
        muted
      />
      <Control id="link" x={80} y={190} width={414} height={110}>
        <Button label="Connection" ref={bind("button")} />
      </Control>
      <Control id="focus" x={80} y={324} width={414} height={110}>
        <Button label="Focus" />
      </Control>
      <Control id="autopilot" x={80} y={458} width={414} height={110}>
        <Button label="Autopilot" selected ref={bind("selected")} />
      </Control>
      <Surface id="display-panel" x={542} y={190} width={914} height={378} />
      <Caption id="display-title" x={582} y={228} text="Display" size={30} />
      <Caption
        id="brightness-label"
        x={582}
        y={292}
        text="Cabin illumination"
        muted
      />
      <Control
        id="brightness"
        x={582}
        y={342}
        width={834}
        height={38}
        theme="slider"
      >
        <Slider value={0.56} min={0} max={1} step={0.01} ref={bind("slider")} />
      </Control>
      <Caption id="adaptive-label" x={582} y={442} text="Adaptive lighting" />
      <Control
        id="adaptive"
        x={1312}
        y={426}
        width={104}
        height={48}
        theme="switch"
      >
        <Checkbox checked ref={bind("switch")} />
      </Control>
      <Caption
        id="display-note"
        x={582}
        y={509}
        text="Adjust the light above to explore the edge."
        size={18}
        muted
      />
      <Surface id="message-panel" x={80} y={614} width={1376} height={250} />
      <Caption
        id="message-title"
        x={120}
        y={650}
        text="Communications"
        size={28}
      />
      <Control
        id="message"
        x={120}
        y={714}
        width={888}
        height={64}
        theme="input"
      >
        <TextInput text="Request docking clearance" ref={bind("input")} />
      </Control>
      <Control id="send" x={1040} y={714} width={376} height={64}>
        <Button label="Transmit" />
      </Control>
      <Caption
        id="message-note"
        x={120}
        y={815}
        text="Bridge link connected"
        size={18}
        muted
      />
      <Caption
        id="footer"
        x={80}
        y={924}
        text="Live native controls  /  move the view and light  /  no idle glow"
        size={19}
        muted
      />
    </>
  );
}
