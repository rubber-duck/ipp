import assert from "node:assert/strict";
import test from "node:test";
import { generateClient } from "./generated-client.mjs";
import { guiCodecCases } from "./gui-semantics-codec.mjs";

await guiCodecCases("native", "target/gui-host");
const target = {
  world: { id: 1n, incarnation: 2n },
  entity: 3n,
  component: 51,
  incarnation: 4n,
};

const lean = await generateClient("gui-local-lean");
test("lean contract omits ordinary GUI declarations and callable methods", () => {
  assert.equal(lean.codec.WIRE.COMMAND_GUI_ACTION, undefined);
  assert.equal(lean.codec.WIRE.REQUEST_GUI_OBSERVATION, undefined);
  assert.equal(lean.codec.WIRE.RESPONSE_GUI_OBSERVATION, undefined);
  assert.equal("guiAction" in lean.codec.Entity, false);
  assert.equal("subscribeGuiEffects" in lean.codec.IppClient.prototype, false);
  assert.equal(lean.source.includes("function writeGuiAction("), false);
  assert.equal(lean.manifestSource.includes("gui-local"), false);
  assert.throws(() =>
    lean.codec.encodeRequest({
      session: 7n,
      requestId: 9n,
      body: { kind: "inspect", collection: "guiFocus" },
    }),
  );
  assert.throws(() =>
    lean.codec.encodeRequest({
      session: 7n,
      requestId: 9n,
      body: {
        kind: "submitBatch",
        batchId: 1,
        last: true,
        operations: [
          {
            kind: "guiAction",
            entity: { kind: "handle", id: target.entity },
            component: target.component,
            incarnation: target.incarnation,
            action: { kind: "press" },
          },
        ],
      },
    }),
  );
});
