import { createRef, StrictMode } from "react";
import type {
  Command,
  GuiAction,
  GuiObservedEffect,
  GuiTarget,
  GuiWorldClient,
} from "@ipp/client";
import { createRoot, Entity, EntityLink } from "@ipp/react";
import {
  Button,
  Checkbox,
  Slider,
  TextInput,
  type GuiControlHandle,
} from "@ipp/react/gui";
import { check, deferred, entity, type GuiContract } from "./gui-authoring.js";
import { guiAction } from "../integration/gui-actions.js";

/** The exact target of `component` on `entity`, from a lifecycle baseline. */
async function controlTarget(
  client: GuiWorldClient,
  entity: bigint,
  component: number,
): Promise<GuiTarget> {
  const watch = await client.watchLifecycle(
    [{ target: { kind: "component", entity, component }, kinds: 8 }],
    () => {},
  );
  try {
    const lifetime = watch.baselines[0]?.lifetime;
    check(
      lifetime?.kind === "component" && lifetime.incarnation !== null,
      "Control component is absent",
    );
    check(client.worldReference, "Control client has no exact World");
    return {
      world: client.worldReference,
      entity,
      component,
      incarnation: lifetime.incarnation,
    };
  } finally {
    await watch.remove();
  }
}

export async function guiCallbacks(
  client: GuiWorldClient,
  peer: GuiWorldClient,
  open: () => Promise<GuiWorldClient>,
  contract: GuiContract,
): Promise<void> {
  const errors: Error[] = [];
  const events: GuiObservedEffect[] = [];
  const order: string[] = [];
  const toggles: boolean[] = [];
  const scalars: number[] = [];
  const texts: string[] = [];
  const submits: string[] = [];
  /** Value actions the button's own element received, by phase. */
  const buttonValues: string[] = [];
  let valueSignal = deferred<void>();
  const button = createRef<GuiControlHandle>();
  const checkbox = createRef<GuiControlHandle>();
  const slider = createRef<GuiControlHandle>();
  const text = createRef<GuiControlHandle>();
  let signal = deferred<void>();
  let batches = 0;
  let subscriptions = 0;
  let holdEvents = false;
  let releasedEvents: (() => void)[] = [];
  let arrived = deferred<void>();
  let holdBatch:
    | {
        arrived: ReturnType<typeof deferred<void>>;
        release: ReturnType<typeof deferred<void>>;
      }
    | undefined;
  let releaseBatch: (() => void) | undefined;
  let rejectCommit = false;
  let throwPress = false;
  let stopCapture = false;
  const wait = async <Value,>(
    promise: Promise<Value>,
    label: string,
  ): Promise<Value> => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([
        promise,
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () =>
              reject(
                new Error(
                  `Callback timeout: ${label}; order=${order}; events=${events.length}; errors=${errors.map((error) => error.message)}`,
                ),
              ),
            8_000,
          );
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
  };
  const observed = new Proxy(client, {
    get(target, property) {
      if (property === "batch")
        return async (commands: Command[]) => {
          batches++;
          const reject =
            rejectCommit &&
            commands.some((command) => command.kind === "setField");
          if (reject) rejectCommit = false;
          const result = await target.batch(
            reject
              ? [
                  ...commands,
                  {
                    kind: "delete",
                    entity: { kind: "handle", id: 0xffffffffffffffffn },
                  },
                ]
              : commands,
          );
          const held = holdBatch;
          if (
            held &&
            commands.some((command) => command.kind === "insertComponent")
          ) {
            holdBatch = undefined;
            held.arrived.resolve();
            await held.release.promise;
          }
          return result;
        };
      if (property === "subscribeGuiEffects")
        return (listener: (event: GuiObservedEffect) => void) => {
          subscriptions++;
          return target.subscribeGuiEffects((event) => {
            events.push(event);
            if (holdEvents) releasedEvents.push(() => listener(event));
            else listener(event);
            arrived.resolve();
          });
        };
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const root = createRoot(observed, { onError: (error) => errors.push(error) });
  const notify = (label: string) => {
    order.push(label);
    signal.resolve();
  };
  const tree = (
    version = "first",
    parent = "callback-parent",
    added = false,
    label = "",
  ) => (
    <StrictMode>
      <Entity
        id="callback-parent"
        onActionCapture={(event) => {
          order.push("parent-capture");
          if (stopCapture) {
            event.stopPropagation();
            signal.resolve();
          }
        }}
        onAction={() => notify("parent-bubble")}
      />
      <Entity
        id="callback-other"
        onActionCapture={() => order.push("other-capture")}
        onAction={() => notify("other-bubble")}
      />
      <Entity
        id="callback-button"
        onActionCapture={(event) => {
          // A Button's selection is a value like any other control's, and
          // bubbles through the element tree; presses are recorded in order.
          if (event.kind === "value") {
            buttonValues.push(`capture:${JSON.stringify(event.value)}`);
            valueSignal.resolve();
          } else order.push("target-capture");
        }}
        onAction={(event) => {
          if (event.kind === "value") {
            buttonValues.push(`bubble:${JSON.stringify(event.value)}`);
            valueSignal.resolve();
          } else order.push("target-bubble");
        }}
      >
        <EntityLink parent={parent} />
        <Button
          ref={button}
          label={label}
          onPress={() => {
            order.push(version);
            if (throwPress) throw new Error("Expected application error");
          }}
        />
      </Entity>
      <Entity id="callback-checkbox">
        <Checkbox
          ref={checkbox}
          onToggle={(event) => {
            toggles.push(event.value);
            valueSignal.resolve();
          }}
        />
      </Entity>
      <Entity id="callback-slider">
        <Slider
          ref={slider}
          onScalarCommit={(event) => {
            scalars.push(event.value);
            valueSignal.resolve();
          }}
        />
      </Entity>
      <Entity id="callback-text">
        <TextInput
          ref={text}
          onTextCommit={(event) => {
            texts.push(event.value);
            valueSignal.resolve();
          }}
          onSubmit={(event) => {
            submits.push(event.value);
            signal.resolve();
          }}
        />
      </Entity>
      {added && (
        <Entity id="callback-added">
          <Button onPress={() => notify("added")} />
        </Entity>
      )}
    </StrictMode>
  );
  const action = async (
    ref: { current: GuiControlHandle | null },
    operation: GuiAction,
  ) => {
    check(ref.current, "Missing callback control ref");
    const outcome = await guiAction(peer, ref.current.target, operation);
    check(outcome.ok, "Semantic callback action did not apply");
    return outcome;
  };
  /** Wait until value callbacks satisfy `condition`. */
  const until = async (condition: () => boolean, label: string) => {
    for (;;) {
      const next = deferred<void>();
      valueSignal = next;
      if (condition()) return;
      await wait(next.promise, label);
    }
  };
  try {
    await wait(root.render(tree()), "initial render");
    // Value callbacks report each control's current value first.
    await until(
      () =>
        toggles.length > 0 &&
        scalars.length > 0 &&
        texts.length > 0 &&
        buttonValues.length > 1,
      "current values",
    );
    const unselected = JSON.stringify({ kind: "selected", value: false });
    check(
      buttonValues.join() === `capture:${unselected},bubble:${unselected}`,
      `Button selection actions: ${buttonValues}`,
    );
    check(
      JSON.stringify([toggles, scalars, texts]) ===
        JSON.stringify([[false], [0], [""]]),
      `Current values: ${JSON.stringify([toggles, scalars, texts])}`,
    );
    const structural = batches;
    await root.render(tree("latest"));
    check(
      batches === structural && subscriptions === 1,
      "Callback-only render wrote structure or resubscribed",
    );
    await action(button, { kind: "press" });
    await wait(signal.promise, "press callback");
    check(
      order.join() ===
        "latest,parent-capture,target-capture,target-bubble,parent-bubble",
      `Callback order: ${order}`,
    );
    check(
      events[0]?.source === "semantic",
      "Foreign session effect lost its source",
    );
    order.length = 0;
    signal = deferred();
    throwPress = true;
    await action(button, { kind: "press" });
    await wait(signal.promise, "throwing callback propagation");
    throwPress = false;
    check(
      errors.length === 1 &&
        errors.pop()?.message === "Expected application error" &&
        order.at(-1) === "parent-bubble",
      "Callback exception interrupted propagation",
    );
    order.length = 0;
    signal = deferred();
    stopCapture = true;
    await action(button, { kind: "press" });
    await wait(signal.promise, "stopped callback propagation");
    stopCapture = false;
    check(
      order.join() === "latest,parent-capture",
      "stopPropagation escaped JavaScript callback scope",
    );
    order.length = 0;
    signal = deferred();
    holdEvents = true;
    arrived = deferred();
    await action(button, { kind: "press" });
    await arrived.promise;
    await root.render(tree("delayed", "callback-other"));
    holdEvents = false;
    for (const deliver of releasedEvents.splice(0)) deliver();
    await wait(signal.promise, "delayed callback");
    check(
      order.join() ===
        "delayed,parent-capture,target-capture,target-bubble,parent-bubble",
      "Delayed callback followed new ancestry",
    );
    await action(checkbox, { kind: "toggle" });
    await until(() => toggles.at(-1) === true, "toggle value");
    await action(slider, { kind: "scalar", value: 0.5 });
    await until(() => scalars.at(-1) === 0.5, "scalar value");
    await action(text, { kind: "text", value: "é🙂" });
    await until(() => texts.at(-1) === "é🙂", "text value");
    check(
      JSON.stringify([toggles, scalars, texts]) ===
        JSON.stringify([
          [false, true],
          [0, 0.5],
          ["", "é🙂"],
        ]),
      `Committed values: ${JSON.stringify([toggles, scalars, texts])}`,
    );
    signal = deferred();
    await action(text, { kind: "submit" });
    await wait(signal.promise, "submit callback");
    check(
      submits.join() === "é🙂",
      "Submission did not carry the committed text",
    );
    check(
      (await text.current!.read()).text === "é🙂" && texts.length === 2,
      "Submit changed the text value",
    );
    const priorCount = events.length;
    const rejected = await guiAction(
      peer,
      {
        ...text.current!.target,
        incarnation: text.current!.target.incarnation + 1n,
      },
      { kind: "submit" },
    );
    check(
      !rejected.ok &&
        rejected.error.reason === "StaleTarget" &&
        events.length === priorCount,
      "Rejected submit fabricated a callback",
    );
    // A compare-and-set is a value change like any other write.
    check(
      await text.current!.compareAndSet("text", "é🙂", "replacement"),
      "Compare-and-set rejected",
    );
    await until(() => texts.at(-1) === "replacement", "replacement value");

    signal = deferred();
    arrived = deferred();
    const held = { arrived: deferred<void>(), release: deferred<void>() };
    holdBatch = held;
    releaseBatch = () => held.release.resolve();
    const rendering = root.render(tree("ready", "callback-other", true));
    await wait(held.arrived.promise, "dynamic author ACK");
    const added = await entity(peer, "callback-added");
    await guiAction(
      peer,
      await controlTarget(peer, added.id, contract.GuiButton.id),
      { kind: "press" },
    );
    await wait(arrived.promise, "dynamic effect before author continuation");
    held.release.resolve();
    await rendering;
    await wait(signal.promise, "dynamic callback");
    check(order.at(-1) === "added", "ACK-held dynamic mount effect was lost");

    const priorTarget = button.current!.target;
    rejectCommit = true;
    check(
      await root
        .render(tree("failed", "callback-other", true, "known-prefix"))
        .then(
          () => false,
          () => true,
        ),
      "Partial callback declaration did not reject",
    );
    const afterFailure = (
      await entity(peer, "callback-button")
    ).components.find(
      (component) => component.component === contract.GuiButton.id,
    )?.fields;
    check(
      afterFailure?.label === "known-prefix",
      "Failed declaration lost its actual known prefix",
    );
    const beforeFailure = order.length;
    arrived = deferred();
    await guiAction(peer, priorTarget, { kind: "press" });
    await wait(arrived.promise, "effect after rejected authoring");
    check(
      await root.flush().then(
        () => false,
        () => true,
      ),
      "Observed effect erased the authored failure cutoff",
    );
    check(
      order.length === beforeFailure,
      "Failed authoring republished callback registrations",
    );
    errors.length = 0;
    await root.render(tree("recovered", "callback-other", true, "corrected"));

    holdEvents = true;
    arrived = deferred();
    await action(button, { kind: "press" });
    await arrived.promise;
    const beforeUnmount = order.length;
    await root.unmount();
    for (const deliver of releasedEvents.splice(0)) deliver();
    await Promise.resolve();
    check(
      order.length === beforeUnmount && button.current === null,
      "Unmounted callback/ref escaped local fence",
    );
    check(errors.length === 0, errors.map((error) => error.message).join("; "));
  } finally {
    releaseBatch?.();
    holdBatch?.release.resolve();
    await root.unmount();
  }
  await callbackLifetimes(open, peer, contract);
}

async function callbackLifetimes(
  open: () => Promise<GuiWorldClient>,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  const client = await open();
  let rejectUnsubscribe = true;
  let unsubscribes = 0;
  let calls = 0;
  let siblingCalls = 0;
  let reenter = true;
  let reentered: Promise<void> | undefined;
  const callbackRan = deferred<void>();
  let heldEffect: (() => void) | undefined;
  let effectArrived = deferred<void>();
  let hold = false;
  const ref = createRef<GuiControlHandle>();
  const wrapped = new Proxy(client, {
    get(target, property) {
      if (property === "subscribeGuiEffects")
        return async (listener: (effect: GuiObservedEffect) => void) => {
          const subscription = await target.subscribeGuiEffects((effect) => {
            if (hold) heldEffect = () => listener(effect);
            else listener(effect);
            effectArrived.resolve();
          });
          return {
            ...subscription,
            unsubscribe: () => {
              unsubscribes++;
              return rejectUnsubscribe
                ? Promise.reject(
                    Object.assign(new Error("Unsubscribe not sent"), {
                      code: "IPP_REQUEST_NOT_SENT",
                    }),
                  )
                : subscription.unsubscribe();
            },
          };
        };
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const root = createRoot(wrapped, { onError: () => {} });
  const healthy = createRoot(peer);
  const healthyRef = createRef<GuiControlHandle>();
  const healthyCalled = deferred<void>();
  const created = await peer.batch([
    contract.Entity.create(1, { symbolicId: "callback-producer" }),
    contract.GuiButton.insert(contract.Entity.alias(1), {}),
  ]);
  check(created.ok, "Callback producer creation");
  try {
    const tree = (duplicate = false) => (
      <>
        <Entity key="first" bindTo="callback-producer">
          <Button
            ref={ref}
            onPress={() => {
              calls++;
              if (reenter) reentered = root.render(tree());
              callbackRan.resolve();
            }}
          />
        </Entity>
        {duplicate && (
          <Entity key="second" bindTo="callback-producer">
            <Button onPress={() => siblingCalls++} />
          </Entity>
        )}
        <Entity id="callback-owned-cleanup">
          <Button onPress={() => calls++} />
        </Entity>
      </>
    );
    await root.render(tree(true));
    const initial = ref.current;
    check(initial, "Borrowed callback target missing");
    await initial.action({ kind: "press" });
    await callbackRan.promise;
    await reentered;
    check(
      calls === 1 && siblingCalls === 0,
      "Reentrant removal invoked an already-fenced sibling listener",
    );
    reenter = false;
    calls = 0;
    effectArrived = deferred();
    await healthy.render(
      <Entity id="callback-healthy">
        <Button ref={healthyRef} onPress={() => healthyCalled.resolve()} />
      </Entity>,
    );
    hold = true;
    await guiAction(peer, initial.target, { kind: "press" });
    await effectArrived.promise;
    check(
      (
        await peer.batch([
          contract.GuiButton.insert(
            contract.Entity.handle(initial.target.entity),
            { label: "replacement" },
          ),
        ])
      ).ok,
      "Callback producer replacement",
    );
    await initial.read().then(
      () => {
        throw new Error("Retired callback ref retargeted");
      },
      () => {},
    );
    await root.render(tree());
    // The press happened on the declared entity's button; it is delivered
    // after the replacement, and the ref follows the new incarnation.
    heldEffect?.();
    await root.flush();
    const replacedRef = (): GuiControlHandle | null => ref.current;
    check(
      calls === 1 &&
        replacedRef()?.target.incarnation !== initial.target.incarnation,
      "A delayed press was lost or the replaced ref kept its retired handle",
    );
    const first = root.unmount();
    check(
      await first.then(
        () => false,
        () => true,
      ),
      "Unsubscribe failure was hidden",
    );
    check(
      (await entity(peer, "callback-producer")).id === initial.target.entity,
      "Observer failure deleted producer",
    );
    check(
      (await peer.inspect()).entities.some(
        (entry) => entry.metadata.symbolicId === "callback-owned-cleanup",
      ),
      "Unmount deleted an owned entity",
    );
    rejectUnsubscribe = false;
    await root.unmount();
    check(
      unsubscribes === 2,
      "Cleanup did not retry exact retained subscription",
    );
    check(healthyRef.current, "Sibling callback root lost its ref");
    await healthyRef.current.action({ kind: "press" });
    await healthyCalled.promise;
    check(!peer.closure && !client.closure, "Cleanup closed a shared session");
  } finally {
    rejectUnsubscribe = false;
    await root.unmount();
    await healthy.unmount();
    await client.close();
  }

  const pendingClient = await open();
  const arrived = deferred<void>();
  const release = deferred<void>();
  let removals = 0;
  let pendingCalls = 0;
  const late = new Proxy(pendingClient, {
    get(target, property) {
      if (property === "subscribeGuiEffects")
        return async (
          ...args: Parameters<GuiWorldClient["subscribeGuiEffects"]>
        ) => {
          const subscription = await target.subscribeGuiEffects(...args);
          arrived.resolve();
          await release.promise;
          return {
            ...subscription,
            unsubscribe: () => {
              removals++;
              return subscription.unsubscribe();
            },
          };
        };
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const lateRoot = createRoot(late);
  const pending = lateRoot.render(
    <Entity id="callback-late">
      <Button onPress={() => pendingCalls++} />
    </Entity>,
  );
  try {
    await arrived.promise;
    const closing = lateRoot.unmount();
    await guiAction(
      peer,
      await controlTarget(
        peer,
        (await entity(peer, "callback-late")).id,
        contract.GuiButton.id,
      ),
      { kind: "press" },
    );
    release.resolve();
    await pending;
    await closing;
    check(
      pendingCalls === 0 && removals === 1,
      "Late subscription ACK revived unmounted callbacks or leaked owner",
    );
  } finally {
    release.resolve();
    await lateRoot.unmount();
    await pendingClient.close();
  }
}
