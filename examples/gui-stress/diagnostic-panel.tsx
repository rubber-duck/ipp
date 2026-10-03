/** Fixed entity-count panels supplement the interactive stress scene. */
import {
  CanvasWorld,
  Children,
  Entity,
  Surface,
  type CanvasWorldHandle,
} from "@ipp/react";
import { Box, Layout, Style } from "@ipp/react/gui";

export function GuiDiagnosticPanel({
  count,
  revision,
  position,
  onReady,
}: {
  readonly count: number;
  readonly revision: number;
  readonly position: number;
  readonly onReady: (handle: CanvasWorldHandle) => void;
}) {
  const columns = Math.ceil(Math.sqrt(count - 1));
  const rows = Math.ceil((count - 1) / columns);
  const width = 4 / columns;
  const height = 2.5 / rows;
  return (
    <>
      <Entity id="diagnostic-surface">
        <Surface width={4} height={2.5} />
      </Entity>
      <CanvasWorld
        presentation={{ anchor: "diagnostic-surface" }}
        extent={[4, 2.5]}
        unitsPerMetre={1}
        create={{
          symbolicId: "gui-diagnostic-panel",
          selectedSystems: [
            "ipp.animation",
            "ipp.gui",
            "ipp.gui-layout",
            "ipp.canvas",
            "ipp.asset-dependencies",
            "ipp.lifecycle-publisher",
          ],
        }}
        onReady={onReady}
      >
        <Entity id="diagnostic-root">
          <Layout kind={3} width={4} height={2.5} />
          <Children>
            {Array.from({ length: count - 1 }, (_, index) => (
              <Entity key={index} id={`diagnostic-box-${index}`}>
                <Layout width={width * 0.9} height={height * 0.9} />
                <Style
                  x={(index % columns) * width + (index === 0 ? position : 0)}
                  y={Math.floor(index / columns) * height}
                  red={index === 0 ? 0.2 + (revision % 2) * 0.6 : 0.1}
                  green={0.4}
                  blue={0.7}
                />
                <Box width={width * 0.9} height={height * 0.9} />
              </Entity>
            ))}
          </Children>
        </Entity>
      </CanvasWorld>
    </>
  );
}
