/**
 * A collapsible section: a full-width header button showing a chevron, the
 * section's label and an optional summary, and the section's content while
 * expanded. Pressing the header, or Enter or Space while it has focus, toggles
 * it. Hidden entities keep their layout space, so collapsing removes the
 * content's declarations, and any state held only in them, rather than
 * hiding them.
 *
 * The content follows the header as its siblings: put the expander in a
 * column's `Children`, and its content's Entity declarations as its children.
 * The expansion is the application's: pass `expanded` with
 * `onExpandedChange`, or only `defaultExpanded` to let the expander keep it.
 */
import { useState, type ReactNode } from "react";
import { Children, Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Behavior, Font, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import type { GuiControlRef } from "../gui/control-ref.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_ROW, Strut, type GuiKitLayout } from "./layout.js";
import { Icon, TextLine } from "./text.js";

export interface ExpanderProps {
  /** Symbolic id of the header entity. */
  readonly id: string;
  readonly label: string;
  /** Secondary text at the header's end, such as how many options it holds. */
  readonly summary?: string;
  /** Controlled expansion; pair it with `onExpandedChange`. */
  readonly expanded?: boolean;
  /** Initial expansion while `expanded` is omitted. */
  readonly defaultExpanded?: boolean;
  readonly onExpandedChange?: (expanded: boolean) => void;
  readonly disabled?: boolean;
  /** The header's control handle. */
  readonly ref?: GuiControlRef;
  /** Layout of the header. */
  readonly layout?: GuiKitLayout;
  /** The content's Entity declarations, declared only while expanded. */
  readonly children?: ReactNode;
}

export function Expander({
  id,
  label,
  summary,
  expanded,
  defaultExpanded = false,
  onExpandedChange,
  disabled = false,
  ref,
  layout,
  children,
}: ExpanderProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const [own, setOwn] = useState(defaultExpanded);
  const open = expanded ?? own;
  const toggle = () => {
    if (expanded === undefined) setOwn(!open);
    onExpandedChange?.(!open);
  };
  return (
    <>
      <Entity id={id}>
        <Layout
          kind={LAYOUT_ROW}
          height={kit.unit(t.controlHeight)}
          padding_left={kit.unit(t.inset)}
          padding_right={kit.unit(t.inset)}
          {...layout}
        />
        <Font source={kit.font} font_size={kit.fontSize} />
        <Skin theme={kit.theme("expanderHeader")} />
        <Behavior semantic_label={label} enabled={!disabled} />
        <Button label="" onPress={toggle} {...(ref ? { ref } : {})} />
        <Children>
          <Strut id={`${id}/strut`} height={kit.unit(t.controlHeight)} />
          <Icon
            id={`${id}/chevron`}
            icon={open ? "expanded" : "collapsed"}
            tone={disabled ? "neutral" : "accent"}
          />
          <TextLine
            id={`${id}/label`}
            text={label}
            tone={disabled ? "neutral" : "accent"}
            layout={{ flex: 1, margin_left: kit.unit(t.inset) }}
          />
          {summary !== undefined && (
            <TextLine id={`${id}/summary`} text={summary} tone="neutral" />
          )}
        </Children>
      </Entity>
      {open && children}
    </>
  );
}
