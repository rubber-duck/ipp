import type { OutputReference } from "@ipp/client";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { IppCanvasHandle } from "@ipp/react/canvas";
import { PlatformerSession, type PlatformerPlayback } from "./session.js";

import type { GalleryAssets } from "../../shared/scene.js";

export function usePlatformerScene(
  canvas: IppCanvasHandle | undefined,
  active: boolean,
  assets: GalleryAssets,
  output: OutputReference,
) {
  const scope = useMemo(() => ({ canvas, active }), [canvas, active]);
  const [committed, setCommitted] = useState<typeof scope>();
  const onCommit = useCallback(() => setCommitted(scope), [scope]);
  const [attachment, setAttachment] = useState<{
    scope: typeof scope;
    session: PlatformerSession;
  }>();
  const session = attachment?.scope === scope ? attachment.session : undefined;
  const [playback, setPlayback] = useState<PlatformerPlayback>({
    mode: "walk",
    direction: 1,
    playing: true,
  });
  const [error, setError] = useState<string>();
  const serial = useRef(Promise.resolve());
  useEffect(() => {
    let disposed = false;
    const startup = new AbortController();
    let owned: PlatformerSession | undefined;
    setAttachment(undefined);
    setError(undefined);
    if (!canvas || !active || committed !== scope) return;
    const report = (failure: unknown) => {
      if (!disposed)
        setError(failure instanceof Error ? failure.message : String(failure));
    };
    serial.current = serial.current
      .then(async () => {
        if (disposed) return;
        owned = await PlatformerSession.create(
          canvas,
          assets,
          output,
          startup.signal,
          (state) => {
            if (!disposed) setPlayback(state);
          },
        );
        if (!disposed) setAttachment({ scope, session: owned });
      })
      .catch(report);
    return () => {
      disposed = true;
      startup.abort();
      serial.current = serial.current
        .then(async () => owned?.close())
        .catch(report);
    };
  }, [canvas, active, committed, scope, assets, output]);
  return { session, playback, error, setError, onCommit };
}

export type PlatformerDemo = ReturnType<typeof usePlatformerScene>;
