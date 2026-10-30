import { useCallback, useEffect, useRef, useState } from "react";
import {
  htmlCanvasAvailable,
  registerHtml,
  nativeHtmlAvailable,
  registerNativeHtml,
  type HtmlSource,
} from "@ipp/client";
import { BrowserNavigation } from "./browser-navigation.js";
import { installPageBehavior } from "./browser-pages/behavior.js";
import { BrowserPage } from "./browser-page.js";
import {
  Button,
  Column,
  Row,
  TextField,
  TextLine,
  Toolbar,
  useGuiKit,
} from "@ipp/react/gui-kit";

const FLAGS = [
  {
    id: "flag",
    copy: "copy",
    label: "HTML-in-Canvas",
    address: "chrome://flags/#canvas-draw-element",
  },
  {
    id: "experimental-flag",
    copy: "copy-experimental",
    label: "Experimental Web Platform features",
    address: "chrome://flags/#enable-experimental-web-platform-features",
  },
] as const;
const PAGES = [
  { name: "Welcome", file: "welcome.html" },
  { name: "Mission", file: "mission.html" },
  { name: "Crew", file: "crew.html" },
] as const;
const ROOT = "/examples/demos/scenes/cockpit/browser-pages/";

function chromeBrowser(): boolean {
  if (typeof navigator === "undefined") return false;
  return (
    /Chrome\//.test(navigator.userAgent) &&
    !/Edg\/|OPR\/|SamsungBrowser\/|Electron\//.test(navigator.userAgent) &&
    navigator.vendor === "Google Inc."
  );
}

/** The browser chrome is IPP GUI; the page itself is a live HTML capture. */
export function BrowserScreen({
  width,
  height,
  host,
}: {
  width: number;
  height: number;
  host?: object | undefined;
}) {
  const kit = useGuiKit();
  const initialPage =
    typeof location === "undefined"
      ? ROOT + PAGES[0].file
      : new URL(ROOT + PAGES[0].file, location.href).pathname;
  const [history, setHistory] = useState({
    entries: [initialPage],
    index: 0,
  });
  const [address, setAddress] = useState(initialPage);
  const [loaded, setLoaded] = useState<{
    navigation: typeof history;
    host: object;
    source: HtmlSource;
  }>();
  const [error, setError] = useState("");
  const [copied, setCopied] = useState("");
  const chrome = chromeBrowser();
  const native = nativeHtmlAvailable(host);
  const available = native || (chrome && htmlCanvasAvailable());
  const current = history.entries[history.index]!;
  // A navigation commit must stop presenting its previous source before the
  // effect releases it. A later Back creates a new history snapshot as well.
  const registration =
    available && loaded?.navigation === history && loaded.host === host
      ? loaded.source
      : undefined;
  const report = useCallback((failure: Error) => setError(failure.message), []);
  const toolbarHeight = kit.unit(
    Math.max(kit.tokens.controlHeight, kit.tokens.row),
  );
  const pageHeight = Math.max(1, height - toolbarHeight - kit.spacing.label);

  const pageBase =
    typeof location === "undefined" ? "https://ipp.invalid/" : location.href;
  const pageOrigin = new URL(pageBase).origin;
  const navigate = (value: string) => {
    try {
      const input = value.trim();
      const url = new URL(
        PAGES.some((page) => page.file === input) ? ROOT + input : input,
        PAGES.some((page) => page.file === input)
          ? pageBase
          : new URL(current, pageBase),
      );
      if (
        url.origin !== pageOrigin ||
        url.search ||
        url.hash ||
        !PAGES.some(
          (page) =>
            url.pathname === new URL(ROOT + page.file, pageBase).pathname,
        )
      )
        throw new Error(
          "This demo opens the bundled welcome.html, mission.html and crew.html pages.",
        );
      setError("");
      setAddress(url.pathname);
      setHistory((previous) => {
        if (previous.entries[previous.index] === url.pathname) return previous;
        const entries = [
          ...previous.entries.slice(0, previous.index + 1).slice(-49),
          url.pathname,
        ];
        return { entries, index: entries.length - 1 };
      });
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };

  const navigateRef = useRef(navigate);
  navigateRef.current = navigate;
  const traverse = (delta: number) => {
    const index = history.index + delta;
    setHistory({ ...history, index });
    setAddress(history.entries[index]!);
    setError("");
  };

  useEffect(() => {
    if (!available || !current || !host) return;
    const abort = new AbortController();
    let registration: HtmlSource | undefined;
    void (async () => {
      if (native) {
        registration = await registerNativeHtml(
          host,
          current.slice(ROOT.length),
          report,
          (url) => {
            if (abort.signal.aborted) return;
            const parsed = new URL(url);
            navigateRef.current(
              parsed.protocol === "file:"
                ? decodeURIComponent(parsed.pathname.split("/").at(-1)!)
                : url,
            );
          },
        );
        if (abort.signal.aborted) registration.release();
        else setLoaded({ navigation: history, host, source: registration });
        return;
      }
      const response = await fetch(current, { signal: abort.signal });
      if (!response.ok)
        throw new Error(`Page unavailable (${response.status})`);
      const document = new DOMParser().parseFromString(
        await response.text(),
        "text/html",
      );
      abort.signal.throwIfAborted();
      const content = window.document.createElement("div");
      const shadow = content.attachShadow({ mode: "open" });
      for (const node of [
        ...document.head.querySelectorAll("style"),
        ...document.body.childNodes,
      ])
        shadow.append(window.document.importNode(node, true));
      content.addEventListener(
        "click",
        (event) => {
          const element = event.composedPath()[0];
          const anchor =
            element instanceof Element ? element.closest("a[href]") : null;
          if (!anchor) return;
          event.preventDefault();
          navigateRef.current(anchor.getAttribute("href")!);
        },
        { signal: abort.signal },
      );
      installPageBehavior(shadow, abort.signal);
      await window.document.fonts.ready;
      abort.signal.throwIfAborted();
      registration = await registerHtml(host, content, (failure) => {
        if (!abort.signal.aborted) setError(failure.message);
      });
      if (abort.signal.aborted) registration.release();
      else setLoaded({ navigation: history, host, source: registration });
    })().catch((failure) => {
      if (!abort.signal.aborted) setError(String(failure));
    });
    return () => {
      abort.abort();
      registration?.release();
    };
  }, [available, current, history, host, native, report]);

  return (
    <Column width={width} height={height} fitHeight={false} gap="label">
      <Toolbar height={toolbarHeight}>
        <BrowserNavigation
          back={available && history.index > 0 ? () => traverse(-1) : undefined}
          forward={
            available && history.index < history.entries.length - 1
              ? () => traverse(1)
              : undefined
          }
        />
        <TextField
          id="cockpit-browser-address"
          value={address}
          onChange={setAddress}
          onSubmit={(event) => navigate(event.value)}
          disabled={!available}
          flex={1}
        />
      </Toolbar>
      {!chrome && !native ? (
        <TextLine
          id="cockpit-browser-status"
          text="Not supported outside of Chrome."
        />
      ) : !available ? (
        <Column gap="label">
          <TextLine
            id="cockpit-browser-status"
            text="Enable both Chrome flags to view and interact with these pages."
          />
          <TextLine text="1. Paste each address into a new Chrome tab." />
          {FLAGS.map((flag) => (
            <Column key={flag.id} gap="label">
              <TextLine text={flag.label} />
              <Row>
                <TextField
                  id={`cockpit-browser-${flag.id}`}
                  value={flag.address}
                  flex={1}
                />
                <Button
                  id={`cockpit-browser-${flag.copy}`}
                  label="Copy"
                  onPress={() => {
                    void (
                      navigator.clipboard?.writeText(flag.address) ??
                      Promise.reject(new Error("Clipboard unavailable"))
                    ).then(
                      () =>
                        setCopied("Copied. Paste into Chrome's address bar."),
                      () =>
                        setCopied(
                          "Copy unavailable. Select the address and copy it.",
                        ),
                    );
                  }}
                />
              </Row>
            </Column>
          ))}
          <TextLine text={copied} tone="neutral" />
          <TextLine text="2. Set both flags to Enabled, then relaunch Chrome." />
          <TextLine text="3. Return here and open the browser pane." />
          <TextLine
            text="If the flag is missing, update Chrome to a version with HTML-in-Canvas."
            tone="neutral"
          />
        </Column>
      ) : (
        <BrowserPage
          registration={error ? undefined : registration}
          status={error || "Loading page…"}
          width={width}
          height={pageHeight}
          onError={report}
        />
      )}
    </Column>
  );
}
