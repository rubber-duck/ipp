import { createRef } from "react";
import {
  isLifecycleWatchRemoveError,
  type Client,
  type GuiWorldClient,
  type LifecycleWatchRemoveError,
} from "@ipp/client";
import { createRoot, Entity } from "@ipp/react";
import { Checkbox, type GuiControlHandle } from "@ipp/react/gui";
import type { LifecycleTransportProbe } from "../integration/lifecycle-target-transport.js";
import { check } from "./gui-authoring.js";

export async function controlRefSubsetFailure(
  client: GuiWorldClient,
  peer: Client,
  probe: LifecycleTransportProbe,
) {
  const refs = Array.from({ length: 1400 }, () =>
    createRef<GuiControlHandle>(),
  );
  let starts = 0;
  let batches = 0;
  let partial: LifecycleWatchRemoveError | undefined;
  let retryAllowed = false;
  let rejectedRetries = 0;
  let output: bigint | undefined;
  const submitted: number[] = [];
  const wrapped = new Proxy(client, {
    get(target, property) {
      if (property === "watchLifecycle")
        return async (
          ...args: Parameters<GuiWorldClient["watchLifecycle"]>
        ) => {
          starts++;
          const watch = await target.watchLifecycle(...args);
          output = watch.baselines[0]?.member.output;
          check(
            watch.cuts.length === 2,
            "Large React group did not cross the real SDK byte-page bound",
          );
          return {
            ...watch,
            async removeMembers(
              members: Parameters<typeof watch.removeMembers>[0],
            ) {
              if (partial && !retryAllowed) {
                rejectedRetries++;
                throw Object.assign(new Error("Retained subset was not sent"), {
                  code: "IPP_REQUEST_NOT_SENT",
                });
              }
              submitted.push(members.length);
              try {
                return await watch.removeMembers(members);
              } catch (error) {
                check(
                  isLifecycleWatchRemoveError(error),
                  "Generated removal error failed public recognition",
                );
                partial = error;
                throw error;
              }
            },
          };
        };
      if (property === "batch")
        return (...args: Parameters<GuiWorldClient["batch"]>) => {
          batches++;
          return target.batch(...args);
        };
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const root = createRoot(wrapped, { onError: () => {} });
  try {
    await root.render(
      refs.map((ref, index) => (
        <Entity key={index} id={`paged-control-${index}`}>
          <Checkbox ref={ref} />
        </Entity>
      )),
    );
    check(
      refs.every((ref) => ref.current) && starts === 1,
      "Paged root failed to acknowledge every ref in one group",
    );
    const retained = refs[0]!.current!;
    check(output !== undefined, "Paged watch omitted its output");
    // The client waits for connection credit instead of refusing requests,
    // so the probe answers the second removal page with the Host's enqueue
    // rejection after the first page's ACK: a page definitely not applied.
    probe.afterRemovalAck(client.session, () =>
      probe.refuseNextRemoval(client.session),
    );
    const attempt = root.unmount();
    check(
      refs.every((ref) => ref.current === null),
      "Paged cleanup did not fence locally before ACK",
    );
    const failure = await attempt.then(
      () => null,
      (error: unknown) => error,
    );
    check(
      failure instanceof Error &&
        partial?.cuts.length === 1 &&
        partial.unconfirmedMembers.length > 0 &&
        partial.unconfirmedMembers.length < 2800,
      `Failed second page lost exact acknowledged prefix or fabricated complete cleanup: ${JSON.stringify({ failure: failure instanceof Error ? failure.message : failure, cuts: partial?.cuts.length, unconfirmed: partial?.unconfirmedMembers.length, submitted, requests: probe.requests(client.session) })}`,
    );
    check(!client.closure, "Known-unsent second page closed its session");
    // Unmount deletes nothing: the controls stay whatever happened to the
    // membership pages.
    check(
      (await peer.inspect()).entities.filter((entity) =>
        entity.metadata.symbolicId?.startsWith("paged-control-"),
      ).length === refs.length,
      "Unmount deleted paged controls",
    );
    await retained.read().then(
      () => {
        throw new Error("Paged old ref survived failed cleanup");
      },
      () => {},
    );
    const beforeRetry = { starts, batches };
    retryAllowed = true;
    await root.unmount();
    check(
      starts === beforeRetry.starts && batches === beforeRetry.batches,
      "Partial cleanup retry reran authoring or acquisition",
    );
    check(
      submitted.length === 2 &&
        submitted[0] === 2800 &&
        submitted[1] === partial.unconfirmedMembers.length,
      "Cleanup retry scanned/resubmitted successful membership prefix",
    );
    const requests = probe.requests(client.session);
    check(
      requests.adds === 2 && requests.removes === 2,
      "Partial cleanup did not send exactly two acknowledged add and removal pages",
    );
    console.info("React coalesced partial membership cleanup", {
      submitted,
      rejectedRetries,
      requests,
      beforeRetry,
    });
  } finally {
    retryAllowed = true;
    await root.unmount().catch(() => {});
    const paged = (await peer.inspect()).entities.filter((entity) =>
      entity.metadata.symbolicId?.startsWith("paged-control-"),
    );
    if (paged.length)
      await peer.batch(
        paged.map((entity) => ({
          kind: "delete" as const,
          entity: { kind: "handle" as const, id: entity.id },
        })),
      );
  }
}
