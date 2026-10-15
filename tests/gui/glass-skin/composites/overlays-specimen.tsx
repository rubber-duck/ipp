import { useState } from "react";
import { Entity, Children } from "@ipp/react";
import { Box, Font, Layout, Style } from "@ipp/react/gui";
import {
  Menu,
  Floating,
  Popover,
  Tooltip,
  ConfirmationDialog,
  ToastStack,
  ContextMenu,
  useContextMenu,
  TextLine,
} from "@ipp/react/gui-kit";
import { GlassGuiKit, GlassButton } from "@ipp/react/gui-glass";
import { resolveGlassPalette } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import type {
  GlassLight,
  GlassPaletteInput,
} from "../../../../packages/ipp-react/src/gui-glass/types.js";
import {
  defineSpecimen,
  type SpecimenContext,
} from "../../skin-lab/specimen.js";
import type { ThemeContract } from "../../skin-lab/theme.js";

export const OVERLAYS_EXTENT = [900, 640] as const;
export type OverlayVariant = "overview" | "dialog" | "toast";
const items = [
  { key: "open", label: "Open" },
  { key: "copy", label: "Copy" },
  { key: "delete", label: "Delete", tone: "amber" as const, separator: true },
  { key: "off", label: "Unavailable", disabled: true },
];
function Content({ dialog }: { dialog: boolean }) {
  const context = useContextMenu<string>();
  const [open, setOpen] = useState(dialog);
  return (
    <>
      <Entity id="menu-anchor">
        <Layout
          width={240}
          height={40}
          margin_left={20}
          margin_top={30}
          align_x={-1}
          align_y={-1}
        />
        <Children>
          <Floating
            id="glass-menu-surface"
            mode="manual"
            open
            side="top"
            layout={{ width: 240 }}
          >
            <Menu id="glass-menu" items={items} />
          </Floating>
        </Children>
      </Entity>
      <Entity id="popover-anchor">
        <Layout
          kind={2}
          width={240}
          height={60}
          margin_left={320}
          margin_top={30}
          align_x={-1}
          align_y={-1}
        />
        <Children>
          <Popover id="glass-popover" label="Details" title="DETAILS">
            <TextLine id="popover-text" text="Native focus and dismissal" />
          </Popover>
        </Children>
      </Entity>
      <Entity id="tooltip-anchor">
        <Layout
          width={180}
          height={40}
          margin_left={620}
          margin_top={30}
          align_x={-1}
          align_y={-1}
        />
        <GlassButton label="Hover for help" />
        <Children>
          <Tooltip id="glass-tooltip" text="A quiet glass hint" side="bottom" />
        </Children>
      </Entity>
      <Entity id="context-target">
        <Layout
          width={240}
          height={60}
          margin_left={320}
          margin_top={210}
          align_x={-1}
          align_y={-1}
        />
        <GlassButton
          label="Context commands"
          onContextMenu={context.opener("scene")}
        />
      </Entity>
      <ContextMenu id="glass-context" menu={context} items={items} />
      <Entity id="dialog-trigger">
        <Layout
          width={240}
          height={40}
          margin_left={20}
          margin_top={310}
          align_x={-1}
          align_y={-1}
        />
        <GlassButton label="Confirm action" onPress={() => setOpen(true)} />
      </Entity>
      <ConfirmationDialog
        id="glass-dialog"
        open={open}
        title="APPLY CHANGES"
        body="The native dialog owns focus."
        action="Apply"
        destructive={false}
        onConfirm={() => setOpen(false)}
        onCancel={() => setOpen(false)}
      />
    </>
  );
}
function ToastMessages() {
  return (
    <ToastStack
      id="glass-toasts"
      toasts={[
        {
          key: "saved",
          text: "Changes saved",
          severity: "success",
          persistent: true,
        },
        {
          key: "warning",
          text: "Review before continuing",
          severity: "warning",
          persistent: true,
        },
        {
          key: "error",
          text: "Action unavailable",
          severity: "error",
          persistent: true,
        },
      ]}
      side="top"
      align="end"
      layout={{ width: 260 }}
    />
  );
}
export function overlaysSpecimen(
  contract: GlassKitContract & ThemeContract,
  variant: OverlayVariant = "overview",
  palette: Partial<GlassPaletteInput> = {},
  light: Partial<GlassLight> = {},
) {
  const referencePalette = { primary: "#00D9FF", ...palette };
  const tokens = {
    controlHeight: 48.8,
    row: 44.8,
    icon: 32,
    cut: 7.2,
    partCut: 7.2,
  };
  const p = resolveGlassPalette(referencePalette);
  return defineSpecimen({
    extent: OVERLAYS_EXTENT,
    states: [{ name: "overlays", cell: [0, 0, ...OVERLAYS_EXTENT] }],
    render: (lab: SpecimenContext) => (
      <GlassGuiKit
        contract={contract}
        tokens={tokens}
        font={lab.font}
        fontSize={10}
        palette={referencePalette}
        light={light}
        reducedMotion
      >
        <Entity id="overlay-page">
          <Layout width={900} height={640} align_x={-1} align_y={-1} />
          <Style
            red={p.background[0]}
            green={p.background[1]}
            blue={p.background[2]}
            alpha={1}
          />
          <Box width={900} height={640} />
        </Entity>
        <Entity id="overlay-content">
          <Layout kind={3} width={900} height={640} align_x={-1} align_y={-1} />
          <Font source={lab.font} font_size={10} />
          <Children>
            <Content key={variant} dialog={variant === "dialog"} />
          </Children>
        </Entity>
        {variant === "toast" && (
          <Entity id="toast-anchor">
            <Layout
              width={880}
              height={0}
              margin_top={160}
              align_x={-1}
              align_y={-1}
            />
            <Children>
              <ToastMessages />
            </Children>
          </Entity>
        )}
      </GlassGuiKit>
    ),
  });
}
