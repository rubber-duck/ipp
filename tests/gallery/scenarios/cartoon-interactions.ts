/** Samples of completed gallery frames in logical canvas coordinates at density one. */
interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export async function journalOutline(rect: Bounds) {
  const frame = await window.ippDemoCanvas!.capture();
  const pixels = new Uint8Array(frame.pixels);
  const stride = frame.view.binding.viewport.width;
  const ink = (x: number, y: number) => {
    const i = 4 * (Math.round(y) * stride + Math.round(x));
    return pixels[i]! < 33 && pixels[i + 1]! < 44 && pixels[i + 2]! < 70;
  };
  const midX = rect.x + rect.width / 2;
  const midY = rect.y + rect.height / 2;
  const offsets = [0, 1, 2, 3, 4, 5];
  return (
    offsets.some((d) => ink(midX, rect.y - 1 - d)) &&
    offsets.some((d) => ink(midX, rect.y + rect.height + d)) &&
    offsets.some((d) => ink(rect.x + d, midY)) &&
    offsets.some((d) => ink(rect.x + rect.width - d, midY))
  );
}

export async function stepCornersStayClear(rect: Bounds) {
  for (let n = 0; n < 18; n++) {
    const frame = await window.ippDemoCanvas!.capture();
    const pixels = new Uint8Array(frame.pixels);
    const stride = frame.view.binding.viewport.width;
    const rgb = (dx: number, dy: number) => {
      const i =
        4 * (Math.round(rect.y + dy) * stride + Math.round(rect.x + dx));
      return (pixels[i]! << 16) | (pixels[i + 1]! << 8) | pixels[i + 2]!;
    };
    const surface = rgb(-8, -9);
    if (
      rgb(-5, -6) !== surface ||
      rgb(rect.width + 2, rect.height + 3) !== surface
    )
      return false;
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
  return true;
}

export async function rangeLabelsStayVisible(rail: Bounds) {
  // The two pulls stay on opposite halves of the rail. Only the readouts use
  // the cream text colour in this band immediately below the thumbs.
  for (let n = 0; n < 36; n++) {
    const frame = await window.ippDemoCanvas!.capture();
    const pixels = new Uint8Array(frame.pixels);
    const stride = frame.view.binding.viewport.width;
    const mid = rail.x + rail.width / 2;
    let lower = 0;
    let upper = 0;
    for (
      let y = Math.ceil(rail.y + rail.height + 2);
      y < rail.y + rail.height + 20;
      y++
    ) {
      for (let x = Math.floor(rail.x); x < rail.x + rail.width; x++) {
        const i = 4 * (y * stride + x);
        if (pixels[i]! > 220 && pixels[i + 1]! > 215 && pixels[i + 2]! > 170) {
          if (x < mid) lower++;
          else upper++;
        }
      }
    }
    if (lower < 20 || upper < 20) return false;
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
  }
  return true;
}

/** Sample the face away from text and measure the height of its painted contour. */
export async function choiceHighlight({
  rect,
  count = 20,
  checkCorners = false,
}: {
  rect: Bounds;
  count?: number;
  checkCorners?: boolean;
}) {
  const samples: { green: number; height: number }[] = [];
  for (let n = 0; n < count; n++) {
    const frame = await window.ippDemoCanvas!.capture();
    const pixels = new Uint8Array(frame.pixels);
    const width = frame.view.binding.viewport.width;
    const offset = (x: number, y: number) =>
      4 * (Math.round(y) * width + Math.round(x));
    const x = rect.x + rect.width * 0.8;
    const y = rect.y + rect.height * 0.5;
    const surface = offset(rect.x - 3, y);
    let height = 0;
    for (let dy = 0; dy < Math.floor(rect.height); dy++) {
      const i = offset(x, rect.y + dy);
      if (
        [0, 1, 2].some(
          (k) => Math.abs(pixels[i + k]! - pixels[surface + k]!) > 8,
        )
      )
        height++;
    }
    samples.push({ green: pixels[offset(x, y) + 1]!, height });
    if (
      checkCorners &&
      pixels[offset(rect.x, rect.y - 1) + 1] !==
        pixels[offset(rect.x - 2, rect.y - 1) + 1]
    )
      throw new Error("Dropdown option painted a square corner");
    if (n + 1 < count)
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
  }
  return samples;
}
