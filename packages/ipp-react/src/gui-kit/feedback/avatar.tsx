import { Children, Entity, PresentationGate } from "../../components.js";
import type { AssetReference } from "../../assets/declarations.js";
import { Behavior, Image, Layout } from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { useReadyBitmap } from "../bitmap.js";
import { useEntityId } from "../../scope/identity.js";
import { useGuiKit } from "../kit.js";
import { TextLine } from "../text.js";

export interface AvatarProps {
  readonly id?: string;
  readonly source?: AssetReference;
  readonly name?: string;
  readonly decorative?: boolean;
  readonly fallback?: string;
  readonly size?: number;
}

export function Avatar({
  id: providedId,
  source,
  name = "",
  decorative = false,
  fallback,
  size,
}: AvatarProps) {
  const id = useEntityId(providedId),
    kit = useGuiKit(),
    bitmap = useReadyBitmap(source);
  const side = size ?? kit.unit(kit.tokens.controlHeight);
  if (!Number.isFinite(side) || side <= 0)
    throw new RangeError("Invalid avatar size");
  const initials = name
    .trim()
    .split(/\s+/u)
    .filter(Boolean)
    .slice(0, 2)
    .map((word) => Array.from(word)[0])
    .join("")
    .toLocaleUpperCase();
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Layout kind={3} width={side} height={side} clip />
      <Skin theme={kit.theme("frame")} />
      {!decorative && <Behavior semantic_label={name} focusable={false} />}
      <Children>
        {bitmap ? (
          <Entity id={`${id}/image`}>
            <Layout width={side} height={side} />
            <Image
              source={bitmap.source}
              variant={bitmap.variant ?? 0}
              width={side}
              height={side}
            />
          </Entity>
        ) : (
          <TextLine
            id={`${id}/fallback`}
            text={fallback ?? (initials || "?")}
            layout={{ align_x: 0, align_y: 0, fit_width: true }}
          />
        )}
      </Children>
    </Entity>
  );
}
