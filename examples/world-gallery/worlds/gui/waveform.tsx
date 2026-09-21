import {
  guiProperty,
  type AnimationClipSource,
  type AnimationTrack,
  type AnimationPlaybackEvent,
  type ClientAssetSource,
  type GuiNodeHandle,
} from "@ipp/client";
import { Animation, type AnimationHandle } from "@ipp/react";
import {
  Drawing,
  Padding,
  ScrollView,
  Stack,
  type GuiNodeRef,
} from "@ipp/react/gui";
import { useCallback, useEffect, useRef, useState } from "react";
import type { Palette } from "./dashboard.js";
import type { GuiSceneState } from "./scene.js";

const WIDTH = 3.54;
const HEIGHT = 0.46;
const PERIOD = 330;
const PACKET_WIDTH = 110;
const CURVE_SCALE = WIDTH / PERIOD;

function asset(name: string): ClientAssetSource {
  return {
    kind: 18,
    source: new URL(
      `/target/gallery-gui-assets/${name}.ippd`,
      globalThis.location.href,
    ).href,
  };
}

export function waveformResourceSources(): readonly ClientAssetSource[] {
  return ["waveform-grid", "waveform", "waveform-pulse"].map(asset);
}

export interface WaveformMotionAssets {
  readonly scan: ClientAssetSource;
  readonly wavePulse: ClientAssetSource;
  readonly guiComponent: number;
}

function positionTrack(
  component: number,
  samples: readonly (readonly [number, number])[],
): AnimationTrack {
  return {
    property: { component, name: guiProperty(1, "position") },
    keys: samples.map(([time, x], index) => ({
      time,
      value: { kind: "dynamic", value: { kind: "vec2", value: [x, 0] } },
      ...(index + 1 < samples.length
        ? { interpolation: { kind: "linear" as const } }
        : {}),
    })),
  };
}

export function waveformClips(
  component: number,
): readonly AnimationClipSource[] {
  return [
    {
      duration: 2.4,
      tracks: [
        positionTrack(component, [
          [0, 0],
          [2.4, -WIDTH],
        ]),
      ],
    },
    {
      duration: 1.2,
      tracks: [
        positionTrack(component, [
          [0, WIDTH],
          [1.2, -PACKET_WIDTH * CURVE_SCALE],
        ]),
      ],
    },
  ];
}

export interface WaveformRefs {
  readonly signal: GuiNodeRef;
  readonly pulse: GuiNodeRef;
  readonly pulseActive: boolean;
}

export function useWaveformNode() {
  const [node, setNode] = useState<GuiNodeHandle | null>(null);
  const receive = useCallback((next: GuiNodeHandle | null) => {
    setNode((previous) =>
      previous?.session === next?.session &&
      previous?.entity === next?.entity &&
      previous?.rootIncarnation === next?.rootIncarnation &&
      previous?.nodeId === next?.nodeId &&
      previous?.nodeLifetime === next?.nodeLifetime
        ? previous
        : next,
    );
  }, []);
  return [node, receive] as const;
}

export function BoundWaveformAnimations(props: {
  scene: GuiSceneState;
  signal: GuiNodeHandle;
  pulse: GuiNodeHandle;
  onPulseActive: (active: boolean) => void;
}) {
  const { scene, signal, pulse } = props;
  const [bound, setBound] = useState(false);
  useEffect(() => {
    let active = true;
    void scene.prepareWaveform(signal, pulse).then(
      () => {
        if (active) setBound(true);
      },
      (failure: unknown) => {
        if (active) scene.reportFailure(failure);
      },
    );
    return () => {
      active = false;
    };
  }, [scene.prepareWaveform, scene.reportFailure, signal, pulse]);
  return bound ? <WaveformAnimations {...props} /> : null;
}

/** Only the traces move; the viewport and reference grid remain fixed. */
export function Waveform({
  scene,
  palette,
  nodes,
}: {
  scene: GuiSceneState;
  palette: Palette;
  nodes: WaveformRefs;
}) {
  const amplitude = 0.12 + scene.gain * 0.88;
  return (
    <ScrollView width={WIDTH} height={HEIGHT} enabled={false}>
      <Stack width={WIDTH} height={HEIGHT}>
        <Drawing
          width={WIDTH}
          height={HEIGHT}
          asset={asset("waveform-grid")}
          color={palette.muted}
          opacity={0.35}
          enabled={false}
          theme={{
            parts: { icon: { base: { scale: [WIDTH / PERIOD, HEIGHT / 50] } } },
          }}
        />
        <Padding width={WIDTH} height={HEIGHT} padding={[HEIGHT / 2, 0, 0, 0]}>
          <Drawing
            nodeRef={nodes.signal}
            width={WIDTH * 2}
            height={0.01}
            asset={asset("waveform")}
            color={palette.primary}
            opacity={scene.autoscan ? 0.9 : 0.35}
            enabled={false}
            theme={{
              parts: {
                icon: {
                  base: { scale: [CURVE_SCALE, CURVE_SCALE * amplitude] },
                },
              },
            }}
          />
        </Padding>
        <Padding width={WIDTH} height={HEIGHT} padding={[HEIGHT / 2, 0, 0, 0]}>
          <Drawing
            nodeRef={nodes.pulse}
            width={PACKET_WIDTH * CURVE_SCALE}
            height={0.01}
            asset={asset("waveform-pulse")}
            color={palette.hovered}
            opacity={nodes.pulseActive ? 1 : 0}
            enabled={false}
            theme={{
              parts: {
                icon: {
                  base: {
                    scale: [
                      CURVE_SCALE,
                      CURVE_SCALE * (0.65 + scene.gain * 0.35),
                    ],
                  },
                },
              },
            }}
          />
        </Padding>
      </Stack>
    </ScrollView>
  );
}

/** Acknowledged GUI identities are bound once; the Host owns both clocks. */
function WaveformAnimations({
  scene,
  signal,
  pulse,
  onPulseActive,
}: {
  scene: GuiSceneState;
  signal: GuiNodeHandle;
  pulse: GuiNodeHandle;
  onPulseActive: (active: boolean) => void;
}) {
  const scanController = useRef<AnimationHandle>(null);
  const pulseController = useRef<AnimationHandle>(null);
  const previousPulse = useRef(scene.pulseSequence);
  const restart = useRef(Promise.resolve());
  const live = useRef(false);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const onPlaybackEvent = useCallback(
    (event: AnimationPlaybackEvent) => {
      if (
        event.kind !== "completed" ||
        previousPulse.current !== scene.pulseSequence
      )
        return;
      const sequence = previousPulse.current;
      // Completion can arrive after a newer PULSE. Observe state after its restart acknowledgement.
      void restart.current
        .then(() => {
          if (!live.current || previousPulse.current !== sequence)
            return undefined;
          return scene.readWaveformPulse();
        })
        .then(
          (controller) => {
            if (
              live.current &&
              previousPulse.current === sequence &&
              controller?.id === event.controller.id &&
              controller.state === "completed"
            )
              onPulseActive(false);
          },
          (failure: unknown) => {
            if (live.current && previousPulse.current === sequence)
              scene.reportFailure(failure);
          },
        );
    },
    [
      scene.pulseSequence,
      scene.readWaveformPulse,
      scene.reportFailure,
      onPulseActive,
    ],
  );
  useEffect(() => {
    let active = true;
    const action =
      scene.revealed && scene.autoscan
        ? scanController.current?.play()
        : scanController.current?.pause();
    void action?.catch((failure: unknown) => {
      if (active) scene.reportFailure(failure);
    });
    return () => {
      active = false;
    };
  }, [scene.revealed, scene.autoscan, scene.reportFailure]);
  useEffect(() => {
    if (previousPulse.current === scene.pulseSequence) return;
    previousPulse.current = scene.pulseSequence;
    let active = true;
    onPulseActive(true);
    restart.current = pulseController.current!.restart();
    void restart.current.catch((failure: unknown) => {
      if (active) scene.reportFailure(failure);
    });
    return () => {
      active = false;
    };
  }, [scene.pulseSequence, scene.reportFailure, onPulseActive]);
  const motions = scene.motions!;
  return (
    <>
      <Animation
        ref={scanController}
        source={motions.scan.source}
        target={signal.entity}
        bindings={[
          {
            track: 0,
            property: {
              component: motions.guiComponent,
              name: guiProperty(signal.nodeId, "position"),
            },
          },
        ]}
        looping
        autoPlay={false}
      />
      <Animation
        ref={pulseController}
        source={motions.wavePulse.source}
        target={pulse.entity}
        bindings={[
          {
            track: 0,
            property: {
              component: motions.guiComponent,
              name: guiProperty(pulse.nodeId, "position"),
            },
          },
        ]}
        onPlaybackEvent={onPlaybackEvent}
        autoPlay={false}
      />
    </>
  );
}
