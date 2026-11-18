import type { ReactNode } from "react";
import type { GuiClientInput } from "@ipp/client";
import { Children, Entity, PresentationGate } from "../../components.js";
import { Behavior, InputTarget } from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { useTargetInput } from "../../gui/target-input.js";
import { useGuiKit } from "../kit.js";
import { KitLayout, type GuiKitLayout } from "../layout.js";

export function Grip({
  id,
  label,
  disabled = false,
  layout,
  children,
  onInput,
  onBlur,
  onInteractionChange,
}: {
  id: string;
  label: string;
  disabled?: boolean;
  layout: GuiKitLayout;
  children?: ReactNode;
  onInput: (input: GuiClientInput) => void;
  onBlur?: () => void;
  onInteractionChange?: import("../../gui/callbacks.js").GuiInteractionListener;
}) {
  const kit = useGuiKit(),
    ref = useTargetInput(onInput, onBlur);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <KitLayout kind={3} {...layout} />
      <Skin theme={kit.theme("secondary")} />
      <Behavior enabled={!disabled} semantic_label={label} />
      <InputTarget ref={ref} onInteractionChange={onInteractionChange} />
      <Children>{children}</Children>
    </Entity>
  );
}
