/**
 * A popover: a trigger, a secondary button whose caret says it opens
 * something, and the anchored light overlay it opens centred below itself,
 * a quarter inset away, a floating surface whose header strip holds the
 * title, in the accent, and an optional close button, its only control,
 * over a division and the application's content at the content inset. The content is ordinary
 * declarations, such as a text field, a checkbox and Apply and Cancel
 * buttons that commit or discard the application's draft; give them explicit
 * heights, since the popover fits its content's height.
 *
 * The trigger takes its open look, the checked variant, while the popover is
 * open, and toggles it. Opening moves focus to the content's first control,
 * with the ring when the trigger was opened from the keyboard; Escape, an
 * outside press, which is swallowed, or focus leaving closes it and returns
 * focus to the trigger. The close button takes no focus, so focus enters at
 * the content; Escape is its key. The popover flips above the trigger, or
 * shifts, to stay inside the canvas.
 */
import type { ReactNode } from "react";
import { Children, Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Style, Behavior, Font, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { GUI_KIT_ICONS } from "./icons.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_COLUMN, LAYOUT_LEAF, type GuiKitLayout } from "./layout.js";
import { Floating, useOverlayOpen, type OverlayOpenProps } from "./overlay.js";
import { PanelHeader } from "./panel.js";
import { textWidth } from "./text.js";
import { WindowControl } from "./window-controls.js";

/**
 * A popover's width by default, at the tokens' `em`: a field or two equal
 * buttons a content inset apart, at the content inset.
 */
const WIDTH = 240;

export interface PopoverProps extends OverlayOpenProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the trigger; the popover and its parts extend it. */
  readonly id: string;
  /** The trigger's label; a caret follows it. */
  readonly label: string;
  /** The popover's title. */
  readonly title: string;
  /** Show the close button; true by default. */
  readonly closable?: boolean;
  /** The popover's width at the tokens' `em`; 240 by default. */
  readonly width?: number;
  /** Layout of the trigger. */
  readonly layout?: GuiKitLayout;
  /** The content: entity declarations in a column at the content inset. */
  readonly children?: ReactNode;
}

export function Popover({
  id,
  layer = 0,
  label,
  title,
  closable = true,
  width = WIDTH,
  layout,
  children,
  ...open
}: PopoverProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const overlay = useOverlayOpen(open);
  const size = kit.typeSize("small");
  const text = `${label} ${GUI_KIT_ICONS.sortDescending}`;
  const inset = kit.unit(t.inset);
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_LEAF}
        width={textWidth(text, size) + 2 * inset}
        height={kit.unit(t.smallHeight)}
        align_y={0}
        {...layout}
      />
      <Font source={kit.font} font_size={size} />
      <Skin theme={kit.theme("secondarySmall")} />
      <Behavior semantic_label={label} />
      <Button label={text} selected={overlay.open} onPress={overlay.toggle} />
      <Children>
        <Floating
          id={`${id}/popover`}
          mode="light"
          align="centre"
          open={overlay.open}
          onVisibleChange={overlay.onVisibleChange}
          offset={[0, kit.unit(t.inset / 4)]}
          layout={{ width: kit.unit(width) }}
        >
          <PanelHeader id={`${id}/header`} title={title}>
            {closable && (
              <WindowControl
                id={`${id}/close`}
                kind="close"
                focusable={false}
                onPress={() => overlay.setOpen(false)}
              />
            )}
          </PanelHeader>
          <Entity id={`${id}/content`}>
            <Layout
              kind={LAYOUT_COLUMN}
              padding_left={inset}
              padding_right={inset}
              padding_top={inset}
              padding_bottom={inset}
            />
            {children !== undefined && <Children>{children}</Children>}
          </Entity>
        </Floating>
      </Children>
    </Entity>
  );
}
