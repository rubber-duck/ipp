/** Flat specimens of production wrappers; the environment supplies GlassSkin. */
import { SpecimenCaption } from "./specimens/caption.js";
import { Entity } from "../../../packages/ipp-react/src/components.js";
import {
  Behavior,
  Font,
  Layout,
} from "../../../packages/ipp-react/src/gui/components.js";
import type { GuiControlRef } from "../../../packages/ipp-react/src/gui/control-ref.js";
import {
  GlassButton,
  GlassTextInput,
} from "../../../packages/ipp-react/src/gui-glass/actions.js";
import { useGlassSkin } from "../../../packages/ipp-react/src/gui-glass/skin.js";
import type { ActionRect } from "./actions-oracle.js";
import type { PinStep, SpecimenState } from "../skin-lab/specimen.js";
import {
  GLASS_BUTTON_CASES,
  GLASS_TEXT_INPUT_CASES,
} from "./fixtures/action-cases.js";

export const ACTION_SPECIMEN_EXTENT = [1020, 420] as const;

/** Logical bounds shared with input drivers and scaled by the capture driver. */
export function actionSpecimenBounds(
  family: "button" | "textInput",
  index: number,
): ActionRect {
  return [
    12 + (index % 5) * 204,
    (family === "button" ? 24 : 224) + Math.floor(index / 5) * 80,
    180,
    40,
  ];
}

function pins(
  family: "button" | "textInput",
  name: string,
  index: number,
): readonly PinStep[] {
  const [x, y, width, height] = actionSpecimenBounds(family, index);
  const at = [x + width / 2, y + height / 2] as const;
  if (name.endsWith("hover")) return [{ kind: "hover", at }];
  if (name.endsWith("pressed")) return [{ kind: "press", at }];
  if (name.endsWith("focus"))
    return [
      {
        kind: "action",
        control: `${family === "button" ? "button" : "input"}-${name}`,
        action: { kind: "focus" },
      },
    ];
  if (family === "textInput" && name === "selection")
    return [
      { kind: "click", at },
      { kind: "selectText", start: 0, end: 5 },
      { kind: "hover", at: [x, y + height + 24] },
    ];
  return [];
}

/** Existing SpecimenSession pins these states through actual native ingress. */
export const actionStates: readonly SpecimenState[] = (
  [
    ["button", GLASS_BUTTON_CASES],
    ["textInput", GLASS_TEXT_INPUT_CASES],
  ] as const
).flatMap(([family, cases]) =>
  cases.map(({ name }, index) => {
    const [x, y, width, height] = actionSpecimenBounds(family, index);
    return {
      name: `${family === "button" ? "button" : "input"}-${name}`,
      cell: [x - 12, y - 12, width + 24, height + 36] as const,
      pin: pins(family, name, index),
    };
  }),
);

export interface ActionsSpecimenProps {
  readonly control: (name: string) => GuiControlRef;
}

/** Every state uses a real native control; state names guide input pinning. */
export function ActionsSpecimen({ control }: ActionsSpecimenProps) {
  const glass = useGlassSkin();
  return (
    <>
      {GLASS_BUTTON_CASES.map((item, index) => {
        const [x, y, width, height] = actionSpecimenBounds("button", index);
        const { name, ...props } = item;
        return (
          <Entity key={name} id={`glass-button-${name}`}>
            <Layout
              kind={0}
              width={width}
              height={height}
              margin_left={x}
              margin_top={y}
              align_x={-1}
              align_y={-1}
            />
            <Font source={glass.font} font_size={glass.fontSize} />
            {"disabled" in props && props.disabled && (
              <Behavior enabled={false} />
            )}
            <GlassButton
              label={props.label}
              selected={"selected" in props && props.selected}
              ref={control(`button-${name}`)}
            />
          </Entity>
        );
      })}
      {GLASS_TEXT_INPUT_CASES.map((item, index) => {
        const [x, y, width, height] = actionSpecimenBounds("textInput", index);
        const { name, ...props } = item;
        const { disabled, ...fields } = {
          disabled: false,
          ...props,
        };
        return (
          <Entity key={name} id={`glass-input-${name}`}>
            <Layout
              kind={0}
              width={width}
              height={height}
              margin_left={x}
              margin_top={y}
              align_x={-1}
              align_y={-1}
            />
            <Font source={glass.font} font_size={glass.fontSize} />
            {disabled && <Behavior enabled={false} />}
            <GlassTextInput {...fields} ref={control(`input-${name}`)} />
          </Entity>
        );
      })}
      {(
        [
          ["button", GLASS_BUTTON_CASES],
          ["textInput", GLASS_TEXT_INPUT_CASES],
        ] as const
      ).flatMap(([family, cases]) =>
        cases.map(({ name }, index) => {
          const [x, y, width, height] = actionSpecimenBounds(family, index);
          return (
            <SpecimenCaption
              key={`${family}-${name}`}
              id={`${family}-${name}`}
              text={name}
              x={x}
              y={y + height + 6}
              width={width}
            />
          );
        }),
      )}
    </>
  );
}
