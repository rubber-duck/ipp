/**
 * A wizard's step navigation. Native groups own focus, pointer feedback and
 * selection; arrows move focus without advancing, Enter/Space selects, and
 * disabled steps are skipped. Applications own completion and availability:
 * navigating backward never marks another step incomplete or validates a form.
 * Number buttons have their labels beside them, with clear space at each end
 * of the connectors. Optional content mounts only for the selected step.
 */
import { Fragment, type ReactNode } from "react";
import { Children, Entity, PresentationGate } from "../../components.js";
import { Behavior, Font, Group, Layout, Style } from "../../gui/components.js";
import { Button } from "../../gui/controls.js";
import { Skin } from "../../gui/theme.js";
import { useEntityId } from "../../scope/identity.js";
import {
  GROUP_HORIZONTAL,
  GROUP_VERTICAL,
  SELECT_SINGLE,
  useChoice,
  useMountSelection,
  type Choice,
  type ChoiceProps,
} from "../choice.js";
import { useGuiKit } from "../kit.js";
import {
  KitLayout,
  LAYOUT_COLUMN,
  LAYOUT_LEAF,
  LAYOUT_ROW,
  LAYOUT_STACK,
  useGuiKitLayout,
  type GuiKitLayoutProps,
} from "../layout.js";
import { TextLine } from "../text.js";

export interface StepperStep {
  /** Stable application key, unique within this stepper. */
  readonly value: string;
  readonly label: string;
  /** Completion is independent of selection; omitted means incomplete. */
  readonly completed?: boolean;
  /** Prevent navigation to this step without hiding its progress. */
  readonly disabled?: boolean;
  /** Mounted only while this step is selected. Omit for navigation alone. */
  readonly content?: ReactNode;
}

export interface StepperProps extends ChoiceProps, GuiKitLayoutProps {
  readonly id?: string;
  /** Relative layer of the complete control, including its content. */
  readonly layer?: number;
  readonly steps: readonly StepperStep[];
  readonly orientation?: "horizontal" | "vertical";
}

/** Controlled with value/onChange, or initially selects the first enabled step. */
export function Stepper(inputProps: StepperProps) {
  const {
    id: providedId,
    layer = 0,
    steps,
    orientation = "horizontal",
    layout,
    defaultValue,
    ...props
  } = useGuiKitLayout(inputProps);
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const initial = defaultValue ?? steps.find((step) => !step.disabled)?.value;
  const choice = useChoice({
    ...props,
    ...(initial === undefined ? {} : { defaultValue: initial }),
  });
  const vertical = orientation === "vertical";
  const height = kit.unit(kit.tokens.controlHeight);
  const gap = kit.spacing.label;
  const connector = kit.spacing.field;
  const line = kit.unit(kit.tokens.lineWidth);
  const content = steps.find((step) => step.value === choice.current)?.content;
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <KitLayout kind={LAYOUT_COLUMN} fit_height gap={connector} {...layout} />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Children>
        <Entity id={`${id}/steps`}>
          <PresentationGate open />
          <Layout
            kind={vertical ? LAYOUT_COLUMN : LAYOUT_ROW}
            height={
              vertical
                ? steps.length * height +
                  Math.max(0, steps.length - 1) * (connector + 2 * gap)
                : steps.length
                  ? height
                  : 0
            }
          />
          <Group
            axis={vertical ? GROUP_VERTICAL : GROUP_HORIZONTAL}
            selection={SELECT_SINGLE}
          />
          <Children>
            {steps.map((step, index) => (
              <Fragment key={step.value}>
                <Step
                  id={`${id}/step/${step.value}`}
                  step={step}
                  index={index}
                  count={steps.length}
                  choice={choice}
                />
                {index + 1 < steps.length && (
                  <Entity id={`${id}/connector/${step.value}`}>
                    <PresentationGate open />
                    <Layout
                      kind={LAYOUT_LEAF}
                      width={vertical ? line : -1}
                      min_width={vertical ? 0 : connector}
                      height={vertical ? connector : line}
                      flex={vertical ? 0 : 1}
                      align_y={-1}
                      margin_left={vertical ? (height - line) / 2 : gap}
                      margin_right={vertical ? 0 : gap}
                      margin_top={vertical ? gap : (height - line) / 2}
                      margin_bottom={vertical ? gap : 0}
                    />
                    <Skin
                      theme={kit.theme(
                        step.completed
                          ? "stepperConnectorComplete"
                          : "stepperConnector",
                      )}
                    />
                  </Entity>
                )}
              </Fragment>
            ))}
          </Children>
        </Entity>
        {content !== undefined && (
          <Entity id={`${id}/content`}>
            <PresentationGate open />
            <Layout kind={LAYOUT_COLUMN} fit_height />
            <Children>
              <Fragment key={choice.current}>{content}</Fragment>
            </Children>
          </Entity>
        )}
      </Children>
    </Entity>
  );
}

function Step({
  id,
  step,
  index,
  count,
  choice,
}: {
  readonly id: string;
  readonly step: StepperStep;
  readonly index: number;
  readonly count: number;
  readonly choice: Choice;
}) {
  const kit = useGuiKit();
  const item = choice.item(step.value);
  const selected = useMountSelection(item.selected);
  const height = kit.unit(kit.tokens.controlHeight);
  const status = item.selected
    ? "Current step"
    : step.completed
      ? "Completed"
      : step.disabled
        ? "Unavailable"
        : "Upcoming";
  return (
    <Entity id={`${id}/item`}>
      <PresentationGate open />
      <Layout
        kind={LAYOUT_ROW}
        fit_width
        gap={kit.spacing.label}
        height={height}
        flex={0}
      />
      <Children>
        <Entity id={id}>
          <PresentationGate open />
          <Layout kind={LAYOUT_STACK} width={height} height={height} />
          <Behavior
            enabled={!step.disabled}
            semantic_label={`${index + 1} of ${count}: ${step.label}, ${status}`}
          />
          <Skin
            theme={kit.theme(
              step.completed ? "stepperComplete" : "stepperStep",
            )}
          />
          <Button
            label={String(index + 1)}
            selected={selected}
            ref={item.ref}
            onSelectedChange={item.onSelectedChange}
          />
        </Entity>
        <Entity id={`${id}/labels`}>
          <PresentationGate open />
          <Layout kind={LAYOUT_COLUMN} fit_width height={height} />
          <Children>
            <TextLine
              id={`${id}/label`}
              text={step.label}
              tone={step.disabled ? "neutral" : "text"}
              height={height / 2}
            />
            <TextLine
              id={`${id}/status`}
              text={status}
              size="small"
              tone={
                step.disabled
                  ? "neutral"
                  : item.selected || step.completed
                    ? "accent"
                    : "neutral"
              }
              height={height / 2}
            />
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}
