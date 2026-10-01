import * as React from "react";
import type { AnimationClipSource, Client, Command } from "@ipp/client";
import {
  Animation,
  AnimationAsset,
  Entity,
  EntityLink,
  assetRef,
  createRoot,
  type AnimationHandle,
} from "@ipp/react";
import { findEntity } from "./fixture-helpers.js";

export async function exerciseLinkAnimation(
  client: Client,
  fallback: bigint,
  otherParent: bigint,
): Promise<string[]> {
  const batches: Command[][] = [];
  const observed = new Proxy(client, {
    get(target, key) {
      if (key === "batch")
        return (operations: Command[]) => {
          batches.push(operations);
          return target.batch(operations);
        };
      const value = Reflect.get(target, key);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const root = createRoot(observed);
  const playback = React.createRef<AnimationHandle>();
  const clip: AnimationClipSource = {
    duration: 1,
    tracks: [
      {
        property: { entityLink: true },
        keys: [
          {
            time: 0,
            value: {
              kind: "entityPlacement",
              value: { parent: null, before: null },
            },
            interpolation: { kind: "step" },
          },
          {
            time: 0.5,
            value: {
              kind: "entityPlacement",
              value: { parent: 0, before: null },
            },
            interpolation: { kind: "step" },
          },
          {
            time: 1,
            value: {
              kind: "entityPlacement",
              value: { parent: 1, before: null },
            },
          },
        ],
      },
    ],
  };
  const scene = (onPlaybackEvent: () => void) =>
    React.createElement(
      React.StrictMode,
      null,
      React.createElement(AnimationAsset, { id: "links", clip }),
      React.createElement(Entity, { id: "animated-parent" }),
      React.createElement(
        Entity,
        { bindTo: "producer-child" },
        React.createElement(EntityLink, { parent: fallback }),
        React.createElement(Animation, {
          ref: playback,
          source: assetRef("links"),
          speed: 0,
          bindings: [
            { track: 0, entityBindings: ["animated-parent", otherParent] },
          ],
          onPlaybackEvent,
        }),
      ),
    );
  const checks: string[] = [];
  const check = (condition: boolean, label: string) => {
    if (!condition) throw new Error(label);
    checks.push(label);
  };
  try {
    await root.render(scene(() => {}));
    const deadline = performance.now() + 10000;
    while (root.getAsset("links")?.status !== "loaded") {
      if (performance.now() > deadline)
        throw new Error("Structural clip did not become ready");
      await client.waitForFrame();
      await root.flush();
    }
    await root.flush();
    await playback.current!.play();
    await playback.current!.pause();
    await playback.current!.seek(0.5);
    let state = await client.inspect();
    check(
      findEntity(state, "producer-child")!.link.parent ===
        findEntity(state, "animated-parent")!.id,
      "typed structural animation resolves scene entityBindings after acknowledgement",
    );
    batches.length = 0;
    const controller = state.controllers?.[0]?.id;
    await root.render(scene(() => {}));
    await root.flush();
    check(
      batches.length === 0 &&
        (await client.inspect()).controllers?.[0]?.id === controller,
      "callback-only animation rendering submits no structural operations and retains playback",
    );
    await playback.current!.seek(1);
    state = await client.inspect();
    check(
      findEntity(state, "producer-child")!.link.parent === otherParent,
      "structural animation accepts mixed scene names and runtime entity bindings",
    );
    await playback.current!.stop();
    check(
      findEntity(await client.inspect(), "producer-child")!.link.parent ===
        otherParent,
      "stopping structural animation leaves its last placement",
    );
    // Unmount deletes nothing; removing the declarations deletes them.
    await root.render(null);
    return checks;
  } finally {
    await root.unmount();
  }
}
