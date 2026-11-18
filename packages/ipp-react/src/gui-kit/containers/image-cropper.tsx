import { useRef } from "react";
import { Children, Entity } from "../../components.js";
import { useReadyBitmap } from "../bitmap.js";
import type { ClientAssetSource } from "@ipp/client";
import { useEntityId } from "../../scope/identity.js";
import { Image, Layout, Style } from "../../gui/components.js";
import type { AssetReference } from "../../assets/declarations.js";
import { Skin } from "../../gui/theme.js";
import { useGuiKit } from "../kit.js";
import { Column, Row, type ContainerProps } from "../layout.js";
import { TextLine } from "../text.js";
import { Button } from "../button.js";
import { Grip } from "./grip.js";
import { useApplicationValue } from "../selection/application-value.js";

export interface CropRect {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}
export interface ImageCropperProps extends Omit<ContainerProps, "children"> {
  readonly source: AssetReference;
  readonly imageWidth: number;
  readonly imageHeight: number;
  readonly previewWidth: number;
  readonly previewHeight: number;
  readonly minWidth?: number;
  readonly minHeight?: number;
  readonly aspect?: number;
  readonly crop?: CropRect;
  readonly defaultCrop?: CropRect;
  readonly onCropChange?: (crop: CropRect) => void;
}

export function constrainCrop(
  rect: CropRect,
  imageWidth: number,
  imageHeight: number,
  minWidth = 1,
  minHeight = 1,
  aspect?: number,
): CropRect {
  if (
    ![
      rect.x,
      rect.y,
      rect.width,
      rect.height,
      imageWidth,
      imageHeight,
      minWidth,
      minHeight,
    ].every(Number.isFinite) ||
    imageWidth < 1 ||
    imageHeight < 1 ||
    minWidth <= 0 ||
    minHeight <= 0 ||
    minWidth > imageWidth ||
    minHeight > imageHeight ||
    (aspect !== undefined && (!Number.isFinite(aspect) || aspect <= 0))
  )
    throw new RangeError("Invalid source or crop bounds");
  let width = Math.min(imageWidth, Math.max(minWidth, rect.width));
  let height = Math.min(imageHeight, Math.max(minHeight, rect.height));
  if (aspect !== undefined) {
    const least = Math.max(minWidth, minHeight * aspect),
      most = Math.min(imageWidth, imageHeight * aspect);
    if (least > most)
      throw new RangeError("Crop aspect cannot satisfy the source bounds");
    width = Math.min(most, Math.max(least, width));
    height = width / aspect;
  }
  return {
    x: Math.max(0, Math.min(imageWidth - width, rect.x)),
    y: Math.max(0, Math.min(imageHeight - height, rect.y)),
    width,
    height,
  };
}

/** Source rectangles are in original bitmap pixels. The application encodes or exports them. */
export function ImageCropper(props: ImageCropperProps) {
  const { source, ...view } = props;
  const bitmap = useReadyBitmap(source);
  const id = useEntityId(props.id);
  return (
    <CropView
      key={`${props.source.assetId}:${bitmap?.source ?? ""}:${props.imageWidth}:${props.imageHeight}`}
      {...view}
      id={id}
      bitmap={bitmap}
    />
  );
}

function CropView({
  id,
  bitmap,
  imageWidth,
  imageHeight,
  previewWidth,
  previewHeight,
  minWidth = 1,
  minHeight = 1,
  aspect,
  crop,
  defaultCrop,
  onCropChange,
  ...layout
}: Omit<ImageCropperProps, "source"> & {
  id: string;
  bitmap: Readonly<ClientAssetSource> | undefined;
}) {
  const kit = useGuiKit();
  if (![previewWidth, previewHeight].every((n) => Number.isFinite(n) && n > 0))
    throw new RangeError("Preview size must be positive");
  const bound = (r: CropRect) =>
    constrainCrop(r, imageWidth, imageHeight, minWidth, minHeight, aspect);
  const initial = bound(
    defaultCrop ?? { x: 0, y: 0, width: imageWidth, height: imageHeight },
  );
  const state = useApplicationValue(crop, initial, onCropChange),
    shown = bound(state.current),
    scale = Math.min(previewWidth / imageWidth, previewHeight / imageHeight);
  const x = (previewWidth - imageWidth * scale) / 2,
    y = (previewHeight - imageHeight * scale) / 2;
  const held = useRef<{
    pointer: bigint;
    point: readonly [number, number];
    rect: CropRect;
    resize: boolean;
  } | null>(null);
  function change(next: CropRect) {
    const c = bound(next);
    state.change((old) =>
      c.x === old.x &&
      c.y === old.y &&
      c.width === old.width &&
      c.height === old.height
        ? old
        : c,
    );
  }
  function resized(rect: CropRect, dx: number, dy: number): CropRect {
    const horizontal =
      aspect && Math.abs(dy * aspect) > Math.abs(dx) ? dy * aspect : dx;
    return {
      ...rect,
      width: rect.width + horizontal,
      height: rect.height + dy,
    };
  }
  function zoom(factor: number) {
    const width = shown.width * factor,
      height = shown.height * factor;
    const sized = bound({ ...shown, width, height });
    change({
      ...sized,
      x: shown.x + (shown.width - sized.width) / 2,
      y: shown.y + (shown.height - sized.height) / 2,
    });
  }
  return (
    <Column id={id} {...layout}>
      <Grip
        id={`${id}/preview`}
        label="Crop image"
        layout={{ width: previewWidth, height: previewHeight, clip: true }}
        disabled={!bitmap}
        onBlur={() => {
          held.current = null;
        }}
        onInput={(e) => {
          if (!bitmap) return;
          if (e.kind === "key" && e.pressed) {
            const dx =
                e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0,
              dy = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
            if (dx || dy)
              change(
                e.modifiers & 1
                  ? resized(shown, dx, dy)
                  : { ...shown, x: shown.x + dx, y: shown.y + dy },
              );
          }
          if (e.kind !== "pointer") return;
          const [px, py] = [(e.point[0] - x) / scale, (e.point[1] - y) / scale];
          if (
            e.phase === "down" &&
            e.button === "primary" &&
            px >= shown.x &&
            px <= shown.x + shown.width &&
            py >= shown.y &&
            py <= shown.y + shown.height
          )
            held.current = {
              pointer: e.pointer,
              point: [px, py],
              rect: shown,
              resize:
                Math.abs(px - shown.x - shown.width) * scale < 12 &&
                Math.abs(py - shown.y - shown.height) * scale < 12,
            };
          const g = held.current;
          if (!g || g.pointer !== e.pointer) return;
          if (e.phase === "cancel") {
            held.current = null;
            return;
          }
          if (e.phase === "move" || e.phase === "up") {
            const dx = px - g.point[0],
              dy = py - g.point[1];
            change(
              g.resize
                ? resized(g.rect, dx, dy)
                : { ...g.rect, x: g.rect.x + dx, y: g.rect.y + dy },
            );
            if (e.phase === "up") held.current = null;
          }
        }}
      >
        {bitmap ? (
          <>
            <Entity id={`${id}/image`}>
              <Layout
                width={imageWidth * scale}
                height={imageHeight * scale}
                align_x={-1}
                align_y={-1}
              />
              <Style x={x} y={y} />
              <Image
                source={bitmap.source}
                variant={bitmap.variant ?? 0}
                width={imageWidth * scale}
                height={imageHeight * scale}
              />
            </Entity>
            <Entity id={`${id}/selection`}>
              <Style x={x + shown.x * scale} y={y + shown.y * scale} />
              <Layout
                kind={3}
                align_x={-1}
                align_y={-1}
                width={shown.width * scale}
                height={shown.height * scale}
              />
              <Children>
                {[
                  [0, 0, shown.width * scale, 2],
                  [0, 0, 2, shown.height * scale],
                  [0, shown.height * scale - 2, shown.width * scale, 2],
                  [shown.width * scale - 2, 0, 2, shown.height * scale],
                ].map(([left, top, w, h], i) => (
                  <Entity key={i} id={`${id}/edge/${i}`}>
                    <Layout
                      width={Math.max(1, w!)}
                      height={Math.max(1, h!)}
                      align_x={-1}
                      align_y={-1}
                    />
                    <Style x={left!} y={top!} />
                    <Skin theme={kit.theme("valueAccent")} />
                  </Entity>
                ))}
              </Children>
            </Entity>
          </>
        ) : (
          <TextLine text="Image unavailable" />
        )}
      </Grip>
      <Row>
        <Button
          id={`${id}/zoom-in`}
          label="Zoom in"
          disabled={!bitmap}
          onPress={() => zoom(0.8)}
        />
        <Button
          id={`${id}/zoom-out`}
          label="Zoom out"
          disabled={!bitmap}
          onPress={() => zoom(1.25)}
        />
        <Button
          id={`${id}/reset`}
          label="Reset"
          disabled={!bitmap}
          onPress={() => change(initial)}
        />
      </Row>
    </Column>
  );
}
