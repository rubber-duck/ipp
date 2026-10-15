import { Fragment, useState, type ReactNode } from "react";
import {
  Asset,
  Children,
  Entity,
  PaintShader,
  ShaderAsset,
  assetRef,
} from "@ipp/react";
import {
  Behavior,
  Box,
  Button,
  Checkbox,
  Font,
  Group,
  Layout,
  Overlay,
  Paint,
  Skin,
  Slider,
  Style,
  Text,
  TextInput,
  type GuiControlHandle,
} from "@ipp/react/gui";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import { COMPONENT_PARAMETERS, COMPONENT_PAINT } from "./component-material.js";
import {
  ComponentPaint,
  ComponentThemes,
  DROPDOWN_TRIGGER_HEIGHT,
  DROPDOWN_MENU_HEIGHT,
} from "./component-themes.js";
import {
  REGION_PARAMETERS,
  REGION_PAINT,
  SurfaceRegion,
} from "./surface-region.js";

export const COMPONENT_EXTENT = [1536, 1024] as const;

function Label({
  id,
  x,
  y,
  text,
  size = 18,
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
        red={muted ? 0.3 : 0.65}
        green={muted ? 0.48 : 0.83}
        blue={muted ? 0.55 : 0.87}
      />
      <Text source={assetRef("font")} text={text} font_size={size} />
    </Entity>
  );
}

function At({
  id,
  x,
  y,
  width,
  height,
  theme = "button",
  children,
  disabled = false,
}: {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  theme?: string;
  children: ReactNode;
  disabled?: boolean;
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
      <Behavior enabled={!disabled} />
      <Font source={assetRef("font")} font_size={20} />
      <Skin theme={`component-theme-${theme}`} />
      {children}
    </Entity>
  );
}

function Sheet({
  id,
  x,
  y,
  width,
  height,
  roughness = 0.2,
  radius = 12,
  fill = 0,
  tail = false,
}: {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  roughness?: number;
  radius?: number;
  fill?: number;
  tail?: boolean;
}) {
  return (
    <Entity id={id}>
      <Style x={x} y={y} />
      <Box width={width} height={height} />
      <ComponentPaint
        radius={radius}
        roughness={roughness}
        fill={fill}
        lens={tail ? -2 : 0}
      />
    </Entity>
  );
}

function Mark({
  id,
  x,
  y,
  mode,
}: {
  id: string;
  x: number;
  y: number;
  mode: number;
}) {
  return (
    <Entity id={id}>
      <Style x={x} y={y} layer={1} />
      <Box width={26} height={26} />
      <Paint source={assetRef("component-mark")} mark={[mode, 0, 0, 0]} />
    </Entity>
  );
}

function NativeTabs({
  id,
  x,
  y,
  width,
  height,
  labels,
  selected,
  onSelect,
  contract,
  surfaceHeight = height,
  controls,
}: {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  labels: readonly string[];
  selected: number;
  onSelect?: (index: number) => void;
  contract: GuiKitContract;
  surfaceHeight?: number;
  controls?: Map<string, GuiControlHandle> | undefined;
}) {
  return (
    <Entity id={id}>
      <Layout
        kind={1}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      <Group axis={0} selection={1} />
      <Children>
        {labels.map((label, i) => (
          <Entity key={label} id={`${id}-${i}`}>
            <Layout width={width / labels.length} height={height} />
            <Font source={assetRef("font")} font_size={18} />
            <SurfaceRegion
              contract={contract}
              surface={[(i * width) / labels.length, 0, width, surfaceHeight]}
              leadingDivider={i > 0}
              bottomDivider={surfaceHeight > height}
            />
            <Button
              label={label}
              selected={i === selected}
              onPress={() => onSelect?.(i)}
              ref={(handle) => {
                if (handle) controls?.set(`${id}-${i}`, handle);
                else controls?.delete(`${id}-${i}`);
              }}
            />
          </Entity>
        ))}
      </Children>
    </Entity>
  );
}

/** A component specimen, not a replacement for the production Glass skin. */
export function ComponentSamples({
  font,
  contract,
  controls,
}: {
  font: Uint8Array<ArrayBuffer>;
  contract: GuiKitContract;
  controls?: Map<string, GuiControlHandle>;
}) {
  const [tab, setTab] = useState(0);
  const [segment, setSegment] = useState(1);
  const [finish, setFinish] = useState(0);
  const [opened, setOpened] = useState(false);
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const bind = (id: string) => (handle: GuiControlHandle | null) => {
    if (handle) controls?.set(id, handle);
    else controls?.delete(id);
  };
  return (
    <>
      <Asset id="font" kind={17} data={font} encode={(v) => v} />
      <ShaderAsset
        id="component-glass"
        recipe={{ presentation: true, lighting: true }}
        parameters={COMPONENT_PARAMETERS}
      >
        <PaintShader>{COMPONENT_PAINT}</PaintShader>
      </ShaderAsset>
      <ShaderAsset
        id="component-region"
        recipe={{}}
        parameters={REGION_PARAMETERS}
      >
        <PaintShader>{REGION_PAINT}</PaintShader>
      </ShaderAsset>
      <ShaderAsset
        id="component-mark"
        recipe={{}}
        parameters={{ mark: "vec4" }}
      >
        <PaintShader>{`
      vec2 a=p_mark.x<.5?vec2(16,16):vec2(4,8),b=p_mark.x<.5?vec2(23,23):vec2(13,17);
      vec2 pa=position-a,ba=b-a;
      float d=length(pa-ba*clamp(dot(pa,ba)/dot(ba,ba),0.0,1.0));
      if(p_mark.x<.5) d=min(d,abs(length(position-vec2(10,10))-8.0));
      else {a=vec2(13,17);b=vec2(22,8);pa=position-a;ba=b-a;d=min(d,length(pa-ba*clamp(dot(pa,ba)/dot(ba,ba),0.0,1.0)));}
      return vec4(.65,.83,.87,1.0-smoothstep(.7,1.7,d));
    `}</PaintShader>
      </ShaderAsset>
      <ShaderAsset id="component-backdrop" recipe={{}} parameters={{}}>
        <PaintShader>{`
      vec2 p=position/size;
      float pool=exp(-dot((p-vec2(.38,.3))*vec2(1.5,1.5),(p-vec2(.38,.3))*vec2(1.5,1.5)));
      float ribbon=exp(-pow((p.x-p.y*.58-.23)/.06,2.0));
      return vec4(vec3(.002,.006,.01)+vec3(.004,.014,.018)*pool+vec3(.005,.008,.009)*ribbon,1);
    `}</PaintShader>
      </ShaderAsset>
      <Entity id="sheet-background">
        <Box width={1536} height={1024} />
        <Style />
        <Paint source={assetRef("component-backdrop")} />
      </Entity>
      <ComponentThemes contract={contract} />
      <Label
        id="heading"
        x={64}
        y={40}
        size={34}
        text="GLASS / COMPONENT STUDY"
      />
      <Label
        id="subheading"
        x={64}
        y={88}
        size={22}
        text="Clear · Frosted · Smoked"
        muted
      />
      {[
        "01  BUTTONS",
        "02  CHOICES",
        "03  INPUTS",
        "04  VALUES",
        "05  SURFACES",
      ].map((label, i) => (
        <Label
          key={label}
          id={`section-${i}`}
          x={64}
          y={[168, 336, 490, 646, 800][i]!}
          text={label}
          size={18}
        />
      ))}

      {["Rest", "Hover", "Pressed", "Focus", "Disabled"].map((label, i) => (
        <Fragment key={label}>
          <Label
            key={`caption-${label}`}
            id={`button-caption-${i}`}
            x={230 + i * 258}
            y={155}
            text={label}
            muted
          />
          <At
            key={`control-${label}`}
            id={`sample-button-${i}`}
            x={230 + i * 258}
            y={187}
            width={214}
            height={76}
            theme={["button", "hover", "pressed", "focus", "disabled"][i]!}
            disabled={i === 4}
          >
            <Button label="Action" />
          </At>
        </Fragment>
      ))}

      <Label id="switch-off-label" x={230} y={332} text="Switch Off" muted />
      <Label id="switch-on-label" x={397} y={332} text="Switch On" muted />
      <At
        id="switch-off"
        x={230}
        y={365}
        width={108}
        height={46}
        theme="switch"
      >
        <Checkbox label="" checked={false} ref={bind("switch")} />
      </At>
      <At id="switch-on" x={397} y={365} width={100} height={46} theme="switch">
        <Checkbox label="" checked />
      </At>
      <Label id="checkbox-label" x={552} y={332} text="Checkbox" muted />
      <Label id="checked-label" x={682} y={332} text="Checked" muted />
      <At
        id="check-off"
        x={552}
        y={365}
        width={46}
        height={46}
        theme="checkbox"
      >
        <Checkbox label="" checked={false} />
      </At>
      <At id="check-on" x={682} y={365} width={46} height={46} theme="checkbox">
        <Checkbox label="" checked />
      </At>
      <Label id="radio-off-label" x={815} y={332} text="Radio Off" muted />
      <Label id="radio-on-label" x={943} y={332} text="Radio On" muted />
      <Entity id="radio-group">
        <Layout
          kind={1}
          width={174}
          height={46}
          margin_left={815}
          margin_top={365}
          align_x={-1}
          align_y={-1}
        />
        <Group axis={0} selection={1} />
        <Children>
          {[false, true].map((selected, i) => (
            <Entity key={i} id={`radio-${i}`}>
              <Layout width={46} height={46} margin_left={i === 1 ? 82 : 0} />
              <Skin theme="component-theme-radio" />
              <Button label="" selected={selected} />
            </Entity>
          ))}
        </Children>
      </Entity>
      <Label
        id="segmented-label"
        x={1103}
        y={332}
        text="Segmented Control"
        muted
      />
      <Sheet id="segmented-bed" x={1103} y={359} width={372} height={61} />
      <NativeTabs
        id="segment"
        x={1103}
        y={359}
        width={372}
        height={61}
        labels={["Day", "Week", "Month"]}
        selected={segment}
        onSelect={setSegment}
        contract={contract}
        controls={controls}
      />

      <Label id="search-label" x={230} y={475} text="Search Field" muted />
      <Label id="input-label" x={635} y={475} text="Text Field" muted />
      <Label id="dropdown-label" x={1084} y={475} text="Dropdown" muted />
      <At id="search" x={230} y={502} width={364} height={65} theme="search">
        <TextInput text="" placeholder="Search" />
      </At>
      <Mark id="search-icon" x={252} y={521} mode={0} />
      <At id="text-field" x={635} y={502} width={404} height={65} theme="input">
        <TextInput text="Project name" ref={bind("input")} />
      </At>
      <At
        id="dropdown"
        x={1084}
        y={502}
        width={391}
        height={DROPDOWN_TRIGGER_HEIGHT}
        theme={dropdownOpen ? "dropdown-open" : "button"}
      >
        <Button
          label={["Clear glass", "Frosted glass", "Smoked glass"][finish]}
          selected={dropdownOpen}
          ref={bind("dropdown")}
          onPress={() => setDropdownOpen(!dropdownOpen)}
        />
        <Children>
          <Entity id="dropdown-menu">
            <Layout
              kind={2}
              width={391}
              height={DROPDOWN_MENU_HEIGHT}
              padding_top={6}
              padding_bottom={6}
              padding_left={6}
              padding_right={6}
            />
            <Overlay mode={1} side={0} align={0} />
            <Behavior
              visible={dropdownOpen}
              onVisibleChange={({ value }) => setDropdownOpen(value)}
            />
            <Skin theme="component-theme-menu" />
            <Group axis={1} selection={1} />
            <Children>
              {["Clear glass", "Frosted glass", "Smoked glass"].map(
                (label, i) => (
                  <Entity key={label} id={`finish-option-${i}`}>
                    <Layout width={379} height={46} />
                    <Font source={assetRef("font")} font_size={20} />
                    <SurfaceRegion
                      contract={contract}
                      surface={[
                        6,
                        DROPDOWN_TRIGGER_HEIGHT + 6 + i * 46,
                        391,
                        DROPDOWN_TRIGGER_HEIGHT + DROPDOWN_MENU_HEIGHT,
                      ]}
                    />
                    <Button
                      label={label}
                      selected={finish === i}
                      onPress={() => {
                        setFinish(i);
                        setDropdownOpen(false);
                      }}
                    />
                  </Entity>
                ),
              )}
            </Children>
          </Entity>
        </Children>
      </At>
      <Mark id="dropdown-chevron" x={1435} y={521} mode={1} />

      <Label id="slider-label" x={230} y={633} text="Slider (Single)" muted />
      <Label id="range-label" x={507} y={633} text="Slider (Range)" muted />
      <Label id="progress-label" x={780} y={633} text="Progress" muted />
      <At
        id="slider-single"
        x={218}
        y={660}
        width={246}
        height={40}
        theme="slider"
      >
        <Slider min={0} max={100} step={1} value={58} ref={bind("slider")} />
      </At>
      <At
        id="slider-range"
        x={495}
        y={660}
        width={244}
        height={40}
        theme="slider"
      >
        <Slider min={0} max={100} step={1} value={18} upper={72} range />
      </At>
      <Sheet
        id="progress-bed"
        x={780}
        y={675}
        width={200}
        height={12}
        radius={999}
      />
      <Sheet
        id="progress-fill"
        x={780}
        y={675}
        width={128}
        height={12}
        radius={999}
        fill={1}
      />
      <Label id="progress-value" x={995} y={671} text="64%" />
      <Label id="chips-label" x={1095} y={633} text="Status Chips" muted />
      {["Ready", "Paused", "Offline"].map((text, i) => (
        <Fragment key={text}>
          <Sheet
            key={`chip-${text}`}
            id={`chip-${i}`}
            x={1095 + i * 129}
            y={664}
            width={118}
            height={46}
            radius={999}
          />
          <Entity key={`dot-${text}`} id={`chip-dot-${i}`}>
            <Style
              x={1110 + i * 129}
              y={680}
              red={[0.14, 0.95, 0.83][i]!}
              green={[0.56, 0.6, 0.16][i]!}
              blue={[0.38, 0.16, 0.19][i]!}
            />
            <Box width={13} height={13} radius_x={6.5} radius_y={6.5} />
          </Entity>
          <Label
            key={`label-${text}`}
            id={`chip-text-${i}`}
            x={1135 + i * 129}
            y={678}
            text={text}
            size={18}
          />
        </Fragment>
      ))}

      <Sheet id="card" x={230} y={775} width={390} height={171} />
      <Label id="card-title" x={256} y={806} text="Surface" size={25} />
      <Label
        id="card-line-1"
        x={256}
        y={851}
        text={opened ? "A responsive surface." : "A clean sheet of glass,"}
        muted
      />
      <Label id="card-line-2" x={256} y={878} text="ready for content." muted />
      <At id="card-button" x={489} y={872} width={102} height={47}>
        <Button
          label={opened ? "Close" : "Open"}
          onPress={() => setOpened(!opened)}
        />
      </At>
      <Sheet id="tabs-panel" x={665} y={775} width={550} height={171} />
      <NativeTabs
        id="tabs"
        x={665}
        y={775}
        width={550}
        height={58}
        surfaceHeight={171}
        labels={["Overview", "Activity", "Settings"]}
        selected={tab}
        onSelect={setTab}
        contract={contract}
        controls={controls}
      />
      <Label
        id="tab-title"
        x={692}
        y={867}
        text={
          [
            "Live surface optics",
            "Native state transitions",
            "Clear, frosted and smoked",
          ][tab]!
        }
        size={22}
      />
      <Label
        id="tab-description"
        x={692}
        y={904}
        text="Move the light to explore the material."
        muted
        size={17}
      />
      <Sheet
        id="tooltip"
        x={1268}
        y={805}
        width={188}
        height={88}
        roughness={0.5}
        tail
      />
      <Label id="tooltip-text" x={1294} y={830} text="Adjust value" size={19} />
      <Label
        id="footnote"
        x={64}
        y={984}
        text="Button row: pinned state samples. Switches, fields, sliders and tabs: live native controls."
        size={16}
        muted
      />
    </>
  );
}
