import assert from "node:assert/strict";
import test from "node:test";
import {
  generateClient,
  hostAnnouncement,
  replyToHostCreate,
} from "./generated-client.mjs";

const { codec, hostProtocol, hostPresentation } =
  await generateClient("host-lifecycle");

function controlledTransport() {
  let events;
  let closes = 0;
  const sent = [];
  const replies = [];
  let hold = false;
  let nextSession = 7n;
  const deliver = (bytes) => {
    if (hold) replies.push(bytes);
    else events.message(bytes);
  };
  return {
    sent,
    replies,
    get closes() {
      return closes;
    },
    hold() {
      hold = true;
    },
    release() {
      hold = false;
      for (const bytes of replies.splice(0)) events.message(bytes);
    },
    emit(bytes) {
      events.message(bytes);
    },
    fail(error) {
      events.error(error);
    },
    disconnect() {
      events.closed();
    },
    transport: {
      start(value) {
        events = value;
        events.ready();
      },
      send(bytes) {
        sent.push(bytes.slice());
        if (bytes.length === 4) {
          const response = hostAnnouncement(codec);
          deliver(response);
        } else if (bytes[24] === codec.WIRE.HOST_REQUEST_RESOLVE_WORLD) {
          const response = new Uint8Array(41);
          response.set(bytes.subarray(0, 24));
          response[3] = 65;
          response[24] = codec.WIRE.HOST_RESPONSE_WORLD_REFERENCE;
          const view = new DataView(response.buffer);
          view.setBigUint64(25, 1n, true);
          view.setBigUint64(33, 1n, true);
          deliver(response);
        } else replyToHostCreate(bytes, { message: deliver }, nextSession++);
      },
      async close() {
        closes++;
      },
    },
  };
}

test("Host requests beyond the Host's admission window wait instead of failing", async () => {
  const controlled = controlledTransport();
  const host = await codec.IppHostClient.connectTransport(controlled.transport);
  try {
    const created = await host.createWorld({ selectedSystems: [] });
    const client = await host.openWorld(created.reference);
    controlled.hold();
    const pending = Array.from({ length: 65 }, () => host.resolveWorld(1n));
    const closing = client.close();
    assert.equal((await client.closed).reason.message, "Client closed");
    assert.equal(host.sessions.size, 1);
    controlled.release();
    await Promise.all(pending);
    await closing;
    assert.equal(host.sessions.size, 0);
    assert.equal(
      controlled.sent.filter(
        (bytes) => bytes[24] === codec.WIRE.HOST_REQUEST_DETACH_WORLD,
      ).length,
      1,
    );
    assert.equal(controlled.closes, 0);
  } finally {
    await host.close();
  }
});

for (const tag of ["HOST_RESPONSE_ERROR", "HOST_RESPONSE_ATTACHED"]) {
  test(`malformed ${tag} rejects its waiter and closes physical transport`, async () => {
    const controlled = controlledTransport();
    const host = await codec.IppHostClient.connectTransport(
      controlled.transport,
    );
    controlled.hold();
    const opening = host.openWorld({ id: 1n, incarnation: 1n });
    const response = controlled.replies[0].slice(0, 25);
    response[24] = codec.WIRE[tag];
    const rejected = assert.rejects(opening, /truncated/i);
    controlled.emit(response);
    await rejected;
    assert.equal(controlled.closes, 1);
    assert.equal(host.sessions.size, 0);
    await host.close();
  });
}

test("a transport that sends then throws terminally fences the shared Host", async () => {
  const controlled = controlledTransport();
  const host = await codec.IppHostClient.connectTransport(controlled.transport);
  const created = await host.createWorld({ selectedSystems: [] });
  const first = await host.openWorld(created.reference);
  const second = await host.openWorld(created.reference);
  controlled.hold();
  const send = controlled.transport.send;
  controlled.transport.send = (bytes) => {
    send(bytes);
    throw new Error("injected post-send exception");
  };
  await assert.rejects(
    host.createWorld({ selectedSystems: [] }),
    /outcome is unknown/,
  );
  assert.equal(
    (await first.closed).reason.message,
    (await second.closed).reason.message,
  );
  assert.equal(host.sessions.size, 0);
  assert.equal(controlled.closes, 1);
  await assert.rejects(host.resolveWorld(created.id), /closed/);
  await host.close();
});

test("local pre-send validation leaves the Host reusable", async () => {
  const controlled = controlledTransport();
  const host = await codec.IppHostClient.connectTransport(controlled.transport);
  const count = controlled.sent.length;
  await assert.rejects(
    host.createWorld({ selectedSystems: [], capacityHints: { entities: -1 } }),
    /unsigned/,
  );
  assert.equal(controlled.sent.length, count);
  assert.equal(controlled.closes, 0);
  assert.ok((await host.createWorld({ selectedSystems: [] })).reference);
  await host.close();
});

test("a World creation without a System selection is refused before sending", async () => {
  const controlled = controlledTransport();
  const host = await codec.IppHostClient.connectTransport(controlled.transport);
  const count = controlled.sent.length;
  for (const options of [undefined, {}, { symbolicId: "unselected" }]) {
    await assert.rejects(host.createWorld(options), (error) => {
      assert.ok(error instanceof codec.WorldSelectionRequiredError);
      assert.equal(error.code, "IPP_WORLD_SELECTION_REQUIRED");
      return true;
    });
  }
  assert.equal(controlled.sent.length, count);
  assert.equal(controlled.closes, 0);
  assert.ok((await host.createWorld({ selectedSystems: [] })).reference);
  await host.close();
  // A convenience connection refuses before its World request and closes the
  // Host connection it owns.
  const convenience = controlledTransport();
  await assert.rejects(
    codec.IppClient.connectTransport(convenience.transport, {}),
    codec.WorldSelectionRequiredError,
  );
  assert.equal(convenience.closes, 1);
});

for (const stage of ["create", "open"]) {
  test(`convenience startup cancellation during ${stage} closes its owned Host`, async () => {
    const controlled = controlledTransport();
    const originalSend = controlled.transport.send;
    controlled.transport.send = (bytes) => {
      if (
        bytes[24] ===
        codec.WIRE[
          stage === "create"
            ? "HOST_REQUEST_CREATE_WORLD"
            : "HOST_REQUEST_OPEN_WORLD"
        ]
      )
        controlled.hold();
      originalSend(bytes);
    };
    const abort = new AbortController();
    const connecting = codec.IppClient.connectTransport(controlled.transport, {
      selectedSystems: [],
      signal: abort.signal,
    });
    const rejected = assert.rejects(connecting, /aborted/i);
    for (
      let attempt = 0;
      attempt < 20 && controlled.replies.length === 0;
      attempt++
    )
      await Promise.resolve();
    assert.equal(controlled.replies.length, 1);
    abort.abort();
    controlled.release();
    await rejected;
    assert.equal(controlled.closes, 1);
  });
}

for (const ending of ["close", "error", "disconnect", "malformed"]) {
  test(`terminal session state is retained after ${ending}`, async () => {
    const controlled = controlledTransport();
    const client = await codec.IppClient.connectTransport(
      controlled.transport,
      { selectedSystems: [], logLevel: "off" },
    );
    assert.equal(client.closure, undefined);
    if (ending === "close") await client.close();
    else if (ending === "error")
      controlled.fail(new Error("injected transport error"));
    else if (ending === "disconnect") controlled.disconnect();
    else controlled.emit(Uint8Array.of(73, 80, 80, 65, 2, 0, 0, 0));
    const closure = await client.closed;
    assert.equal(closure, client.closure);
    assert.ok(closure.reason instanceof Error);
    await client.close();
    assert.equal(await client.closed, closure);
    assert.equal(controlled.closes, 1);
  });
}

function presentationFixture(options = {}) {
  const root = {
    output: {
      world: { id: 1n, incarnation: 2n },
      entity: 3n,
      kind: "camera",
      incarnation: 4n,
    },
    viewport: { width: 2, height: 3, devicePixelRatio: 2 },
    generation: { host: 8n, serial: 9n },
  };
  const view = {
    surface: { id: 1n, context: 2n, maxWidth: 4096, maxHeight: 4096 },
    selection: 11n,
    binding: root,
  };
  const frame = {
    view,
    sequence: 12n,
    publication: { host: 8n, revision: 13n },
    drawCalls: 3,
    triangles: 7,
    failedDrawCalls: 1,
    sources: (options.afterOutputs ?? []).map((output) => ({
      output,
      minimumTick: 5n,
      publication: { host: 8n, revision: 13n },
      tick: 6n,
    })),
  };
  const returnedFrame = structuredClone(frame);
  if (options.frameMismatch === "view") returnedFrame.view.selection += 1n;
  if (options.frameMismatch === "sequence") returnedFrame.sequence = 11n;
  if (options.frameMismatch === "publication")
    returnedFrame.publication.revision -= 1n;
  const calls = [];
  const writeFrame = (writer) => {
    writer.u64(returnedFrame.view.surface.id);
    writer.u64(returnedFrame.view.surface.context);
    writer.u32(returnedFrame.view.surface.maxWidth);
    writer.u32(returnedFrame.view.surface.maxHeight);
    writer.u64(returnedFrame.view.selection);
    hostPresentation.writeRootBinding(writer, returnedFrame.view.binding);
    writer.u64(returnedFrame.sequence);
    writer.u64(returnedFrame.publication.host);
    writer.u64(returnedFrame.publication.revision);
    writer.u32(returnedFrame.drawCalls);
    writer.u32(returnedFrame.triangles);
    writer.u32(returnedFrame.failedDrawCalls);
    writer.u32(returnedFrame.sources.length);
    for (const source of returnedFrame.sources) {
      writer.u64(source.output.world.id);
      writer.u64(source.output.world.incarnation);
      if (source.output.kind === "canvas") writer.u8(0);
      else {
        writer.u8(1);
        writer.u64(source.output.entity);
        writer.u64(source.output.incarnation);
      }
      writer.u64(source.minimumTick);
      writer.u64(source.publication.host);
      writer.u64(source.publication.revision);
      writer.u64(source.tick);
    }
  };
  const presentation = new codec.HostPresentation(
    async (tag, encode) => {
      assert.equal(tag, codec.WIRE.HOST_REQUEST_PRESENTATION);
      const request = new hostProtocol.HostWireWriter();
      encode(request);
      const read = new hostProtocol.HostWireReader(request.finish());
      const operation = read.u8();
      calls.push(operation);
      const response = new hostProtocol.HostWireWriter();
      response.u8(codec.WIRE.HOST_RESPONSE_PRESENTATION);
      if (operation === codec.WIRE.PRESENTATION_REQUEST_FRAME) {
        if (options.error) {
          response.u8(codec.WIRE.PRESENTATION_RESPONSE_ERROR);
          response.u8(codec.WIRE[options.error]);
        } else {
          response.u8(codec.WIRE.PRESENTATION_RESPONSE_CAPTURE);
          writeFrame(response);
          response.u64(17n);
          response.u64(24n);
        }
      } else if (operation === codec.WIRE.PRESENTATION_REQUEST_READ_CAPTURE) {
        assert.equal(read.u64(), 17n);
        assert.equal(read.u64(), 0n);
        read.end();
        if (options.chunkFailure)
          throw new Error("injected capture chunk failure");
        response.u8(codec.WIRE.PRESENTATION_RESPONSE_CHUNK);
        response.u64(17n);
        response.u64(0n);
        response.bytes(new Uint8Array(24).fill(42));
      } else {
        assert.equal(
          operation,
          codec.WIRE.PRESENTATION_REQUEST_RELEASE_CAPTURE,
        );
        assert.equal(read.u64(), 17n);
        read.end();
        if (options.releaseFailure) throw new Error("injected release failure");
        response.u8(codec.WIRE.PRESENTATION_RESPONSE_COMPLETE);
      }
      const bytes = response.finish();
      const omitted =
        operation !== codec.WIRE.PRESENTATION_REQUEST_FRAME
          ? 0
          : options.truncated === "beforeCapture"
            ? 16
            : options.truncated === "afterCapture"
              ? 8
              : 0;
      return new hostProtocol.HostWireReader(
        bytes.subarray(0, bytes.length - omitted),
      );
    },
    (name) => codec.WIRE[name],
  );
  return {
    presentation,
    view,
    frame: returnedFrame,
    expectedFrame: frame,
    calls,
  };
}

test("generated Host capture preserves exact root/context/publication stamp and releases transfer", async () => {
  const output = { world: { id: 5n, incarnation: 6n }, kind: "canvas" };
  const fixture = presentationFixture({ afterOutputs: [output] });
  const captured = await fixture.presentation.capture(fixture.view, {
    afterSequence: 11n,
    publication: fixture.frame.publication,
    afterOutputs: [output],
  });
  assert.deepEqual(captured.sources, fixture.frame.sources);
  assert.deepEqual(captured.view, fixture.view);
  assert.deepEqual(captured.publication, fixture.frame.publication);
  assert.equal(captured.sequence, 12n);
  assert.deepEqual(
    new Uint8Array(captured.pixels),
    new Uint8Array(24).fill(42),
  );
  assert.deepEqual(fixture.calls, [
    codec.WIRE.PRESENTATION_REQUEST_FRAME,
    codec.WIRE.PRESENTATION_REQUEST_READ_CAPTURE,
    codec.WIRE.PRESENTATION_REQUEST_RELEASE_CAPTURE,
  ]);
});

test("generated Host capture reports typed headless rejection", async () => {
  const fixture = presentationFixture({
    error: "PRESENTATION_ERROR_UNSUPPORTED",
  });
  await assert.rejects(
    fixture.presentation.capture(fixture.view),
    (error) =>
      error instanceof codec.PresentationError &&
      error.reason === "unsupported",
  );
  assert.equal(fixture.calls.length, 1);
});

test("generated Host capture retains exact cleanup identity when read and release both fail", async () => {
  const fixture = presentationFixture({
    chunkFailure: true,
    releaseFailure: true,
  });
  await assert.rejects(
    fixture.presentation.capture(fixture.view),
    (error) =>
      error instanceof codec.CaptureTransferError &&
      error.capture === 17n &&
      error.frame.sequence === 12n &&
      error.cause instanceof AggregateError &&
      error.cause.errors.length === 2,
  );
  assert.equal(
    fixture.calls.at(-1),
    codec.WIRE.PRESENTATION_REQUEST_RELEASE_CAPTURE,
  );
});

for (const frameMismatch of ["view", "sequence", "publication"]) {
  for (const releaseFailure of [false, true]) {
    test(`generated capture rejects wrong ${frameMismatch} fence and releases acknowledged transfer; releaseFailure=${releaseFailure}`, async () => {
      const fixture = presentationFixture({ frameMismatch, releaseFailure });
      const error = await fixture.presentation
        .capture(fixture.view, {
          afterSequence: 11n,
          publication: fixture.expectedFrame.publication,
        })
        .then(
          () => assert.fail("Mismatched capture unexpectedly succeeded"),
          (error) => error,
        );
      assert.deepEqual(fixture.calls, [
        codec.WIRE.PRESENTATION_REQUEST_FRAME,
        codec.WIRE.PRESENTATION_REQUEST_RELEASE_CAPTURE,
      ]);
      assert.ok(error instanceof codec.CaptureTransferError);
      assert.equal(error.capture, 17n);
      assert.deepEqual(error.frame, fixture.frame);
      if (releaseFailure) {
        assert.ok(error.cause instanceof AggregateError);
        assert.equal(error.cause.errors.length, 2);
        assert.match(error.cause.errors[0].message, /presentation fence/);
        assert.match(error.cause.errors[1].message, /injected release failure/);
      } else {
        assert.match(error.cause.message, /presentation fence/);
      }
    });
  }
}

for (const truncated of ["beforeCapture", "afterCapture"]) {
  test(`generated capture truncated ${truncated} cleans only an acknowledged transfer`, async () => {
    const fixture = presentationFixture({ truncated });
    const error = await fixture.presentation.capture(fixture.view).then(
      () => assert.fail("Truncated capture unexpectedly succeeded"),
      (error) => error,
    );
    if (truncated === "beforeCapture") {
      assert.ok(!(error instanceof codec.CaptureTransferError));
      assert.match(error.message, /Truncated/);
      assert.deepEqual(fixture.calls, [codec.WIRE.PRESENTATION_REQUEST_FRAME]);
    } else {
      assert.ok(error instanceof codec.CaptureTransferError);
      assert.equal(error.capture, 17n);
      assert.deepEqual(error.frame, fixture.frame);
      assert.match(error.cause.message, /Truncated/);
      assert.deepEqual(fixture.calls, [
        codec.WIRE.PRESENTATION_REQUEST_FRAME,
        codec.WIRE.PRESENTATION_REQUEST_RELEASE_CAPTURE,
      ]);
    }
  });
}
