import { useCallback, useEffect, useRef, useState } from "react";
import {
  htmlCanvasAvailable,
  registerHtml,
  type HtmlSource,
} from "@ipp/client";
import { BrowserNavigation } from "./browser-navigation.js";
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

const FLAG = "chrome://flags/#canvas-draw-element";
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
  const available = chrome && htmlCanvasAvailable();
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

  const navigate = (value: string) => {
    try {
      const input = value.trim();
      const url = new URL(
        PAGES.some((page) => page.file === input) ? ROOT + input : input,
        PAGES.some((page) => page.file === input)
          ? location.href
          : new URL(current, location.href),
      );
      if (
        url.origin !== location.origin ||
        url.search ||
        url.hash ||
        !PAGES.some(
          (page) =>
            url.pathname === new URL(ROOT + page.file, location.href).pathname,
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
      const status = shadow.querySelector("output");
      content.addEventListener(
        "input",
        (event) => {
          const element = event.composedPath()[0];
          if (
            status &&
            element instanceof HTMLInputElement &&
            element.type === "range"
          )
            status.textContent = `Power: ${element.value}%`;
        },
        { signal: abort.signal },
      );
      shadow.addEventListener("submit", (event) => {
        event.preventDefault();
        if (status && event.target instanceof HTMLFormElement) {
          const data = new FormData(event.target);
          status.style.background = "#16804b";
          status.textContent = `Saved: ${[...data.values()].join(" · ") || "ready"}`;
        }
      });
      shadow.addEventListener("reset", () => {
        if (status) status.textContent = "Form reset. Ready to try again.";
      });
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
  }, [available, current, history, host]);

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
      {!chrome ? (
        <TextLine
          id="cockpit-browser-status"
          text="Not supported outside of Chrome."
        />
      ) : !available ? (
        <Column gap="label">
          <TextLine
            id="cockpit-browser-status"
            text="Enable HTML-in-Canvas in Chrome to view these pages."
          />
          <TextLine text="1. Copy this address and paste it into a new Chrome tab." />
          <TextField id="cockpit-browser-flag" value={FLAG} />
          <Row>
            <Button
              id="cockpit-browser-copy"
              label="Copy flag address"
              onPress={() => {
                void (
                  navigator.clipboard?.writeText(FLAG) ??
                  Promise.reject(new Error("Clipboard unavailable"))
                ).then(
                  () => setCopied("Copied. Paste into Chrome's address bar."),
                  () =>
                    setCopied(
                      "Copy unavailable. Select the address above and copy it.",
                    ),
                );
              }}
            />
            <TextLine text={copied} tone="neutral" />
          </Row>
          <TextLine text="2. Set HTML-in-Canvas to Enabled, then relaunch Chrome." />
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
