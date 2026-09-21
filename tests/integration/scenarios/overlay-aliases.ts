/** Observable alias behavior, independent of transport and generated field offsets. */
export interface OverlayAliasObservation {
  readonly value: number;
  readonly source: string | null;
  readonly sourceA: string;
  readonly sourceB: string | null;
}

export interface OverlayAliasDriver {
  attachToNewSource(): Promise<void>;
  updateToNewSource(): Promise<void>;
  rejectInvalidSource(deleted: boolean): Promise<string>;
  releaseStateOverlayOwner(): Promise<void>;
  observe(): Promise<OverlayAliasObservation>;
}

export async function overlayAliasesResolve(driver: OverlayAliasDriver) {
  await driver.attachToNewSource();
  const attached = await driver.observe();
  requireCondition(
    attached.value === 6 && attached.source === attached.sourceA,
    "Attachment must resolve the earlier source alias and evaluate its driver",
  );
  await driver.updateToNewSource();
  const updated = await driver.observe();
  requireCondition(
    updated.value === 15 && updated.source === updated.sourceB,
    "ComponentStateOverlay update must retain the resolved new source",
  );
  for (const deleted of [false, true]) {
    const error = await driver.rejectInvalidSource(deleted);
    requireCondition(
      error === (deleted ? "InvalidEntity" : "UnknownAlias"),
      "Invalid aliases must reject through ordinary field preparation",
    );
    const preserved = await driver.observe();
    requireCondition(
      preserved.value === 15 && preserved.source === updated.sourceB,
      "Rejected overlay update must preserve its previous binding and value",
    );
  }
  await driver.releaseStateOverlayOwner();
  const released = await driver.observe();
  requireCondition(
    released.value === 0 && released.source === null,
    "Owner release must remove the fallback driver and preserve the bound entity's base",
  );
  return { attached, updated, released };
}

function requireCondition(value: boolean, message: string): asserts value {
  if (!value) throw new Error(message);
}
