import assert from "node:assert/strict";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { encodeManifestLayout, manifestVariant } from "./generated-client.mjs";

for (const directory of ["target/gles-host", "target/browser-build/render"]) {
  test(`complete physical subcodec agrees with executed target ${directory}`, async () => {
    const load = (name) => import(pathToFileURL(resolve(directory, name)).href);
    const codec = await load("generated.js");
    const manifest = await load("generated-manifest.ts");
    const { HostPhysicalInput } = await load("host-input.js");
    const { HostWireReader, HostWireWriter } = await load("host-protocol.js");
    const client = { codec, manifest };
    const covered = new Set();
    const tag = (name) => {
      covered.add(name);
      return manifestVariant(client, name);
    };
    const layout = (name, fields) => encodeManifestLayout(client, name, fields);
    const variant = (name, fields = {}) =>
      layout(manifest.WIRE_TAG_LAYOUTS[name].layout, {
        tag: tag(name),
        ...fields,
      });
    const vector = (x, y) => layout("gui-input-vector", { x, y });
    const framed = (body) => {
      const writer = new HostWireWriter();
      writer.u32(body.bytes.length);
      writer.raw(body.bytes);
      return writer.finish();
    };
    const response = (body) => {
      const writer = new HostWireWriter();
      writer.u8(codec.WIRE.HOST_RESPONSE_GUI_INPUT);
      writer.raw(framed(body));
      return writer.finish();
    };
    let exchange;
    const input = new HostPhysicalInput(
      async (kind, encode, accept) => {
        assert.equal(kind, codec.WIRE.HOST_REQUEST_GUI_INPUT);
        assert.ok(exchange, "unexpected physical request");
        const { request, reply } = exchange;
        exchange = undefined;
        const writer = new HostWireWriter();
        encode(writer);
        assert.deepEqual(writer.finish(), framed(request));
        const bytes = response(reply);
        accept(new HostWireReader(bytes));
        return new HostWireReader(bytes);
      },
      (name) => codec.WIRE[name],
      (name) => {
        const [layoutName, fieldName] = {
          GUI_PHYSICAL_POINTERS: ["gui-physical-cancelled", "pointers"],
          GUI_PHYSICAL_BLOCKERS: ["gui-physical-open", "blockers"],
        }[name];
        return manifest.WIRE_LAYOUTS[layoutName].fields.find(
          (field) => field.name === fieldName,
        ).limit;
      },
    );
    const world = { id: 7n, incarnation: 8n };
    const output = { world, kind: "canvas" };
    const surface = { id: 11n, context: 12n, maxWidth: 100, maxHeight: 100 };
    const binding = {
      output,
      viewport: { width: 100, height: 100, devicePixelRatio: 1 },
      generation: { host: 13n, serial: 14n },
    };
    const view = { surface, binding, selection: 15n };
    const encodedWorld = layout("world-reference", world);
    const encodedView = layout("presentation-view", {
      surface: layout("presentation-surface", {
        id: surface.id,
        context: surface.context,
        max_width: 100,
        max_height: 100,
      }),
      binding: layout("root-binding", {
        output: layout("output-reference", {
          world: encodedWorld,
          target: layout("output-target-canvas", {
            tag: manifestVariant(client, "OUTPUT_TARGET_CANVAS"),
          }),
        }),
        width: 100,
        height: 100,
        device_pixel_ratio: 1,
        generation: layout("presentation-identity", binding.generation),
      }),
      selection: view.selection,
    });
    const blocker = { world, entity: 16n, incarnation: 17n };
    const open = async () => {
      exchange = {
        request: variant("GUI_PHYSICAL_REQUEST_OPEN", {
          view: encodedView,
          blockers: [
            layout("gui-picking-blocker", { ...blocker, world: encodedWorld }),
          ],
        }),
        reply: variant("GUI_PHYSICAL_RESPONSE_OPENED", { context: 1n }),
      };
      return input.open(view, { blockers: [blocker] });
    };
    let context = await open();
    const routed = (disposition = "ROUTED", fields = {}) =>
      variant("GUI_PHYSICAL_RESPONSE_ROUTED", {
        disposition: tag(`GUI_PHYSICAL_DISPOSITION_${disposition}`),
        applied: 1,
        rejected: 0,
        cancelled: 0,
        error: null,
        remaining: null,
        native: null,
        ...fields,
      });
    const send = async (command, expected, reply = routed()) => {
      exchange = {
        request: variant("GUI_PHYSICAL_REQUEST_EVENT", {
          context: 1n,
          input: expected,
        }),
        reply,
      };
      return context.send(command);
    };
    for (const [kind, name] of [
      ["pointerDown", "POINTER_DOWN"],
      ["pointerUp", "POINTER_UP"],
    ]) {
      for (const button of ["primary", "secondary", "auxiliary"]) {
        await send(
          { kind, button, pointer: 3n, point: [0.25, 0.5] },
          variant(`GUI_PHYSICAL_EVENT_${name}`, {
            pointer: 3n,
            point: vector(0.25, 0.5),
            button: tag(`GUI_PHYSICAL_BUTTON_${button.toUpperCase()}`),
          }),
        );
      }
    }
    await send(
      { kind: "pointerMove", pointer: 3n, point: [0.25, 0.5] },
      variant("GUI_PHYSICAL_EVENT_POINTER_MOVE", {
        pointer: 3n,
        point: vector(0.25, 0.5),
      }),
    );
    await send(
      { kind: "pointerCancel", pointer: 3n },
      variant("GUI_PHYSICAL_EVENT_POINTER_CANCEL", { pointer: 3n }),
    );
    for (const [key, suffix] of [
      ["tab", "TAB"],
      ["backTab", "BACK_TAB"],
      ["contextMenu", "CONTEXT_MENU"],
      ...[
        "enter",
        "space",
        "escape",
        "left",
        "right",
        "up",
        "down",
        "home",
        "end",
        "f10",
      ].map((key) => [key, key.toUpperCase()]),
    ]) {
      for (const shift of [undefined, false, true]) {
        await send(
          { kind: "key", key, ...(shift === undefined ? {} : { shift }) },
          variant("GUI_PHYSICAL_EVENT_KEY", {
            key: tag(`GUI_PHYSICAL_KEY_${suffix}`),
            shift: shift === true,
          }),
        );
      }
    }
    for (const [disposition, shift] of [
      ["ROUTED", true],
      ["MISS", false],
      ["BLOCKED", undefined],
      ["UNHANDLED", true],
    ]) {
      const result = await send(
        {
          kind: "wheel",
          point: [0.25, 0.5],
          delta: [4, -5],
          ...(shift === undefined ? {} : { shift }),
        },
        variant("GUI_PHYSICAL_EVENT_WHEEL", {
          point: vector(0.25, 0.5),
          delta: vector(4, -5),
          shift: shift === true,
        }),
        routed(disposition, { remaining: vector(4, -5), error: "bounded" }),
      );
      assert.equal(result.disposition, disposition.toLowerCase());
      assert.deepEqual(result.remaining, [4, -5]);
      assert.equal(result.error, "bounded");
    }
    const target = { world, entity: 18n, component: 36, incarnation: 19n };
    const fence = { target, generation: 20n };
    const encodedTarget = layout("gui-physical-target", {
      ...target,
      world: encodedWorld,
    });
    const nativeState = layout("gui-native-state", {
      target: encodedTarget,
      generation: 20n,
      text: "éa",
      selection_start: 2,
      selection_end: 3,
      composition: layout("gui-native-composition", {
        text: "b",
        selection_start: 0,
        selection_end: 1,
      }),
    });
    const native = layout("gui-native-buffer", {
      byte_length: nativeState.bytes.length,
      state: nativeState,
    });
    input.notification(
      new HostWireReader(
        response(
          variant("GUI_PHYSICAL_RESPONSE_NATIVE", {
            context: 1n,
            state: native,
          }),
        ),
      ),
    );
    assert.deepEqual(context.nativeText, {
      fence,
      text: "éa",
      selectionStart: 2,
      selectionEnd: 3,
      composition: { text: "b", caretStart: 0, caretEnd: 1 },
    });
    const edits = [
      [{ kind: "text", text: "ab" }, "INSERT", { text: "ab" }],
      [
        { kind: "selection", start: 1, end: 2 },
        "SELECTION",
        { start: 1, end: 2 },
      ],
      [
        { kind: "composition", text: "é", caretStart: 0, caretEnd: 2 },
        "COMPOSE",
        {
          composition: layout("gui-native-composition", {
            text: "é",
            selection_start: 0,
            selection_end: 2,
          }),
        },
      ],
      [{ kind: "commitComposition" }, "COMMIT_COMPOSITION", {}],
      [{ kind: "cancelComposition" }, "CANCEL_COMPOSITION", {}],
      ...[
        "backspace",
        "delete",
        "left",
        "right",
        "home",
        "end",
        "selectAll",
        "enter",
      ].map((key) => [
        { kind: "key", key },
        key === "selectAll"
          ? "SELECT_ALL"
          : key === "enter"
            ? "SUBMIT"
            : key.toUpperCase(),
        {},
      ]),
    ];
    for (const [edit, name, fields] of edits) {
      exchange = {
        request: variant("GUI_PHYSICAL_REQUEST_TEXT", {
          context: 1n,
          target: encodedTarget,
          generation: 20n,
          edit: variant(`GUI_NATIVE_EDIT_${name}`, fields),
        }),
        reply: routed("ROUTED", { native }),
      };
      assert.equal((await context.editText(fence, edit)).applied, 1);
    }
    let cancellation;
    context.onCancel((value) => {
      cancellation = value;
    });
    input.notification(
      new HostWireReader(
        response(
          variant("GUI_PHYSICAL_RESPONSE_CANCELLED", {
            context: 1n,
            pointers: [3n, 4n],
            focus: true,
          }),
        ),
      ),
    );
    assert.deepEqual(cancellation, { pointers: [3n, 4n], focus: true });
    assert.equal(context.nativeText, null);
    await assert.rejects(
      send(
        { kind: "blur" },
        variant("GUI_PHYSICAL_EVENT_BLUR"),
        variant("GUI_PHYSICAL_RESPONSE_REJECTED", { reason: "StalePath" }),
      ),
      /StalePath/,
    );
    assert.equal(context.isClosed, false);
    exchange = {
      request: variant("GUI_PHYSICAL_REQUEST_CLOSE", { context: 1n }),
      reply: variant("GUI_PHYSICAL_RESPONSE_CLOSED"),
    };
    await context.close();
    context = await open();
    input.notification(
      new HostWireReader(
        response(variant("GUI_PHYSICAL_RESPONSE_REVOKED", { context: 1n })),
      ),
    );
    assert.equal(context.isClosed, true);
    assert.equal(exchange, undefined);
    assert.deepEqual(
      [...covered].sort(),
      Object.keys(manifest.WIRE_TAG_LAYOUTS)
        .filter(
          (name) =>
            manifest.WIRE_TAG_LAYOUTS[name].space >= 46 &&
            manifest.WIRE_TAG_LAYOUTS[name].space <= 52,
        )
        .sort(),
    );
  });
}
