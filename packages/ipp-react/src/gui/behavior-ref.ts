/** Internal field-write lease for complete overlays; absent from the public GUI entry. */
import { createElement } from "react";
import { componentContract } from "../components.js";
import type { BehaviorProps } from "./components.js";
import type { GuiControlRef } from "./control-ref.js";

export function BehaviorWithRef(
  props: BehaviorProps & { readonly controlRef?: GuiControlRef },
) {
  return createElement(componentContract.GuiBehavior.host, props);
}
