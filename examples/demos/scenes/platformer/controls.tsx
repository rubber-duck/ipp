import { useSyncExternalStore } from "react";
import {
  Button,
  Column,
  Row,
  SegmentedControl,
  TextLine,
} from "@ipp/react/gui-kit";
import type { GallerySceneControlsProps } from "../../shared/scene.js";
import type { PlatformerDemo } from "./playback.js";
import type { PlatformerMode } from "./session.js";

/** One subscription observes playback; the Host owns motion and button feedback. */
export interface PlatformerMovement {
  readonly read: () => PlatformerDemo;
  readonly subscribe: (listener: () => void) => () => void;
  readonly action: (name: string, args?: unknown) => Promise<unknown>;
  readonly report: (error: unknown) => void;
}

export function PlatformerMovementControls({
  movement,
}: {
  readonly movement: PlatformerMovement;
}) {
  const demo = useSyncExternalStore(movement.subscribe, movement.read);
  // Keep controlled selection pending while the requested gait is loading.
  const run = async (name: string, args?: unknown) => {
    await movement.action(name, args).catch(movement.report);
  };
  return (
    <Column id="platformer-movement" padding="inset" gap="label">
      <Row id="platformer-movement-heading">
        <TextLine
          id="platformer-movement-label"
          text="Movement"
          tone="accent"
          flex={1}
        />
        <TextLine
          id="platformer-direction"
          text={demo.playback.direction === 1 ? "Forward" : "Reverse"}
          size="small"
        />
      </Row>
      <SegmentedControl
        id="platformer-mode"
        options={[
          { value: "walk", label: "Walk", disabled: !demo.session },
          { value: "run", label: "Run", disabled: !demo.session },
          { value: "crawl", label: "Crawl", disabled: !demo.session },
        ]}
        value={demo.playback.mode}
        onChange={(value) => run("setMode", value as PlatformerMode)}
        width={-1}
      />
      <Row id="platformer-movement-actions" gap="label">
        <Button
          id="platformer-reverse"
          label="Reverse"
          selected={demo.playback.direction === -1}
          disabled={!demo.session}
          onPress={() => run("reverse")}
          variant="secondary"
          width={0}
          flex={1}
        />
        <Button
          id="platformer-pause"
          label={demo.playback.playing ? "Pause" : "Resume"}
          selected={!demo.playback.playing}
          disabled={!demo.session}
          onPress={() => run("playback", !demo.playback.playing)}
          variant="secondary"
          width={0}
          flex={1}
        />
        <Button
          id="platformer-reset"
          label="Reset"
          disabled={!demo.session}
          onPress={() => run("reset")}
          variant="secondary"
          width={0}
          flex={1}
        />
      </Row>
    </Column>
  );
}

/** The application slot explains the controls that travel with the character. */
export function PlatformerControls({
  mount,
  report,
}: GallerySceneControlsProps) {
  return (
    <>
      <TextLine
        id="platformer-controls-location"
        text="Movement controls stay above the robot. Choose a gait, reverse, pause or reset there."
        tone="neutral"
        size="small"
      />
      <Button
        id="platformer-reset-camera"
        label="Reset following view"
        variant="secondary"
        width={-1}
        onPress={() => {
          void mount.action("resetCamera").catch(report);
        }}
      />
    </>
  );
}
