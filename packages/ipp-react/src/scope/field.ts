import { useEffect, useState } from "react";
import type { ComponentFieldValue } from "@ipp/client";
import { trackFieldWatch, useWorld, type ReactWorldScope } from "./world.js";
import type { ReactFieldWatchLease } from "./field-watches.js";

export interface ReactFieldState {
  readonly status: "pending" | "ready" | "absent" | "failed" | "closed";
  readonly value?: ComponentFieldValue;
  readonly error?: Error;
}

interface Observation {
  scope: ReactWorldScope;
  entity: string | bigint;
  component: string | number;
  field: string | number;
  state: ReactFieldState;
}

/** Observe native reports. Routine React commits never recreate an admitted watch. */
export function useField(
  entity: string | bigint,
  component: string | number,
  field: string | number,
): ReactFieldState {
  const scope = useWorld();
  const [observation, setObservation] = useState<Observation>();
  useEffect(() => {
    let active = true;
    let admittedTarget: bigint | undefined;
    let generation = 0;
    let lease: ReactFieldWatchLease | undefined;
    // This is the latest native report, never an authored field or a restored write.
    let nativeState: ReactFieldState = { status: "pending" };
    const publish = (state: ReactFieldState) => {
      if (active)
        setObservation((previous) =>
          previous?.scope === scope &&
          previous.entity === entity &&
          previous.component === component &&
          previous.field === field &&
          previous.state.status === state.status &&
          previous.state.error === state.error &&
          Object.is(previous.state.value, state.value)
            ? previous
            : { scope, entity, component, field, state },
        );
    };
    const fail = (reason: unknown) =>
      publish({
        status: "failed",
        error: reason instanceof Error ? reason : new Error(String(reason)),
      });
    const publishNative = () => {
      const commit = scope.getCommitStatus();
      if (commit.status === "failed")
        publish({
          status: "failed",
          ...(commit.error ? { error: commit.error } : {}),
        });
      else if (commit.status === "closed") publish({ status: "closed" });
      else publish(nativeState);
    };
    const detach = () => {
      const previous = lease;
      lease = undefined;
      // The receiving root retains exact failed releases and retries on cleanup.
      if (previous) void previous.release().catch(() => {});
    };
    let admit = () => {};
    publish(nativeState);
    try {
      if (!scope.client.watchLifecycle)
        throw new Error(
          "useField requires lifecycle value watches on the receiving client",
        );
      const descriptor =
        typeof component === "string"
          ? scope.client.components[component]
          : Object.values(scope.client.components).find(
              (value) => value.id === component,
            );
      if (!descriptor) throw new Error(`Unknown component: ${component}`);
      const offset =
        typeof field === "number" ? field : descriptor.fields[field]?.offset;
      if (offset === undefined)
        throw new Error(`Unknown field: ${String(component)}.${field}`);
      admit = () => {
        const commit = scope.getCommitStatus();
        if (!active) return;
        if (commit.status === "closed") {
          publish({ status: "closed" });
          generation++;
          detach();
          return;
        }
        if (commit.status === "failed") {
          publish({
            status: "failed",
            ...(commit.error ? { error: commit.error } : {}),
          });
          return;
        }
        if (commit.status !== "ready") return;
        let target: bigint;
        try {
          target = scope.resolveEntity(entity);
        } catch {
          if (admittedTarget !== undefined) {
            admittedTarget = undefined;
            generation++;
            detach();
          }
          nativeState = { status: "absent" };
          publishNative();
          return;
        }
        if (admittedTarget === target) {
          publishNative();
          return;
        }
        admittedTarget = target;
        const admittedGeneration = ++generation;
        detach();
        nativeState = { status: "pending" };
        publishNative();
        const tracked = trackFieldWatch(
          scope,
          scope.client.watchLifecycle!(
            [
              {
                target: {
                  kind: "value",
                  entity: target,
                  component: descriptor.id,
                  fields: [offset],
                },
                kinds: 128,
              },
            ],
            (event) => {
              if (
                !active ||
                generation !== admittedGeneration ||
                event.kind !== "value"
              )
                return;
              const value = event.values?.find(
                (entry) => entry.offset === offset,
              )?.value;
              nativeState =
                value === undefined
                  ? { status: "absent" }
                  : { status: "ready", value };
              publishNative();
            },
          ),
        );
        lease = tracked;
        void tracked.admitted.then(
          (watch) => {
            if (!active || generation !== admittedGeneration) return;
            void watch.closed.then((closure) => {
              if (!active || generation !== admittedGeneration) return;
              admittedTarget = undefined;
              nativeState =
                closure.kind === "closed"
                  ? { status: "closed", error: closure.reason }
                  : { status: "absent" };
              publishNative();
            });
          },
          (error: unknown) => {
            if (!active || generation !== admittedGeneration) return;
            admittedTarget = undefined;
            nativeState = {
              status: "failed",
              error: error instanceof Error ? error : new Error(String(error)),
            };
            fail(error);
          },
        );
      };
    } catch (error) {
      fail(error);
    }
    const unsubscribe = scope.subscribeCommits(admit);
    admit();
    return () => {
      active = false;
      generation++;
      unsubscribe();
      detach();
    };
  }, [scope, entity, component, field]);
  if (
    !observation ||
    observation.scope !== scope ||
    observation.entity !== entity ||
    observation.component !== component ||
    observation.field !== field
  )
    return { status: "pending" };
  return observation.state;
}
