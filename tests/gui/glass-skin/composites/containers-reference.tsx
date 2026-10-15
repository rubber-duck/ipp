import type { ReactNode } from "react";
import { Children, Entity } from "@ipp/react";
import {
  Box,
  Button,
  Font,
  Layout,
  Skin,
  Style,
  Text,
  TextInput,
} from "@ipp/react/gui";
import {
  Panel,
  WindowControl,
  Dropdown,
  SecondaryButton,
} from "@ipp/react/gui-kit";
import {
  GlassGuiKit,
  GlassSwitch,
  resolveGlassPalette,
} from "@ipp/react/gui-glass";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";

function placed(
  id: string,
  x: number,
  y: number,
  width: number,
  height: number,
  content: ReactNode,
) {
  return (
    <Entity key={id} id={`${id}/slot`}>
      <Layout
        kind={3}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      <Children>{content}</Children>
    </Entity>
  );
}

/** Isolated two-surface study at the reference's physical pixel scale (capture scale 2). */
export function containersReference(contract: GlassKitContract) {
  const p = resolveGlassPalette();
  return defineSpecimen({
    extent: [302, 270],
    states: [{ name: "panel-and-selected-row", cell: [0, 0, 302, 270] }],
    render: (lab) => {
      const label = (
        id: string,
        text: string,
        x: number,
        y: number,
        width = 110,
        size = 9,
        color = p.text,
      ) =>
        placed(
          id,
          x,
          y,
          width,
          14,
          <Entity id={id}>
            <Layout kind={0} />
            <Style red={color[0]} green={color[1]} blue={color[2]} />
            <Text text={text} source={lab.font} font_size={size} />
          </Entity>,
        );
      const rule = (id: string, y: number, x = 15, width = 239) =>
        placed(
          id,
          x,
          y,
          width,
          0.35,
          <Entity id={id}>
            <Layout kind={0} width={width} height={0.35} />
            <Style
              red={p.secondary[0] * 0.2}
              green={p.secondary[1] * 0.2}
              blue={p.secondary[2] * 0.2}
            />
            <Box width={width} height={0.35} />
          </Entity>,
        );
      return (
        <GlassGuiKit
          contract={contract}
          font={lab.font}
          fontSize={9}
          reducedMotion
        >
          <Entity id="reference/page">
            <Layout kind={3} width={302} height={270} />
            <Children>
              <Entity id="reference/background">
                <Layout kind={0} width={302} height={270} />
                <Style
                  red={p.background[0]}
                  green={p.background[1]}
                  blue={p.background[2]}
                />
                <Box width={302} height={270} />
              </Entity>
              {placed(
                "settings",
                15,
                15,
                272,
                196,
                <Panel
                  id="settings"
                  padding={0}
                  layout={{
                    kind: 3,
                    width: 272,
                    height: 196,
                    padding_left: 0,
                    padding_right: 0,
                  }}
                >
                  {label("title", "Workspace", 16, 10, 160, 10, p.primary)}
                  {(["minimize", "maximize", "close"] as const).map(
                    (kind, index) =>
                      placed(
                        `settings/${kind}`,
                        199 + index * 24,
                        7,
                        20,
                        18,
                        <WindowControl
                          id={`settings/${kind}`}
                          kind={kind}
                          ref={lab.control(kind)}
                          layout={{ width: 20, height: 18 }}
                        />,
                      ),
                  )}
                  {rule("header-rule", 31, 3, 266)}
                  {label("project-label", "Project name", 17, 53)}
                  {placed(
                    "project",
                    106,
                    45,
                    148,
                    23,
                    <Entity id="project">
                      <Layout kind={3} width={148} height={23} />
                      <Font source={lab.font} font_size={9} />
                      <Skin theme="ipp-glass/kit/cellEditor" />
                      <TextInput text="Orion" ref={lab.control("project")} />
                    </Entity>,
                  )}
                  {rule("project-rule", 73)}
                  {label("autosave-label", "Auto save", 17, 87)}
                  {placed(
                    "autosave",
                    106,
                    83,
                    32,
                    16,
                    <Entity id="autosave">
                      <Layout kind={3} width={32} height={16} />
                      <Font source={lab.font} font_size={9} />
                      <GlassSwitch checked ref={lab.control("autosave")} />
                    </Entity>,
                  )}
                  {label("autosave-state", "ON", 147, 87, 30, 9, p.primary)}
                  {rule("autosave-rule", 106)}
                  {label("quality-label", "Render quality", 17, 121)}
                  {placed(
                    "quality",
                    106,
                    113,
                    148,
                    23,
                    <Dropdown
                      id="quality/control"
                      defaultValue="high"
                      options={[
                        { key: "high", label: "High" },
                        { key: "low", label: "Low" },
                      ]}
                      layout={{ width: 148, height: 23 }}
                    />,
                  )}
                  {rule("footer-rule", 151, 3, 266)}
                  {placed(
                    "details",
                    158,
                    161,
                    100,
                    23,
                    <SecondaryButton
                      id="details/control"
                      layout={{ width: 100, height: 23 }}
                      label="Details  ›"
                      ref={lab.control("details")}
                    />,
                  )}
                </Panel>,
              )}
              {placed(
                "selected-row",
                15,
                228,
                237,
                26,
                <Entity id="selected-row">
                  <Layout kind={3} width={237} height={26} />
                  <Font source={lab.font} font_size={9} />
                  <Skin theme="ipp-glass/kit/gridRow" />
                  <Button label="" selected ref={lab.control("row")} />
                  <Children>
                    {label("row-name", "Core-02", 12, 7, 72)}
                    {placed(
                      "row-status-dot",
                      92,
                      10,
                      5,
                      5,
                      <Entity id="row-status-dot">
                        <Layout kind={0} width={5} height={5} />
                        <Style
                          red={p.primary[0]}
                          green={p.primary[1]}
                          blue={p.primary[2]}
                        />
                        <Box
                          width={5}
                          height={5}
                          radius_x={2.5}
                          radius_y={2.5}
                        />
                      </Entity>,
                    )}
                    {label("row-status", "Online", 106, 7, 60, 9, p.primary)}
                    {label("row-load", "62%", 175, 7, 35)}
                  </Children>
                </Entity>,
              )}
            </Children>
          </Entity>
        </GlassGuiKit>
      );
    },
  });
}
