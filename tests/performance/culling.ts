/** Use the existing explicit mesh-derived bounds contract as a benchmark control. */
import type { Command } from "@ipp/client";
import type { BlenderClient } from "../../integrations/blender/client/adapter.js";

export async function addCullingBounds(
  client: BlenderClient,
  targets: readonly bigint[],
): Promise<number> {
  const bounds = client.components.BoundingGeometry!.id;
  const commands: Command[] = targets.map((id) => ({
    kind: "insertComponent",
    entity: { kind: "handle", id },
    component: bounds,
    fields: [],
  }));
  // These empty-field insertions fit well below the byte bound at the operation limit.
  for (let offset = 0; offset < commands.length; offset += 4096) {
    const outcome = await client.batch(commands.slice(offset, offset + 4096));
    if (!outcome.ok) throw new Error("benchmark culling bounds failed");
  }
  return commands.length;
}
