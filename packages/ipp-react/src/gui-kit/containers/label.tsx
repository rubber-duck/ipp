import { useState, type ReactNode } from "react";
import { Children, Entity, PresentationGate } from "../../components.js";
import { Behavior, Layout, Style } from "../../gui/components.js";
import { Button } from "../../gui/controls.js";
import type { GuiControlHandle } from "../../gui/control-ref.js";
import { useWorld } from "../../scope/world.js";
import { useEntityId } from "../../scope/identity.js";
import { TextLine } from "../text.js";
import {
  LAYOUT_ROW,
  useLayoutFields,
  type GuiKitLayoutProps,
} from "../layout.js";

/** Pair the acknowledged control ref with a Label or Field. */
export function useLabelTarget() {
  const [target, ref] = useState<GuiControlHandle | null>(null);
  return { target, ref };
}

export interface LabelProps extends GuiKitLayoutProps {
  readonly id?: string;
  readonly text: string;
  readonly children?: ReactNode;
  readonly target: GuiControlHandle | null;
  readonly activation?: "focus" | "press" | "toggle";
  readonly disabled?: boolean;
  readonly description?: string | undefined;
  readonly error?: string | undefined;
  readonly required?: boolean;
  readonly invalid?: boolean;
}

export function Label({
  id: supplied,
  text,
  target,
  activation = "focus",
  disabled = false,
  children,
  description,
  error,
  required = false,
  invalid = !!error,
  ...layout
}: LabelProps) {
  const id = useEntityId(supplied);
  const fields = useLayoutFields(layout, { fit_height: true });
  const { world } = useWorld();
  const t = target?.target;
  if (
    t &&
    world &&
    (t.world.id !== world.id || t.world.incarnation !== world.incarnation)
  )
    throw new Error("Label and control must belong to the same World");
  const address = t
    ? [
        t.world.id,
        t.world.incarnation,
        t.entity,
        t.component,
        t.incarnation,
      ].join("/")
    : "";
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Layout kind={LAYOUT_ROW} {...fields} />
      <Children>
        <Entity key={address} id={`${id}/association/${address || "pending"}`}>
          <PresentationGate open />
          <Style />
          <Layout kind={LAYOUT_ROW} fit_height fit_width />
          <Behavior
            enabled={!!text && !!target && !disabled}
            focusable={false}
            semantic_label={`${activation === "focus" ? "Focus" : "Activate"} ${text}`}
            label_for={address}
            semantic_description={description ?? ""}
            semantic_error={error ?? ""}
            semantic_required={required}
            semantic_invalid={invalid}
          />
          <Button
            label={text}
            skin={false}
            onPress={() => {
              if (!target || disabled) return;
              // A retired target's handle refuses locally, even while the label's
              // effect is travelling. Never resolve a replacement by symbolic id.
              void target
                .action(
                  activation === "focus"
                    ? { kind: "focus", part: 0 }
                    : { kind: activation },
                )
                .catch(() => {});
            }}
          />
          <Children>
            <TextLine
              id={`${id}/caption`}
              text={text}
              tone={disabled ? "neutral" : "accent"}
            />
            {children}
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}
