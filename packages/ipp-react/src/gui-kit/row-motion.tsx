/**
 * An entity's own skin row and its motion on the Host clock. A skinned entity
 * that is not a control paints its Background from its own override row over
 * its theme's row, property by property; the row's lengths are absolute in
 * the World's units. Kit parts that differ per entity, such as a ring's sweep
 * or the cut ends of a run of fills, carry them there, so one theme serves
 * every size and position.
 *
 * A looping motion of one property of that row is an ordinary animation on
 * the Host clock: the arc's turn and the leading section's pulse. Animation
 * adds the clip's change since its first key to the authored value, so the
 * authored value is the motion's rest and every key stays a change from it;
 * a result outside the property's range is dropped, so the rest must keep the
 * whole motion in range. A row property takes dynamic values. The animation
 * exists only while it is declared and the kit's motion is not reduced; while
 * it is, its World must select the animation System and its canvas changes
 * every frame. Under reduced motion the property holds its rest.
 */
import { useMemo } from "react";
import { Animation } from "../animation.js";
import { AnimationAsset, assetRef } from "../assets.js";
import { useGuiKit, type GuiKitRow } from "./kit.js";

/** The slot of an entity's own row. */
const SLOT = 0;

/**
 * Seconds per cycle of the leading section's pulse: a calm breath, slower
 * than the spinner's turn.
 */
const PULSE_PERIOD = 1.5;

/**
 * Opacity of the leading section at rest and at the top of its pulse, and at
 * the bottom: dimmer than the fill even at its brightest, so it never reads
 * as progress, and never quite gone.
 */
export const PULSE_REST = 0.6;
const PULSE_LOW = 0.1;

/**
 * Length of the leading section along its track: the language's bar, 8
 * units, about 1% of a wide bar and still plain at small sizes. The fill
 * shares the track with it, so it always fits and the fill never runs ahead.
 */
export const LEAD = 8;

/** Linear pieces per pulse, sampling a cosine so the fade eases at both ends. */
const PULSE_STEPS = 8;

/** One entity's own row, encoded: its Background properties. */
export function useOwnRow(
  row: Readonly<Record<string, unknown>>,
): Uint8Array<ArrayBuffer> {
  const { contract } = useGuiKit();
  // The row's content, not its identity, decides the encoding.
  const content = JSON.stringify(row);
  return useMemo(
    () =>
      contract.GuiSkin.encodeParts({
        nextSlot: SLOT + 1,
        rows: new Map([
          [
            SLOT,
            {
              ...JSON.parse(content),
              part: contract.guiPaintPartIndex({ part: "background" }),
            } as GuiKitRow,
          ],
        ]),
      }),
    [contract, content],
  );
}

/**
 * A looping motion of one property of `target`'s own row through `changes`
 * from its authored value, evenly spaced over `duration` seconds; nothing
 * under reduced motion.
 */
function RowLoop({
  target,
  name,
  property,
  duration,
  changes,
}: {
  readonly target: string;
  readonly name: string;
  readonly property: string;
  readonly duration: number;
  readonly changes: readonly number[];
}) {
  const kit = useGuiKit();
  const skin = kit.contract.GuiSkin;
  const clip = useMemo(
    () => ({
      duration,
      tracks: [
        {
          property: {
            component: skin.id,
            offsets: [skin.partsOffset(SLOT, property)],
          },
          // Numeric keys interpolate linearly to the next by default; the
          // last has no next.
          keys: changes.map((change, index) => ({
            time: (duration * index) / (changes.length - 1),
            value: {
              kind: "dynamic" as const,
              value: { kind: "f32" as const, value: change },
            },
          })),
        },
      ],
    }),
    [skin, property, duration, changes],
  );
  if (kit.reducedMotion) return null;
  return (
    <>
      <AnimationAsset id={`${target}/${name}`} clip={clip} />
      <Animation
        source={assetRef(`${target}/${name}`)}
        target={target}
        looping
        autoPlay
      />
    </>
  );
}

/** One turn of the arc's start, from where it rests. */
const TURN = [0, 1];

/** One turn a second of `target`'s arc start: the spinner's rotation. */
export function Turn({ target }: { readonly target: string }) {
  return (
    <RowLoop
      target={target}
      name="turn"
      property="arc_start"
      duration={1}
      changes={TURN}
    />
  );
}

/** A cosine fade from the rest down to the low opacity and back. */
const PULSE = Array.from(
  { length: PULSE_STEPS + 1 },
  (_, step) =>
    ((PULSE_REST - PULSE_LOW) *
      (Math.cos((2 * Math.PI * step) / PULSE_STEPS) - 1)) /
    2,
);

/** `target`'s opacity fading from its rest down and back up: the liveness cue. */
export function Pulse({ target }: { readonly target: string }) {
  return (
    <RowLoop
      target={target}
      name="pulse"
      property="opacity"
      duration={PULSE_PERIOD}
      changes={PULSE}
    />
  );
}
