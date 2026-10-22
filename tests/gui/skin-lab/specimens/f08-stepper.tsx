import { Entity, Children } from "@ipp/react";
import { Box, Layout, Style } from "@ipp/react/gui";
import {
  GuiKit,
  Stepper,
  TextLine,
  type GuiKitContract,
  type GuiSkinDefinition,
  type StepperStep,
} from "@ipp/react/gui-kit";
import { defineSpecimen } from "../specimen.js";

export const STEPPER_EXTENT = [760, 450] as const;
export interface StepperFixtureState {
  readonly reports: string[];
  reversed?: boolean;
}

function content(state: StepperFixtureState) {
  const steps: StepperStep[] = [
    { value: "details", label: "Details", completed: true },
    { value: "review", label: "Review" },
    { value: "verify", label: "Verify", disabled: true },
    { value: "done", label: "Done" },
  ].map((step) => ({
    ...step,
    content: (
      <TextLine
        id={`stepper-content-${step.value}`}
        text={`${step.label.toUpperCase()} / Application-owned content`}
      />
    ),
  }));
  if (state.reversed) steps.reverse();
  return (
    <>
      <Entity id="stepper-backdrop">
        <Layout width={760} height={450} align_x={-1} align_y={-1} />
        <Style red={0.006} green={0.01} blue={0.016} />
        <Box width={760} height={450} />
      </Entity>
      <Entity id="stepper-page">
        <Layout
          kind={2}
          width={760}
          height={450}
          padding_left={24}
          padding_right={24}
          padding_top={22}
          gap={18}
          align_x={-1}
          align_y={-1}
        />
        <Children>
          <TextLine
            id="stepper-title"
            text="SKINNABLE WIZARD STEPPER"
            tone="accent"
            size="display"
          />
          <TextLine
            id="stepper-intro"
            text="Completed, current, unavailable and upcoming steps"
            tone="neutral"
          />
          <Stepper
            id="wizard"
            steps={steps}
            defaultValue="review"
            onChange={(value) => {
              state.reports.push(value);
            }}
          />
          <Entity id="stepper-lower">
            <Layout kind={1} gap={28} height={230} />
            <Children>
              <Stepper
                id="vertical-wizard"
                width={200}
                orientation="vertical"
                defaultValue="review"
                steps={steps
                  .filter((step) => step.value !== "verify")
                  .map(({ content: _content, ...step }) => step)}
              />
              <Entity id="stepper-notes">
                <Layout kind={2} gap={12} flex={1} />
                <Children>
                  <TextLine text="NATIVE KEYBOARD NAVIGATION" tone="accent" />
                  <TextLine text="Arrows move focus" />
                  <TextLine text="Enter or Space selects a step" />
                  <TextLine text="Disabled steps are skipped" />
                  <TextLine
                    text="Completion stays application-owned"
                    tone="neutral"
                  />
                </Children>
              </Entity>
            </Children>
          </Entity>
        </Children>
      </Entity>
    </>
  );
}

export function stepperSpecimen(
  contract: GuiKitContract,
  skin: GuiSkinDefinition,
  state: StepperFixtureState,
) {
  return defineSpecimen({
    extent: STEPPER_EXTENT,
    states: [{ name: "stepper", cell: [0, 0, ...STEPPER_EXTENT] }],
    render: (lab) => (
      <GuiKit
        id="wizard-skin"
        contract={contract}
        skin={skin}
        font={lab.font}
        fontSize={14}
        reducedMotion
      >
        {content(state)}
      </GuiKit>
    ),
  });
}

export default defineSpecimen({
  extent: STEPPER_EXTENT,
  states: [{ name: "stepper", cell: [0, 0, ...STEPPER_EXTENT] }],
  render: () => <GuiKit fontSize={14}>{content({ reports: [] })}</GuiKit>,
});
