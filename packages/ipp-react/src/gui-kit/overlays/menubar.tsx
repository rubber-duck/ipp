import { useLayoutEffect } from "react";
import { useEntityId } from "../../scope/identity.js";
import { Button } from "../button.js";
import { Toolbar, type ToolbarProps } from "../layout.js";
import { useApplicationValue } from "../selection/application-value.js";
import { Menu, type MenuItem } from "./menu.js";
import { useField } from "../../scope/field.js";
import { Floating, useOverlayOpen } from "./overlay.js";

export interface MenubarMenu {
  readonly key: string;
  readonly label: string;
  readonly disabled?: boolean;
  readonly items: readonly MenuItem[];
}

export interface MenubarProps
  extends Omit<ToolbarProps, "children" | "activateOnCrossAxis"> {
  readonly menus: readonly MenubarMenu[];
  readonly openMenu?: string | null;
  readonly defaultOpenMenu?: string;
  readonly onOpenMenuChange?: (key: string | null) => void;
  readonly onCommand?: (menu: string, command: string) => void;
}

/** Horizontal commands retain native focus on their triggers. A vertical
 * menu below the trigger handles its own axis, Enter and Escape. */
export function Menubar({
  id: supplied,
  menus,
  openMenu,
  defaultOpenMenu,
  onOpenMenuChange,
  onCommand,
  ...layout
}: MenubarProps) {
  const id = useEntityId(supplied);
  const active = useApplicationValue(
    openMenu,
    defaultOpenMenu ?? null,
    onOpenMenuChange,
  );
  const live = menus.find(
    (menu) => menu.key === active.current && !menu.disabled,
  );
  useLayoutEffect(() => {
    if (!live && active.current !== null) active.change(() => null);
  }, [live, active.current]);
  return (
    <Toolbar {...layout} id={id} measureBounds activateOnCrossAxis>
      {menus.map((menu) => {
        const open = live?.key === menu.key;
        const switchOpen = () =>
          active.change((previous) =>
            previous === null || menu.disabled ? previous : menu.key,
          );
        const close = () =>
          active.change((previous) =>
            previous === menu.key ? null : previous,
          );
        return (
          <MenuTrigger
            key={menu.key}
            toolbar={id}
            menu={menu}
            open={open}
            controlled={openMenu !== undefined}
            vertical={layout.orientation === "vertical"}
            switchOpen={switchOpen}
            show={() => active.change(() => menu.key)}
            close={close}
            onCommand={(key) => onCommand?.(menu.key, key)}
          />
        );
      })}
    </Toolbar>
  );
}

interface MenuTriggerProps {
  readonly toolbar: string;
  readonly menu: MenubarMenu;
  readonly open: boolean;
  readonly controlled: boolean;
  readonly vertical: boolean;
  readonly switchOpen: () => void;
  readonly show: () => void;
  readonly close: () => void;
  readonly onCommand: (key: string) => void;
}

/** Both the trigger and the popup are children of the toolbar. Native light
 * dismissal therefore allows focus and pointer movement among sibling triggers. */
function MenuTrigger({
  toolbar,
  menu,
  open,
  controlled,
  vertical,
  switchOpen,
  show,
  close,
  onCommand,
}: MenuTriggerProps) {
  const id = toolbar + "/" + menu.key;
  const axis = vertical ? "y" : "x";
  const trigger = useField(id, "CanvasBounds", axis);
  const group = useField(toolbar, "CanvasBounds", axis);
  const scale = useField(
    toolbar,
    "CanvasBounds",
    vertical ? "scale_y" : "scale_x",
  );
  const offset =
    typeof trigger.value === "number" &&
    typeof group.value === "number" &&
    typeof scale.value === "number" &&
    scale.value !== 0
      ? (trigger.value - group.value) / scale.value
      : 0;
  const overlay = useOverlayOpen({
    open,
    onOpenChange: (next) => {
      if (next) show();
      else close();
    },
  });
  return (
    <>
      <Button
        id={id}
        label={menu.label}
        variant="secondary"
        selected={open}
        disabled={menu.disabled ?? false}
        onPress={() => overlay.toggle()}
        onFocusChange={(event) => {
          if (event.focused) switchOpen();
        }}
        onInteractionChange={(event) => {
          if (event.hovered && !event.pressed) switchOpen();
        }}
      />
      <Floating
        id={id + "/popup"}
        mode="light"
        open={open}
        controlled={controlled}
        side={vertical ? "right" : "bottom"}
        offset={vertical ? [0, offset] : [offset, 0]}
        onVisibleChange={overlay.onVisibleChange}
        layout={{ fit_width: true, fit_height: true }}
      >
        <Menu
          id={id + "/commands"}
          items={menu.items}
          overlay={overlay}
          onSelect={(key) => {
            if (menu.items.some((item) => item.key === key && !item.disabled))
              onCommand(key);
          }}
        />
      </Floating>
    </>
  );
}
