import type {
  Client,
  WorldConnectOptions,
  WorldPersistenceHostClient,
} from "@ipp/client";

export interface HostLifecycleParticipant {
  connectHost(): Promise<WorldPersistenceHostClient<Client>>;
  connectClient(options: WorldConnectOptions): Promise<Client>;
  holdReplies(
    tag:
      | "created"
      | "attached"
      | "worldReference"
      | "worldGraphLoaded"
      | "complete",
  ): Promise<void>;
  releaseReplies(): void;
  truncateReply(tag: "error" | "attached"): void;
  throwAfterSend(): void;
  readonly latestTransferJob: bigint;
  replaceNextHostRequestWithTransfer(
    kind: "cancel" | "read" | "ack",
    job: bigint,
  ): void;
  rejectNextWorldDestroy(): void;
  readonly closed: Promise<void>;
  readonly closeCalls: number;
  close(): Promise<void>;
}

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function rejection(pending: Promise<unknown>): Promise<Error> {
  try {
    await pending;
  } catch (error) {
    check(error instanceof Error, "Expected an Error rejection");
    return error;
  }
  throw new Error("Expected rejection");
}

export async function hostLifecycle(create: () => HostLifecycleParticipant) {
  const uncertain = create();
  try {
    const host = await uncertain.connectHost();
    const created = await host.createWorld({
      selectedSystems: [],
      temporary: true,
    });
    const first = await host.openWorld(created.reference);
    const second = await host.openWorld(created.reference);
    uncertain.throwAfterSend();
    const error = await rejection(
      host.createWorld({ selectedSystems: [], temporary: true }),
    );
    check(
      /outcome is unknown/.test(error.message),
      "Uncertain send remained reusable",
    );
    check(
      (await first.closed).reason.message === error.message &&
        (await second.closed).reason.message === error.message,
      "Uncertain send failed to fence peer sessions",
    );
    await uncertain.closed;
    check(
      uncertain.closeCalls === 1,
      "Uncertain send did not close physical transport",
    );
    await rejection(host.listWorlds());
  } finally {
    await uncertain.close();
  }
  const retry = create();
  try {
    const host = await retry.connectHost();
    const created = await host.createWorld({
      selectedSystems: [],
      temporary: true,
    });
    const client = await host.openWorld(created.reference);
    const peer = await host.openWorld(created.reference);
    // More Host requests than the Host admits on a connection wait for its
    // flow control; none is refused, and closing one session still detaches it.
    const held = retry.holdReplies("worldReference");
    const pending = Array.from({ length: 80 }, () =>
      host.resolveWorld(created.id),
    );
    await client.close();
    check(
      (await client.closed) === client.closure,
      "Local terminal state is not retained",
    );
    check(
      !host.sessions.has(client.session),
      "Close behind outstanding Host requests did not detach the exact session",
    );
    await held;
    retry.releaseReplies();
    check(
      (await Promise.all(pending)).every(
        (reference) => reference.id === created.reference.id,
      ),
      "A Host request beyond the admission window failed",
    );
    check(
      peer.closure === undefined && (await peer.inspect()).tick > 0n,
      "Retry closed a healthy peer",
    );
    check(
      retry.closeCalls === 0,
      "Session-only close terminated the connection",
    );
    await host.destroyWorld(created.reference);
    const destroyed = await peer.closed;
    check(
      destroyed.reason.message.length > 0 && destroyed === peer.closure,
      "Idle external destruction was not observable",
    );
  } finally {
    await retry.close();
  }

  for (const tag of ["error", "attached"] as const) {
    const malformed = create();
    try {
      const host = await malformed.connectHost();
      const created = await host.createWorld({
        selectedSystems: [],
        temporary: true,
      });
      const peer = await host.openWorld(created.reference);
      malformed.truncateReply(tag);
      const error = await rejection(
        host.openWorld(
          tag === "error"
            ? {
                ...created.reference,
                incarnation: created.reference.incarnation + 1n,
              }
            : created.reference,
        ),
      );
      check(
        /truncated/i.test(error.message),
        "Malformed response did not reject its waiter",
      );
      check(
        (await peer.closed).reason.message === error.message,
        "Fatal receive did not terminate attached sessions",
      );
      await malformed.closed;
      check(
        malformed.closeCalls === 1,
        "Fatal receive did not close physical transport once",
      );
    } finally {
      await malformed.close();
    }
  }

  for (const stage of ["created", "attached"] as const) {
    const cancelled = create();
    try {
      const held = cancelled.holdReplies(stage);
      const abort = new AbortController();
      const connecting = rejection(
        cancelled.connectClient({ selectedSystems: [], signal: abort.signal }),
      );
      await held;
      abort.abort();
      const error = await connecting;
      check(
        /aborted/i.test(error.message),
        "Startup returned a live client after cancellation",
      );
      cancelled.releaseReplies();
      await cancelled.closed;
      check(
        cancelled.closeCalls === 1,
        "Cancelled startup leaked its owned Host transport",
      );
    } finally {
      await cancelled.close();
    }
  }
  return {
    closeRetry: true,
    externalClosure: true,
    malformedResponses: 2,
    cancelledStartups: 2,
  };
}
