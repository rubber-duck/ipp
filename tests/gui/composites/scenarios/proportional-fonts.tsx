/** Shared font specimen: the same declarations run through native and browser Hosts. */
import { useMemo } from "react";
import { Children, Entity } from "@ipp/react";
import { Box, Layout, Style, Text } from "@ipp/react/gui";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { glassSkin } from "@ipp/react/gui-glass";
import {
  Button,
  Checkbox,
  Column,
  GuiKit,
  Icon,
  NumericStepper,
  Row,
  StatusBadge,
  Tabs,
  TextField,
  TextLine,
  type GuiKitContract,
} from "@ipp/react/gui-kit";

export const FONT_CANVAS = [960, 640] as const;
export type FontTheme = "cybertron" | "glass";

export function ProportionalFonts({
  contract,
  font,
  iconFont,
  theme = "cybertron",
}: {
  readonly contract: GuiKitContract;
  readonly font: string;
  readonly iconFont: string;
  readonly theme?: FontTheme;
}) {
  const skin = useMemo(
    () =>
      theme === "glass"
        ? glassSkin(contract as Parameters<typeof glassSkin>[0])
        : cybertronSkin(contract),
    [contract, theme],
  );
  return (
    <GuiKit
      contract={contract}
      skin={skin}
      font={font}
      iconFont={iconFont}
      fontSize={18}
      reducedMotion
    >
      <Entity id="font-page">
        <Layout
          kind={3}
          width={FONT_CANVAS[0]}
          height={FONT_CANVAS[1]}
          align_x={-1}
          align_y={-1}
        />
        <Children>
          <Entity id="backdrop">
            <Layout width={FONT_CANVAS[0]} height={FONT_CANVAS[1]} />
            <Style red={0.008} green={0.017} blue={0.035} />
            <Box width={FONT_CANVAS[0]} height={FONT_CANVAS[1]} />
          </Entity>
          <Column
            id="font-content"
            width={880}
            gap={22}
            layout={{
              margin_left: 40,
              margin_top: 32,
              align_x: -1,
              align_y: -1,
            }}
          >
            <Row gap={12}>
              <Icon id="font-icon" icon="information" />
              <TextLine
                text="Workspace settings"
                size="display"
                tone="accent"
              />
            </Row>
            <TextLine
              text="Noto Sans  /  Proportional type  /  Runtime text layout"
              tone="neutral"
            />
            <Row gap={12}>
              <StatusBadge id="online" status="active" label="Connected" />
              <StatusBadge status="warning" label="Unsaved changes" />
              <StatusBadge status="inactive" label="Preview" />
            </Row>
            <Tabs
              id="font-tabs"
              width={880}
              height={72}
              tabs={[
                {
                  value: "general",
                  label: "General",
                  content: (
                    <TextLine text="Personalise this workspace and its presentation." />
                  ),
                },
                { value: "appearance", label: "Appearance" },
                { value: "advanced", label: "Advanced settings" },
              ]}
              defaultValue="general"
            />
            <Row gap={40} alignItems="start">
              <Column width={410} gap={18}>
                <TextField
                  id="workspace-name"
                  label="Workspace name"
                  defaultValue="Willow Studio"
                  description="A name your team can recognise."
                />
                <Checkbox
                  id="font-check"
                  label="Show annotations in the viewport"
                  defaultChecked
                />
                <NumericStepper
                  id="font-number"
                  label="Preview scale"
                  defaultValue={125}
                  min={50}
                  max={200}
                  units="%"
                />
              </Column>
              <Column width={410} gap={18}>
                <TextLine
                  text="Different letters, different advances"
                  tone="accent"
                />
                <Row gap={12}>
                  <Button id="narrow" label="iiii" />
                  <Button id="wide" label="WWWW" />
                  <Button id="kerned" label="AVATAR" />
                </Row>
                <TextLine
                  text="Fitted controls follow the actual glyph widths."
                  size="small"
                  tone="neutral"
                />
                <TextLine text="AV  To  Wa   ·   0123456789" size="display" />
                <TextLine text="Café · Ångström · naïve · résumé" />
              </Column>
            </Row>
            <Row gap={12}>
              <Button id="save" label="Save changes" />
              <Button id="cancel" label="Cancel" variant="secondary" />
              <TextLine
                text={theme === "glass" ? "Glass skin" : "Cybertron skin"}
                tone="neutral"
                size="small"
                layout={{ margin_left: 20 }}
              />
            </Row>
          </Column>
        </Children>
      </Entity>
      {/* Unconstrained runtime measurements are ordinary text bounds, independent of layout. */}
      {[
        ["probe-i", "iiii"],
        ["probe-w", "WWWW"],
        ["probe-av", "AV"],
      ].map(([id, text]) => (
        <Entity key={id} id={id!}>
          <Style opacity={0} />
          <Text source={font} font_size={18} text={text!} />
        </Entity>
      ))}
    </GuiKit>
  );
}
