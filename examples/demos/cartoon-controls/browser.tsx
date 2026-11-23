import { useEffect, useState, useSyncExternalStore } from "react";
import { createRoot } from "react-dom/client";
import { browserRuntime, canvasOutput } from "@ipp/client";
import { IppCanvas, World, type IppCanvasHandle } from "@ipp/react/web";
import { GuiKit, type GuiKitContract } from "@ipp/react/gui-kit";
import { cartoonSkin } from "@ipp/react/gui-cartoon";
import { demoFont, DEMO_ICON_FONT } from "../shared/fonts.js";
import { GALLERY_RUNTIME, GALLERY_WHEEL_STEP } from "../shared/runtime.js";
import { Showcase, sections, type Section } from "./showcase.js";
import type { Settings } from "./inputs.js";

const runtime = new URL(GALLERY_RUNTIME, location.href);
const contract: GuiKitContract = await import(
  new URL("generated.js", runtime).href
);
const skin = cartoonSkin(contract);
const font = new URL(demoFont("lilita-one").path, location.href).href;
const iconFont = new URL(DEMO_ICON_FONT, location.href).href;
const selectedSystems = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
];
const section = (): Section =>
  sections.find((entry) => entry.key === location.hash.slice(1))?.key ??
  "settings";
function subscribe(changed: () => void) {
  window.addEventListener("hashchange", changed);
  return () => window.removeEventListener("hashchange", changed);
}
const dimensions = () =>
  [
    Math.max(320, Math.min(innerWidth, 920)),
    Math.max(320, Math.min(innerHeight, 680)),
  ] as const;

function readSettings(): Settings {
  try {
    const saved = JSON.parse(
      localStorage.getItem("ipp-cartoon-preview") ?? "null",
    );
    if (
      saved &&
      ["easy", "normal", "hard"].includes(saved.difficulty) &&
      typeof saved.music === "boolean"
    )
      return { difficulty: saved.difficulty, music: saved.music };
  } catch {}
  return { difficulty: "normal", music: true };
}

function Preview() {
  const [[width, height], resize] = useState(dimensions);
  const [canvas, ready] = useState<IppCanvasHandle>();
  const [saved, setSaved] = useState(readSettings);
  const [failure, fail] = useState("");
  const page = useSyncExternalStore(subscribe, section);
  useEffect(() => {
    window.ippDemoCanvas = canvas;
    return () => {
      delete window.ippDemoCanvas;
    };
  }, [canvas]);
  useEffect(() => {
    const update = () => resize(dimensions());
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);
  return (
    <>
      <IppCanvas
        id="cartoon-stage"
        width={width}
        height={height}
        runtime={browserRuntime(runtime)}
        world={{
          create: {
            selectedSystems,
            canvas: { extent: [width, height], unitsPerMetre: 1 },
          },
        }}
        output={
          canvas?.client.worldReference
            ? canvasOutput(canvas.client.worldReference)
            : null
        }
        guiInput={{ awaitPointerDown: true, wheelStep: GALLERY_WHEEL_STEP }}
        onReady={ready}
        onError={(e) => fail(e.message)}
      >
        {canvas && (
          <World onError={(e) => fail(e.message)}>
            <GuiKit
              id="cartoon-gallery"
              skin={skin}
              contract={contract}
              font={font}
              iconFont={iconFont}
              fontSize={Math.min(
                17,
                Math.max(11, Math.floor((width - 40) / 25)),
              )}
            >
              <Showcase
                width={width}
                height={height}
                section={page}
                select={(value) => {
                  location.hash = value;
                }}
                saved={saved}
                save={(value) => {
                  setSaved(value);
                  try {
                    localStorage.setItem(
                      "ipp-cartoon-preview",
                      JSON.stringify(value),
                    );
                  } catch {}
                }}
              />
            </GuiKit>
          </World>
        )}
      </IppCanvas>
      {failure && <output role="alert">{failure}</output>}
    </>
  );
}
createRoot(document.getElementById("app")!).render(<Preview />);
