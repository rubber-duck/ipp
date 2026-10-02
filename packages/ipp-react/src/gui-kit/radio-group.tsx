/**
 * One choice from a labelled set of options. The group is one Tab stop,
 * entered at its selected option, or its first; arrows move focus and select,
 * Space selects the focused option, and arrows skip disabled options, whose
 * labels stay readable in the neutral colour. The runtime never clears the
 * last selection, so once an option is selected one always is.
 *
 * Each option's Button is its radio mark, a circle the icon size: a ring
 * round a dark interior, lit with a filled dot while selected, the focus ring
 * glowing on the same circle. A Button lights its whole box, so the label
 * beside the mark is a second Button inside the mark, pointer-only, which
 * selects the option through its handle: a click on the label selects it a
 * client round trip later and leaves focus where it was. Inside the mark,
 * the label is part of the option's item rather than an item of the group.
 * A child's box never exceeds the room its parent leaves it, so the label's
 * margins place it beside the mark while its outer box stays the mark's
 * width: otherwise a label longer than the mark would be squeezed onto it,
 * overlapping the circle and taking presses on its first letters only.
 *
 * The group is a column of the optional group label, in the accent, and the
 * options, stacked or in a row, each the small control height.
 */
import { Children, Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Behavior, Font, Group, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import {
  GROUP_HORIZONTAL,
  GROUP_VERTICAL,
  SELECT_FOLLOW,
  useChoice,
  useMountSelection,
  type Choice,
  type ChoiceProps,
} from "./choice.js";
import { useGuiKit, type GuiKitScope } from "./kit.js";
import {
  LAYOUT_COLUMN,
  LAYOUT_ROW,
  LAYOUT_STACK,
  Row,
  Strut,
  type GuiKitLayout,
} from "./layout.js";
import { TextLine, lineWidth } from "./text.js";

export interface RadioOption {
  /** The option's value, unique in its group and part of its entities' ids. */
  readonly value: string;
  readonly label: string;
  readonly disabled?: boolean;
}

export interface RadioGroupProps extends ChoiceProps {
  /** Symbolic id of the group's root entity; options extend it. */
  readonly id: string;
  /** The group's label above its options, such as AXIS. */
  readonly label?: string;
  readonly options: readonly RadioOption[];
  /** Options in a row rather than stacked. */
  readonly horizontal?: boolean;
  readonly layout?: GuiKitLayout;
}

/** The option geometry in the World's units. */
function geometry(kit: GuiKitScope) {
  const t = kit.tokens;
  return {
    mark: kit.unit(t.icon),
    gap: kit.unit(t.inset / 2),
    height: kit.unit(t.smallHeight),
    spacing: kit.unit(t.inset),
    caption: kit.unit(t.denseRow),
  };
}

export function RadioGroup({
  id,
  label,
  options,
  horizontal = false,
  layout,
  ...props
}: RadioGroupProps) {
  const kit = useGuiKit();
  const g = geometry(kit);
  const choice = useChoice(props);
  const body = kit.typeSize("body");
  const widths = options.map(
    (option) => g.mark + g.gap + lineWidth(option.label, body),
  );
  const optionsHeight = horizontal ? g.height : g.height * options.length;
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_COLUMN}
        height={(label === undefined ? 0 : g.caption) + optionsHeight}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        {label !== undefined && (
          <Row id={`${id}/caption`} height={kit.tokens.denseRow}>
            <TextLine id={`${id}/caption/text`} text={label} tone="accent" />
          </Row>
        )}
        <Entity id={`${id}/options`}>
          <Layout
            kind={horizontal ? LAYOUT_ROW : LAYOUT_COLUMN}
            height={optionsHeight}
          />
          <Group
            axis={horizontal ? GROUP_HORIZONTAL : GROUP_VERTICAL}
            selection={SELECT_FOLLOW}
          />
          <Children>
            {options.map((option, index) => (
              <Option
                key={option.value}
                id={`${id}/${option.value}`}
                option={option}
                choice={choice}
                width={widths[index]!}
                {...(horizontal && index > 0
                  ? { layout: { margin_left: g.spacing } }
                  : {})}
              />
            ))}
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}

function Option({
  id,
  option,
  choice,
  width,
  layout,
}: {
  readonly id: string;
  readonly option: RadioOption;
  readonly choice: Choice;
  readonly width: number;
  readonly layout?: GuiKitLayout;
}) {
  const kit = useGuiKit();
  const g = geometry(kit);
  const item = choice.item(option.value);
  const selected = useMountSelection(item.selected);
  const disabled = option.disabled ?? false;
  // The label's width: the option's beyond the mark and the gap.
  const label = width - g.mark - g.gap;
  return (
    <Entity id={id}>
      <Layout kind={LAYOUT_ROW} width={width} height={g.height} {...layout} />
      <Children>
        <Strut id={`${id}/strut`} height={g.height} />
        <Entity id={`${id}/mark`}>
          <Layout
            kind={LAYOUT_STACK}
            width={g.mark}
            height={g.mark}
            align_y={0}
          />
          <Skin theme={kit.theme("radio")} />
          <Behavior semantic_label={option.label} enabled={!disabled} />
          <Button
            label=""
            selected={selected}
            onSelectedChange={item.onSelectedChange}
            ref={item.ref}
          />
          <Children>
            <Entity id={`${id}/label`}>
              <Layout
                kind={LAYOUT_STACK}
                width={label}
                height={g.mark}
                margin_left={g.mark + g.gap}
                margin_right={-(g.gap + label)}
              />
              <Skin theme={kit.theme("radioLabel")} />
              <Behavior focusable={false} />
              <Button
                label={option.label}
                onPress={() => choice.select(option.value)}
              />
            </Entity>
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}
