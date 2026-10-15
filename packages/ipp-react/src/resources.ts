import { createElement, type ReactNode } from "react";

export const RESOURCES_HOST_TYPE = "ipp-resources";

/** Declare resources in the current World without implicit visual parenting. */
export function Resources(props: { readonly children?: ReactNode }) {
  return createElement(RESOURCES_HOST_TYPE, props);
}
