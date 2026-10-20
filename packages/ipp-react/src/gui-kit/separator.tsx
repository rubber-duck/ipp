import {
  KitLayout,
  useGuiKitLayout,
  type GuiKitLayoutProps,
  LAYOUT_COLUMN,
  LAYOUT_ROW,
  Row,
} from "./layout.js";

import { useEntityId } from "../scope/identity.js";
/**
 * Lines one idle line weight thick whose layout box is the line, filling their
 * container across: horizontal in a column, or vertical in a row; and the
 * labelled separator that names a section.
 */
import { PresentationGate, Entity } from "../components.js";
import { Style } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { useGuiKit } from "./kit.js";

import { TextLine } from "./text.js";

export interface SeparatorProps extends GuiKitLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id?: string;
  /**
   * `division` (neutral) divides a panel, `rule` (accent) underlines a
   * heading, and `quiet` (the quiet line) separates what repeats between data,
   * such as rows.
   */
  readonly tone?: "division" | "rule" | "quiet";
  readonly vertical?: boolean;
}

export function Separator(props: SeparatorProps) {
  const {
    id: providedId,
    layer = 0,
    tone = "division",
    vertical = false,
    layout,
  } = useGuiKitLayout(props);
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const thickness = kit.unit(kit.tokens.lineWidth);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <KitLayout
        kind={vertical ? LAYOUT_COLUMN : LAYOUT_ROW}
        {...(vertical ? { width: thickness } : { height: thickness })}
        {...layout}
      />
      <Skin theme={kit.theme(tone)} />
    </Entity>
  );
}

export interface LabelledSeparatorProps extends GuiKitLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id?: string;
  /** The section's label, in the accent at body size. */
  readonly label: string;
  /**
   * Start with a short line before the label, the content inset long, as a
   * division across a panel does; without it the label starts the row, as a
   * section heading at the content's edge does.
   */
  readonly stub?: boolean;
}

/**
 * A panel division that names the section below it: an optional short line,
 * the label, and a line filling the rest of the row, half an inset from the
 * label on either side. It is a dense row high.
 */
export function LabelledSeparator(props: LabelledSeparatorProps) {
  const {
    id: providedId,
    layer = 0,
    label,
    stub = true,
    layout,
  } = useGuiKitLayout(props);
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const t = kit.tokens;
  const gap = kit.unit(t.inset / 2);
  const line = { align_y: 0 };
  return (
    <Row
      gap={0}
      id={id}
      layer={layer}
      height={kit.unit(t.denseRow)}
      {...(layout ? { layout } : {})}
    >
      {stub && (
        <Separator
          id={`${id}/stub`}
          layout={{ ...line, width: kit.unit(t.inset), margin_right: gap }}
        />
      )}
      <TextLine id={`${id}/label`} text={label} tone="accent" />
      <Separator
        id={`${id}/line`}
        layout={{ ...line, flex: 1, margin_left: gap }}
      />
    </Row>
  );
}
