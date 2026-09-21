import { useLayoutEffect, useState, type ReactNode } from "react";
import type { AssetWorldClient, AssetResourceSnapshot } from "@ipp/client";
import { Entity, MeshInstance, UnlitTexture } from "@ipp/react";
import { useIppCanvas } from "@ipp/react/web";

interface GeometrySources {
  readonly mesh: string;
  readonly texture: string | undefined;
}

/** Keep the displayed references while ordinary world demand loads an edit. */
export function ReadyGeometry({
  id,
  mesh,
  texture,
  children,
}: GeometrySources & {
  readonly id: string;
  readonly children: (sources: GeometrySources) => ReactNode;
}) {
  const canvas = useIppCanvas();
  if (!canvas) throw new Error("ReadyGeometry requires a connected World");
  const client = canvas.client as AssetWorldClient;
  // First attachment uses normal pending-resource semantics. Only replacements
  // need staging; there is no previous geometry to preserve on first mount.
  const [current, setCurrent] = useState<GeometrySources>({ mesh, texture });
  const [loaded, setLoaded] = useState<GeometrySources>();
  const [error, setError] = useState<Error>();
  const replacing = current.mesh !== mesh || current.texture !== texture;

  // A readiness update can be queued alongside a newer props update. Accept it
  // only while it still names the requested pair, never an obsolete edit.
  if (replacing && loaded?.mesh === mesh && loaded.texture === texture) {
    setCurrent(loaded);
  }

  useLayoutEffect(() => {
    if (!replacing) return;
    let active = true;
    const observed = new Map<number, AssetResourceSnapshot>();
    const matches = (resource: AssetResourceSnapshot) =>
      resource.variant === 0 &&
      ((resource.kind === 1 && resource.source === mesh) ||
        (resource.kind === 2 && resource.source === texture));
    const publish = () => {
      if (!active) return;
      if (
        observed.get(1)?.status === "loaded" &&
        (texture === undefined || observed.get(2)?.status === "loaded")
      ) {
        setLoaded({ mesh, texture });
      } else {
        setLoaded(undefined);
      }
    };
    const unsubscribe = client.onResourceChange((resource) => {
      if (!active || !matches(resource)) return;
      observed.set(resource.kind, resource);
      publish();
    });
    // Subscribe before inspection so already-loaded shared resources and events
    // arriving during the request both work. Newer events win over the snapshot.
    void client.inspect().then(
      (inspection) => {
        if (!active) return;
        for (const resource of inspection.resources) {
          if (matches(resource) && !observed.has(resource.kind)) {
            observed.set(resource.kind, resource);
          }
        }
        publish();
      },
      (failure: unknown) => {
        if (active)
          setError(
            failure instanceof Error ? failure : new Error(String(failure)),
          );
      },
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [client, mesh, texture, replacing]);

  if (error) throw error;
  return (
    <>
      {children(current)}
      {replacing && (
        <Entity id={id}>
          {/* No transform/material: demand loads resources without an extra draw. */}
          <MeshInstance source={mesh} />
          {texture !== undefined && <UnlitTexture source={texture} />}
        </Entity>
      )}
    </>
  );
}
