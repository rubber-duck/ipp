/**
 * A small set of mutually exclusive modes as adjacent segments, with the
 * radio group's keys: one Tab stop entered at the selected segment, arrows
 * move focus and select, Space selects, disabled segments are skipped. Use
 * `Tabs` when the choice switches between content panels.
 *
 * The control is one frame, the control frame at rest with its paired cut,
 * whose segments share its width. Segments are clear at rest, separated by
 * quiet lines; the selected one fills with the accent under a dark label and
 * keeps the fill while it holds focus, which glows on its own contour. Only
 * the first segment's top-left corner and the last one's bottom-right are
 * cut, so the whole reads as one control in the language's one cut pattern.
 * The control is the control height and fills its container's width.
 */
import { Children, Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Behavior, Font, Group, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import {
  GROUP_HORIZONTAL,
  SELECT_FOLLOW,
  useChoice,
  useMountSelection,
  type Choice,
  type ChoiceProps,
} from "./choice.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_LEAF, LAYOUT_ROW, type GuiKitLayout } from "./layout.js";
import type { KitThemeName } from "./themes.js";

export interface SegmentedOption {
  /** The segment's value, unique in its control and part of its id. */
  readonly value: string;
  readonly label: string;
  readonly disabled?: boolean;
}

export interface SegmentedControlProps extends ChoiceProps {
  /** Symbolic id of the control's frame; segments extend it. */
  readonly id: string;
  readonly options: readonly SegmentedOption[];
  readonly layout?: GuiKitLayout;
}

/** The look of the segment at `index` of `count`: cut where the frame is. */
function segmentTheme(index: number, count: number): KitThemeName {
  if (count === 1) return "segmentOnly";
  if (index === 0) return "segmentFirst";
  return index === count - 1 ? "segmentLast" : "segment";
}

export function SegmentedControl({
  id,
  options,
  layout,
  ...props
}: SegmentedControlProps) {
  const kit = useGuiKit();
  const choice = useChoice(props);
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_ROW}
        height={kit.unit(kit.tokens.controlHeight)}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Skin theme={kit.theme("frame")} />
      <Group axis={GROUP_HORIZONTAL} selection={SELECT_FOLLOW} />
      <Children>
        {options.map((option, index) => (
          <Segment
            key={option.value}
            id={`${id}/${option.value}`}
            option={option}
            choice={choice}
            theme={segmentTheme(index, options.length)}
          />
        ))}
      </Children>
    </Entity>
  );
}

function Segment({
  id,
  option,
  choice,
  theme,
}: {
  readonly id: string;
  readonly option: SegmentedOption;
  readonly choice: Choice;
  readonly theme: KitThemeName;
}) {
  const kit = useGuiKit();
  const item = choice.item(option.value);
  const selected = useMountSelection(item.selected);
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_LEAF}
        flex={1}
        height={kit.unit(kit.tokens.controlHeight)}
      />
      <Skin theme={kit.theme(theme)} />
      <Behavior enabled={!option.disabled} />
      <Button
        label={option.label}
        selected={selected}
        onSelectedChange={item.onSelectedChange}
        ref={item.ref}
      />
    </Entity>
  );
}
