import { useEntityId } from "../../scope/identity.js";
/**
 * Panels hold controls in a skin-owned frame. A panel is a column of its
 * parts: an optional `PanelHeader` with its title and docked buttons,
 * the application's body and separators, and an
 * optional `PanelFooter`. Minimised, it is the same frame unlit, as tall as
 * its header, which is then all it should hold.
 *
 * Header and footer align their content at the container inset. The header
 * is a control-height title strip with docked buttons; the footer holds
 * leading status and trailing actions with compact vertical padding. Each
 * is divided from the body by a panel division.
 */
import { createContext, useContext, type ReactNode } from "react";
import { PresentationGate, Children, Entity } from "../../components.js";
import { Style, Font, Layout } from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { useGuiKit } from "../kit.js";
import {
  LAYOUT_COLUMN,
  Column,
  Row,
  ActionRow,
  useLayoutFields,
  type ContainerProps,
  type GuiKitFlowLayoutProps,
} from "../layout.js";
import { Separator } from "../separator.js";
import { TextLine } from "../text.js";

const PanelContext = createContext({
  minimized: false,
  surface: false,
  padding: [0, 0, 0, 0] as readonly [number, number, number, number],
});
export function usePanelSurface(): boolean {
  return useContext(PanelContext).surface;
}

/** Resolved top/right/bottom/left insets supplied by the nearest Panel. */
export function usePanelPadding(): readonly [number, number, number, number] {
  return useContext(PanelContext).padding;
}

export interface PanelProps extends GuiKitFlowLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id?: string;
  /** Draw the frame unlit and only as tall as its header. */
  readonly minimized?: boolean;
  readonly children?: ReactNode;
}

/**
 * An unpadded frame whose header, body and footer own their content insets.
 * Use PanelBody for ordinary content or padding="inset" for a simple surface.
 */
export function Panel({
  id: providedId,
  layer = 0,
  minimized = false,
  children,
  ...layout
}: PanelProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const t = kit.tokens;
  const fields = useLayoutFields(layout, {
    fit_height: true,
  });
  const padding = [
    fields.padding_top!,
    fields.padding_right!,
    fields.padding_bottom!,
    fields.padding_left!,
  ] as const;
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        {...fields}
        {...(minimized ? { height: kit.unit(t.controlHeight) } : {})}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Skin theme={kit.theme(minimized ? "containerUnlit" : "container")} />
      {children !== undefined && (
        <Children>
          <PanelContext value={{ minimized, surface: true, padding }}>
            {children}
          </PanelContext>
        </Children>
      )}
    </Entity>
  );
}

export type PanelBodyProps = ContainerProps;

/** Padded content inside an explicitly unpadded frame, header or tab composition. */
export function PanelBody(props: PanelBodyProps) {
  const kit = useGuiKit();
  return (
    <Column
      {...props}
      padding={props.padding ?? kit.spacing.inset}
      gap={props.gap ?? kit.spacing.field}
    />
  );
}

export interface PanelHeaderProps extends GuiKitFlowLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id?: string;
  readonly title: string;
  /** Docked buttons at the header's end, such as `WindowControls`. */
  readonly children?: ReactNode;
}

/**
 * The panel's title strip: the title in the accent at the content inset, or
 * in the text colour while the panel is minimised, then its docked buttons,
 * and the division below it unless minimised.
 */
export function PanelHeader({
  id: providedId,
  layer = 0,
  title,
  children,
  ...layout
}: PanelHeaderProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const t = kit.tokens;
  const { minimized } = useContext(PanelContext);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        {...useLayoutFields(layout, {
          height: kit.unit(t.controlHeight + (minimized ? 0 : t.lineWidth)),
        })}
      />
      <Children>
        <Row
          id={`${id}/row`}
          height={kit.unit(t.controlHeight)}
          layout={{
            padding_left: kit.spacing.inset,
            padding_right: kit.spacing.inset,
          }}
        >
          <TextLine
            id={`${id}/title`}
            text={title}
            tone={minimized ? "text" : "accent"}
            layout={{ flex: 1 }}
          />
          {children}
        </Row>
        {!minimized && <Separator id={`${id}/separator`} />}
      </Children>
    </Entity>
  );
}

export interface PanelFooterProps extends GuiKitFlowLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id?: string;
  /** Status or descriptive content at the leading content edge. */
  readonly leading?: ReactNode;
  /** Actions placed together at the trailing content edge. */
  readonly children?: ReactNode;
}

/**
 * The panel's last row, after a division: actions with label spacing above
 * and below, and the content inset at either end. Give the body before it `flex: 1` to
 * hold it at the bottom of a taller panel.
 */
export function PanelFooter({
  id: providedId,
  layer = 0,
  children,
  leading,
  ...layout
}: PanelFooterProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        {...useLayoutFields(layout, { fit_height: true })}
      />
      <Children>
        <Separator id={`${id}/separator`} />
        <ActionRow
          id={`${id}/row`}
          leading={leading}
          layout={{
            padding_left: kit.spacing.inset,
            padding_right: kit.spacing.inset,
            padding_top: kit.spacing.label,
            padding_bottom: kit.spacing.label,
          }}
        >
          {children}
        </ActionRow>
      </Children>
    </Entity>
  );
}
