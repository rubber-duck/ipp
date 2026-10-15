import { useEntityId } from "../../scope/identity.js";
/**
 * Panels hold controls in a skin-owned frame. A panel is a column of its
 * parts: an optional `PanelHeader` with its title and docked buttons,
 * the application's body and separators, and an
 * optional `PanelFooter`. Minimised, it is the same frame unlit, as tall as
 * its header, which is then all it should hold.
 *
 * Header and footer are rows of buttons in half-inset margins: the header a
 * control-height strip whose docked buttons sit as far from its end as from
 * its edges, the footer a row of small buttons at the content inset. Each is
 * divided from the body by a panel division.
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
  layoutFields,
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
 * A padded content container. Use padding={0} for a frame whose header,
 * separators or tab rail meet its edges, and PanelBody for its padded body.
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
  const inset = kit.spacing.inset;
  const fields = layoutFields(layout, {
    fit_height: true,
    padding_left: inset,
    padding_right: inset,
    padding_top: inset,
    padding_bottom: inset,
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

export interface PanelHeaderProps {
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
        height={kit.unit(t.controlHeight + (minimized ? 0 : t.lineWidth))}
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

export interface PanelFooterProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id?: string;
  /** The footer's row content, such as a count and secondary buttons. */
  readonly children?: ReactNode;
}

/**
 * The panel's last row, after a division: small buttons in half-inset margins
 * with the content inset at either end. Give the body before it `flex: 1` to
 * hold it at the bottom of a taller panel.
 */
export function PanelFooter({
  id: providedId,
  layer = 0,
  children,
}: PanelFooterProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const t = kit.tokens;
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        height={kit.unit(t.smallHeight + t.inset + t.lineWidth)}
      />
      <Children>
        <Separator id={`${id}/separator`} />
        <Row
          id={`${id}/row`}
          height={kit.unit(t.smallHeight + t.inset)}
          layout={{
            padding_left: kit.spacing.inset,
            padding_right: kit.spacing.inset,
          }}
        >
          {children}
        </Row>
      </Children>
    </Entity>
  );
}
