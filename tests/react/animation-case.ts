import * as React from "react";
import { createRoot, Entity, Scalar } from "@ipp/react";
import {
  type ReactRuntimeConfiguration,
  connect,
  findEntity,
  rejectedMessage,
} from "./fixture-helpers.js";

export async function animationComponents(
  configuration: ReactRuntimeConfiguration,
) {
  const { client } = await connect(configuration);
  const { Animation, AnimationAsset, Asset, assetRef } = await import(
    "@ipp/react"
  );
  const { ResourceNotificationGate } = await import(
    "./resource-notification-gate.js"
  );
  const gate = new ResourceNotificationGate();
  const errors: string[] = [];
  const root = createRoot(gate.wrap(client), {
    onError: (error) => errors.push(error.message),
  });
  const ref = React.createRef<import("@ipp/react").AnimationHandle>();
  const peer = React.createRef<import("@ipp/react").AnimationHandle>();
  const scalar = client.components.Scalar!;
  const property = {
    component: scalar.id,
    offsets: [scalar.fields.value!.offset],
  };
  const clip = (start: number) => ({
    duration: 1,
    tracks: [
      {
        property,
        keys: [
          { time: 0, value: { kind: "f32" as const, value: start } },
          { time: 1, value: { kind: "f32" as const, value: start + 4 } },
        ],
      },
    ],
  });
  const checks: string[] = [];
  const check = (ok: boolean, message: string) => {
    if (!ok) throw new Error(message);
    checks.push(message);
  };
  const scene = (start: number, show = true, failed = false) =>
    React.createElement(
      React.StrictMode,
      null,
      show &&
        React.createElement(Animation, {
          key: "forward",
          source: assetRef("clip"),
          target: "anim-target",
          bindings: [{ track: 0, property }],
          ref,
          speed: 0,
        }),
      React.createElement(
        Entity,
        { key: "target", id: "anim-target" },
        React.createElement(Scalar, { value: 10 }),
      ),
      React.createElement(
        Entity,
        { key: "peer", id: "anim-peer" },
        React.createElement(Scalar, { value: 20 }),
        show &&
          React.createElement(Animation, {
            source: assetRef("clip"),
            bindings: [{ track: 0, property }],
            ref: peer,
            speed: 0,
          }),
      ),
      failed
        ? React.createElement(Asset, {
            key: "asset",
            id: "clip",
            kind: 10,
            data: start,
            encode: () => new Uint8Array([1, 2, 3]),
          })
        : React.createElement(AnimationAsset, {
            key: "asset",
            id: "clip",
            clip: clip(start),
          }),
    );
  const wait = async (status = "loaded") => {
    for (let i = 0; i < 100; i++) {
      await client.waitForFrame();
      await root.flush();
      if (root.getAsset("clip")?.status === status) return;
    }
    throw new Error(`Animation asset did not reach ${status}`);
  };
  const snapshot = async () => {
    const state = await client.inspect();
    const value = (id: string) =>
      findEntity(state, id)!.effective.find(
        (component) => component.component === scalar.id,
      )!.fields.value;
    return {
      controllers: state.controllers ?? [],
      value: value("anim-target"),
      peer: value("anim-peer"),
    };
  };
  try {
    gate.hold(
      (resource) => resource.kind === 10 && resource.status === "loaded",
    );
    await root.render(scene(2));
    await gate.wait(client);
    const handle = ref.current!;
    await handle.play();
    await handle.pause();
    await handle.seek(0.5);
    await peer.current!.play();
    await peer.current!.pause();
    await peer.current!.seek(0.25);
    check(
      (await snapshot()).controllers.length === 0,
      "refs accept playback intent before assets load without creating unready controllers",
    );
    gate.release();
    await wait();
    const first = await snapshot();
    check(
      first.controllers.length === 2 && first.value === 4 && first.peer === 3,
      "forward and enclosing targets share a clip with independent playback under StrictMode",
    );
    const ids = first.controllers.map((controller) => controller.id);
    gate.hold(
      (resource) => resource.kind === 10 && resource.status === "loaded",
    );
    await root.render(scene(10));
    await gate.wait(client);
    check(
      (await snapshot()).value === 4,
      "pending replacement keeps the previous animated value",
    );
    gate.hold(() => false);
    await root.render(scene(20));
    await wait();
    gate.release();
    await root.flush();
    const replacement = await snapshot();
    check(
      replacement.value === 22 &&
        replacement.peer === 21 &&
        replacement.controllers.every(
          (controller, index) =>
            controller.id === ids[index] &&
            controller.state === "paused" &&
            controller.time === first.controllers[index]!.time,
        ),
      "ready replacement preserves controller identity time and paused state and ignores superseded completion",
    );
    await root.render(scene(30, true, true));
    await wait("failed");
    check(
      (await snapshot()).value === 22 &&
        errors.some((message) => message.includes("failed")),
      "failed replacement preserves the previous controller binding",
    );
    await root.render(scene(20));
    await wait();
    await handle.stop();
    check(
      (await snapshot()).value === 10,
      "stop restores underlying component values",
    );
    await handle.restart();
    await handle.pause();
    await handle.seek(0.75);
    check(
      (await snapshot()).value === 23,
      "restart pause and seek control real Host playback",
    );
    await root.render(scene(20, false));
    check(
      (await snapshot()).controllers.length === 0 &&
        (await snapshot()).peer === 20,
      "removing Animation deletes controllers and restores surviving targets",
    );
    check(
      (await rejectedMessage(handle.play())).includes("unmounted"),
      "retained refs cannot control an unmounted declaration",
    );
    gate.hold(
      (resource) => resource.kind === 10 && resource.status === "loaded",
    );
    await root.render(scene(50));
    await gate.wait(client);
    await ref.current!.play();
    await root.unmount();
    gate.release();
    check(
      (await client.inspect()).controllers?.length === 0,
      "pending unmount cancels playback and late loading cannot recreate controllers",
    );
    return checks;
  } finally {
    gate.release();
    await root.unmount();
    await client.close();
  }
}
