/**
 * Panels: containers that hold controls rather than act, so they are uncut,
 * a thin accent frame with corner accents on all four corners, filled with
 * the page. A panel is a column of its parts: an optional `PanelHeader` with
 * its title and docked buttons, the application's body and separators, and an
 * optional `PanelFooter`. Minimised, it is the same frame unlit, as tall as
 * its header, which is then all it should hold.
 *
 * Header and footer are rows of buttons in half-inset margins: the header a
 * control-height strip whose docked buttons sit as far from its end as from
 * its edges, the footer a row of small buttons at the content inset. Each is
 * divided from the body by a panel division.
 */
import { createContext, useContext, type ReactNode } from "react";
import { Children, Entity } from "../../components.js";
import { Style, Font, Layout } from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { useGuiKit } from "../kit.js";
import { LAYOUT_COLUMN, Row, type GuiKitLayout } from "../layout.js";
import { Separator } from "../separator.js";
import { TextLine } from "../text.js";

const PanelContext = createContext({ minimized: false });

export interface PanelProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id: string;
  /** Draw the frame unlit and only as tall as its header. */
  readonly minimized?: boolean;
  readonly layout?: GuiKitLayout;
  readonly children?: ReactNode;
}

/**
 * The container frame. Its parts are laid out in a column inside its line,
 * so separators meet the frame without covering it.
 */
export function Panel({
  id,
  layer = 0,
  minimized = false,
  layout,
  children,
}: PanelProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const line = kit.unit(t.lineWidth);
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        padding_left={line}
        padding_right={line}
        {...layout}
        {...(minimized ? { height: kit.unit(t.controlHeight) } : {})}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Skin theme={kit.theme(minimized ? "containerUnlit" : "container")} />
      {children !== undefined && (
        <Children>
          <PanelContext value={{ minimized }}>{children}</PanelContext>
        </Children>
      )}
    </Entity>
  );
}

export interface PanelHeaderProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id: string;
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
  id,
  layer = 0,
  title,
  children,
}: PanelHeaderProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const { minimized } = useContext(PanelContext);
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        height={kit.unit(t.controlHeight + (minimized ? 0 : t.lineWidth))}
      />
      <Children>
        <Row
          id={`${id}/row`}
          height={t.controlHeight}
          layout={{
            padding_left: kit.unit(t.inset),
            padding_right: kit.unit((t.controlHeight - t.dockedHeight) / 2),
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
  readonly id: string;
  /** The footer's row content, such as a count and secondary buttons. */
  readonly children?: ReactNode;
}

/**
 * The panel's last row, after a division: small buttons in half-inset margins
 * with the content inset at either end. Give the body before it `flex: 1` to
 * hold it at the bottom of a taller panel.
 */
export function PanelFooter({ id, layer = 0, children }: PanelFooterProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        height={kit.unit(t.smallHeight + t.inset + t.lineWidth)}
      />
      <Children>
        <Separator id={`${id}/separator`} />
        <Row
          id={`${id}/row`}
          height={t.smallHeight + t.inset}
          layout={{
            padding_left: kit.unit(t.inset),
            padding_right: kit.unit(t.inset),
          }}
        >
          {children}
        </Row>
      </Children>
    </Entity>
  );
}
