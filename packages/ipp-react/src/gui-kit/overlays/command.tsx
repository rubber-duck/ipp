import { useLayoutEffect, useRef, useState } from "react";
import { useEntityId } from "../../scope/identity.js";
import { Button } from "../button.js";
import { TextField } from "../text-field.js";
import { OptionList, type SelectOption } from "../selection/option-list.js";
import { Floating, useOverlayOpen, type OverlayOpenProps } from "./overlay.js";
import type { GuiControlHandle } from "../../gui/control-ref.js";
import { Resources } from "../../resources.js";
import { useGuiKit } from "../kit.js";

export interface CommandItem extends SelectOption {
  readonly children?: readonly CommandItem[];
  readonly keywords?: readonly string[];
}

export interface CommandProps extends OverlayOpenProps {
  readonly id?: string;
  readonly label?: string;
  readonly items: readonly CommandItem[];
  readonly onCommand: (key: string, path: readonly string[]) => void;
  readonly loading?: string;
  readonly onQueryChange?: (query: string, path: readonly string[]) => void;
}

export function Command({
  id: supplied,
  label = "Commands",
  items,
  onCommand,
  loading,
  onQueryChange,
  ...open
}: CommandProps) {
  const id = useEntityId(supplied);
  const kit = useGuiKit();
  const overlay = useOverlayOpen(open);
  const [path, setPath] = useState<readonly string[]>([]);
  const [query, setQuery] = useState("");
  const field = useRef<GuiControlHandle | null>(null);
  let choices = items;
  const livePath: string[] = [];
  for (const key of path) {
    const page = choices.find((item) => item.key === key && !item.disabled);
    if (!page?.children) break;
    choices = page.children;
    livePath.push(key);
  }
  useLayoutEffect(() => {
    if (!overlay.open && (path.length || query)) {
      setPath([]);
      setQuery("");
    } else if (livePath.length < path.length) {
      setPath(path.slice(0, livePath.length));
      setQuery("");
    }
  }, [overlay.open, livePath.length, path, query]);
  const needle = query.trim().toLocaleLowerCase();
  const candidates = choices.filter((item) =>
    [item.label, ...(item.keywords ?? [])].some((term) =>
      term.toLocaleLowerCase().includes(needle),
    ),
  );
  const pageId = livePath.length
    ? id + "/pages/" + livePath.map(encodeURIComponent).join("/")
    : id;
  const page = (next: readonly string[]) => {
    setPath(next);
    setQuery("");
    onQueryChange?.("", next);
    void field.current?.focus();
  };
  const choose = (key: string) => {
    if (!overlay.open) return;
    const item = candidates.find((row) => row.key === key && !row.disabled);
    if (!item) return;
    if (item.children) page([...livePath, key]);
    else {
      overlay.setOpen(false);
      onCommand(key, livePath);
    }
  };
  return (
    <>
      <Button
        id={`${id}/trigger`}
        label={label}
        onPress={() => overlay.setOpen(true)}
      />
      <Resources>
        <Floating
          id={id}
          mode="modal"
          side="centre"
          align="centre"
          band="dialog"
          open={overlay.open}
          controlled={overlay.controlled}
          onVisibleChange={overlay.onVisibleChange}
          layout={{ width: kit.unit(368) }}
        >
          <TextField
            id={`${id}/search`}
            label={label}
            value={query}
            onChange={(text) => {
              setQuery(text);
              onQueryChange?.(text, livePath);
            }}
            ref={field}
          />
          <OptionList
            key={JSON.stringify(livePath)}
            id={`${pageId}/choices`}
            options={candidates}
            onPick={choose}
            emptyLabel="No commands"
            {...(loading === undefined ? {} : { loading })}
          />
          {livePath.length > 0 && (
            <Button
              id={`${id}/back`}
              label="Back"
              onPress={() => page(livePath.slice(0, -1))}
            />
          )}
        </Floating>
      </Resources>
    </>
  );
}
