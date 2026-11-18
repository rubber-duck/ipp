import { useEntityId } from "../../scope/identity.js";
import { Menubar, type MenubarProps } from "./menubar.js";

export interface NavigationDestination {
  /** Stable application-owned route identity, independent of its display name. */
  readonly id: string;
  readonly label: string;
  readonly disabled?: boolean;
}

export interface NavigationSection {
  readonly id: string;
  readonly label: string;
  readonly disabled?: boolean;
  readonly destinations: readonly NavigationDestination[];
}

export interface NavigationMenuProps
  extends Omit<MenubarProps, "menus" | "onCommand"> {
  readonly sections: readonly NavigationSection[];
  readonly activeDestination?: string;
  readonly onNavigate: (destination: NavigationDestination) => void;
}

/** Native destinations. The application decides whether to route, open a URL or replace content. */
export function NavigationMenu({
  id: supplied,
  sections,
  activeDestination,
  onNavigate,
  ...props
}: NavigationMenuProps) {
  const id = useEntityId(supplied);
  const menus = sections.map((section) => ({
    key: section.id,
    label: section.label,
    disabled: section.disabled ?? false,
    items: section.destinations.map((destination) => ({
      key: destination.id,
      label: destination.label,
      disabled: destination.disabled ?? false,
      selected: destination.id === activeDestination,
    })),
  }));
  return (
    <Menubar
      {...props}
      id={id}
      menus={menus}
      onCommand={(group, key) => {
        const section = sections.find(
          (item) => item.id === group && !item.disabled,
        );
        const destination = section?.destinations.find(
          (item) => item.id === key && !item.disabled,
        );
        if (destination) onNavigate(destination);
      }}
    />
  );
}
