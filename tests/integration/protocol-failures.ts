import { protocolRejected } from "./assertions.js";
import type { HarnessDriverFactory, MalformedCase } from "./driver.js";
import type { EvidenceRecorder } from "./evidence.js";

export interface ProtocolFailureContext {
  readonly factory: HarnessDriverFactory;
  readonly url: string;
  readonly signal: AbortSignal;
  readonly evidence: EvidenceRecorder;
}

export async function schemaMismatchFailsDuringHandshake(
  context: ProtocolFailureContext,
): Promise<void> {
  const result = await context.factory.rejectMismatchedSchema(context.url, {
    signal: context.signal,
    record: context.evidence.record.bind(context.evidence),
  });
  await context.evidence.record("schema_mismatch_result", result);
  protocolRejected(result, "handshake");
}

export async function staleSessionFailsBeforeMutation(
  context: ProtocolFailureContext,
): Promise<void> {
  const result = await context.factory.rejectStaleSession(context.url, {
    signal: context.signal,
    record: context.evidence.record.bind(context.evidence),
  });
  await context.evidence.record("stale_session_result", result);
  protocolRejected(result, "request");
}

export async function malformedRequestsFailExplicitly(
  context: ProtocolFailureContext,
): Promise<void> {
  const cases: readonly MalformedCase[] = [
    "no-bootstrap",
    "oversized-message",
    "trailing-bytes",
    "unknown-tag",
    "removed-step-tag",
  ];
  for (const malformedCase of cases) {
    const result = await context.factory.rejectMalformed(
      context.url,
      malformedCase,
      {
        signal: context.signal,
        record: context.evidence.record.bind(context.evidence),
      },
    );
    await context.evidence.record("malformed_request_result", {
      malformedCase,
      result,
    });
    protocolRejected(
      result,
      malformedCase === "no-bootstrap" ? "handshake" : "request",
    );
  }
}
