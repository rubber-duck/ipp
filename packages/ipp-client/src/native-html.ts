import type { GuiClientInput } from "./gui-types.js";
import type { HtmlSource } from "./html-content.js";

/** Platform input sink for one acknowledged client-managed GUI target. */
export interface HtmlInputSink {
  input(input: GuiClientInput): void;
  release(): void;
}

/** A native Host adapter owns source registration and browser event delivery. */
export type NativeHtmlRegistration = (
  page: string,
  fail: (error: Error) => void,
  navigate: (url: string) => void,
) => Promise<HtmlSource>;

const NATIVE_HTML = Symbol.for("ipp.native-html");

export function bindNativeHtmlSources(
  host: object,
  register: NativeHtmlRegistration,
): void {
  Object.defineProperty(host, NATIVE_HTML, {
    value: register,
    configurable: true,
  });
}

export function nativeHtmlAvailable(host: object | undefined): boolean {
  return (
    !!host &&
    typeof (host as { [NATIVE_HTML]?: NativeHtmlRegistration })[NATIVE_HTML] ===
      "function"
  );
}

/** Opens a page within the native Host's explicitly configured local page root. */
export function registerNativeHtml(
  host: object,
  page: string,
  fail: (error: Error) => void,
  navigate: (url: string) => void,
): Promise<HtmlSource> {
  const register = (host as { [NATIVE_HTML]?: NativeHtmlRegistration })[
    NATIVE_HTML
  ];
  if (!register)
    return Promise.reject(new Error("Native CEF content is unavailable"));
  return register(page, fail, navigate);
}
