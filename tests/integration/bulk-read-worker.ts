/** Pending real IO, peer contract progress and explicit revocation on a production worker. */
import { createWorkerHost } from "../../packages/ipp-client/src/worker.js";
import { BulkReadParticipant } from "./scenarios/bulk-reads.js";

export async function probe(urls: {
  generated: string;
  wasm: string;
  workerScript: string;
}) {
  const contract = await import(urls.generated);
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 32;
  document.body.append(canvas);
  const owner = createWorkerHost(
    urls.workerScript,
    urls.wasm,
    contract.MAX_MESSAGE_BYTES,
    { canvas: canvas.transferControlToOffscreen() },
  );
  const pending = new BulkReadParticipant(owner.connect(), 8000);
  const peer = new BulkReadParticipant(owner.connect(), 8000);
  try {
    await pending.open();
    await peer.open();
    const fixture = await pending.fixture(0);
    const read = new DataView(fixture.buffer, fixture.byteOffset).getBigUint64(
      21,
      true,
    );
    pending.send(read, 0);
    const awaited = pending.receive();
    const descriptor = await peer.descriptor();
    const bytes = await peer.readWithoutEofAcknowledgement(descriptor);
    if (
      (await peer.status(
        descriptor.reference.read,
        1,
        descriptor.length,
        true,
      )) !== 1
    )
      throw new Error("Peer EOF acknowledgement failed");
    await peer.fixture(1);
    const failed = await awaited;
    if (
      failed[28] !== 2 ||
      !new TextDecoder().decode(failed).includes("severe Host memory pressure")
    )
      throw new Error("Pending IO did not fail explicitly on pressure");
    const notice = await pending.receive();
    if (
      notice[28] !== 2 ||
      new DataView(notice.buffer, notice.byteOffset).getBigUint64(12, true) !==
        0n
    )
      throw new Error("Missing reliable revocation notice");
    if ((await pending.status(read, 0)) !== 2)
      throw new Error("Revoked read remained usable");
    const replacement = await peer.descriptor();
    const after = await peer.readWithoutEofAcknowledgement(replacement);
    if (!after.every((byte, at) => byte === bytes[at]))
      throw new Error("Peer contract damaged by pending IO revocation");
    await peer.status(replacement.reference.read, 1, replacement.length, true);
    return {
      bytes: bytes.length,
      pendingFailed: true,
      noticeReceived: true,
      peerResponsive: true,
    };
  } finally {
    await Promise.allSettled([pending.close(), peer.close()]);
    await owner.close();
    canvas.remove();
  }
}
