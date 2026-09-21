/** Run against the native host: python tools/ipp.py dev headless-client ws://127.0.0.1:9231 */
import {
  Entity,
  IppClient,
  LinearDriver,
  Scalar,
} from "../../target/integration-artifacts/client/generated.js";

const client = await IppClient.connectWebSocket(
  process.argv[2] ?? "ws://127.0.0.1:9231",
);
try {
  const created = await client.batch([
    Entity.create(1, { symbolicId: "source", classes: ["demo"] }),
    Scalar.insert(Entity.alias(1), { value: 4 }),
    Entity.create(2, { symbolicId: "driven", classes: ["demo"] }),
    Scalar.insert(Entity.alias(2), { value: 99 }),
    LinearDriver.insert(Entity.alias(2), {
      source: Entity.alias(1),
      scale: 2,
      bias: 1,
    }),
  ]);
  if (!created.ok)
    throw new Error(`Creation rejected: ${created.error.reason}`);
  const source = created.aliases.find((entry) => entry.alias === 1);
  if (!source) throw new Error("Missing acknowledged source identity");

  await client.waitForFrame(created.tick);
  console.log("After the initial frame:");
  print(await client.inspect());

  const updated = await client.batch([
    Scalar.setValue(Entity.handle(source.id), 6),
  ]);
  if (!updated.ok) throw new Error(`Update rejected: ${updated.error.reason}`);
  console.log("After updating the source:");
  print(await client.inspect());
} finally {
  await client.close();
}

function print(value: unknown): void {
  console.log(
    JSON.stringify(
      value,
      (_key, item: unknown) =>
        typeof item === "bigint" ? item.toString() : item,
      2,
    ),
  );
}
