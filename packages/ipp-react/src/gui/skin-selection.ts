import {
  createElement,
  Fragment,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
} from "react";
import { AttachmentContext } from "../composition/attached-world.js";
import type { GuiSkinDefinition } from "./skin-types.js";

type Selection = { readonly skin: GuiSkinDefinition; readonly scope: string };

/** Prepare a new immutable resource bundle while the previous skin stays visible. */
export function useSkinSelection(
  id: string,
  skin: GuiSkinDefinition | undefined,
) {
  const container = useContext(AttachmentContext);
  const scopes = useRef(
    new Map<NonNullable<GuiSkinDefinition["assets"]>, string>(),
  );
  const serial = useRef(0);
  const previous = useRef<Selection | undefined>(undefined);
  const requested = useMemo(() => {
    if (!skin) return undefined;
    let scope = id;
    if (skin.assets) {
      const known = scopes.current.get(skin.assets);
      if (known) scope = known;
      else {
        const index = serial.current++;
        scope = index === 0 ? id : `${id}/resources/${index}`;
      }
      scopes.current.set(skin.assets, scope);
      if (!skin.assetIds?.length)
        throw new Error(
          "A skin with assets must declare its required assetIds",
        );
    }
    return { skin, scope };
  }, [id, skin]);
  const ready = useSyncExternalStore(
    (changed) => container?.commits.assets.subscribe(changed) ?? (() => {}),
    () =>
      !requested?.skin.assets ||
      requested.skin.assetIds!.every(
        (name) =>
          container?.commits.assets.get(`${requested.scope}/${name}`)
            ?.status === "loaded",
      ),
  );
  const selected = ready ? requested : previous.current;
  useLayoutEffect(() => {
    previous.current = selected;
    for (const assets of scopes.current.keys()) {
      if (assets !== selected?.skin.assets && assets !== requested?.skin.assets)
        scopes.current.delete(assets);
    }
  }, [selected, requested]);
  const resources = new Map<string, Selection>();
  for (const entry of [selected, requested])
    if (entry?.skin.assets) resources.set(entry.scope, entry);
  return {
    selected,
    resources: [...resources.values()].map((entry) =>
      createElement(
        Fragment,
        { key: entry.scope },
        entry.skin.assets!(entry.scope),
      ),
    ),
  };
}
