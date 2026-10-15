import type { Client, HostClientBase, GuiTarget } from "@ipp/client";
import { Children, Entity } from "@ipp/react";
import {
  Box,
  Button,
  Checkbox,
  Font,
  Layout,
  ScrollView,
  Skin,
  SkinProvider,
  Slider,
  Style,
  TextInput,
} from "@ipp/react/gui";
import {
  type GuiKitContract,
  type GuiSkinDefinition,
} from "@ipp/react/gui-kit";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { glassSkin } from "@ipp/react/gui-glass";
import { ShaderAsset } from "../../../../packages/ipp-react/src/assets/declarations.js";
import { PaintShader } from "../../../../packages/ipp-react/src/shaders.js";
import { check } from "../../../harness/page/checks.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import { image } from "../../../../tools/shared-host/presentation.js";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";

const EXTENT = [560, 270] as const;
const brokenAssets = (scope: string) => (
  <Entity id={`${scope}/material`}>
    <ShaderAsset id={`${scope}/shader`} recipe={{}} parameters={{}}>
      <PaintShader>{"this is not a shader"}</PaintShader>
    </ShaderAsset>
  </Entity>
);

function sameTarget(a: GuiTarget, b: GuiTarget) {
  return (
    a.entity === b.entity &&
    a.component === b.component &&
    a.incarnation === b.incarnation &&
    a.world.id === b.world.id &&
    a.world.incarnation === b.world.incarnation
  );
}

function differences(a: RgbaImage, b: RgbaImage, rect: readonly number[]) {
  const scale = a.width / EXTENT[0];
  const [x, y, width, height] = rect.map((v) => Math.round(v * scale)) as [
    number,
    number,
    number,
    number,
  ];
  let changed = 0;
  for (let row = y; row < y + height; row++)
    for (let col = x; col < x + width; col++) {
      const offset = (row * a.width + col) * 4;
      if (
        [0, 1, 2].some(
          (c) => Math.abs(a.pixels[offset + c]! - b.pixels[offset + c]!) > 5,
        )
      )
        changed++;
    }
  return changed;
}

/** Both transports exercise actual skin declarations, native input, resource failure and frame pixels. */
export async function exerciseSkinOwnership(
  host: HostClientBase<Client>,
  contract: ThemeContract & GuiKitContract,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const cybertron = cybertronSkin(contract);
  const glass = glassSkin(contract, { reducedMotion: true });
  const broken: GuiSkinDefinition = { ...glass, assets: brokenAssets };
  const partial = contract.GuiSkin.encodeParts({
    nextSlot: 1,
    rows: new Map([
      [
        0,
        {
          part: contract.guiPaintPartIndex({ part: "background" }),
          color: [0.02, 0.04, 0.08, 1],
        },
      ],
    ]),
  });
  function specimen(skin: GuiSkinDefinition, override = false) {
    return defineSpecimen({
      extent: EXTENT,
      failingAssets: skin === broken ? ["selection/resources/1/shader"] : [],
      states: [{ name: "ownership", cell: [0, 0, ...EXTENT] }],
      render: (lab) => (
        <>
          <Entity id="background">
            <Layout width={560} height={270} align_x={-1} align_y={-1} />
            <Style red={0} green={0} blue={0} />
            <Box width={560} height={270} />
          </Entity>
          <SkinProvider
            id="selection"
            skin={skin}
            contract={contract}
            font={lab.font}
            fontSize={16}
            reducedMotion
          >
            <Entity id="content">
              <Layout
                kind={3}
                width={560}
                height={270}
                align_x={-1}
                align_y={-1}
              />
              <Font source={lab.font} font_size={16} />
              <Children>
                <Entity id="bare">
                  <Layout
                    width={120}
                    height={40}
                    margin_left={20}
                    margin_top={20}
                    align_x={-1}
                    align_y={-1}
                  />
                  <Button skin={false} label="BARE" ref={lab.control("bare")} />
                </Entity>
                <Entity id="partial">
                  <Layout
                    width={120}
                    height={40}
                    margin_left={180}
                    margin_top={20}
                    align_x={-1}
                    align_y={-1}
                  />
                  <Skin parts={partial} />
                  <Button label="HIDDEN" />
                </Entity>
                <Entity id="action">
                  <Layout
                    width={120}
                    height={40}
                    margin_left={20}
                    margin_top={100}
                    align_x={-1}
                    align_y={-1}
                  />
                  <Button label="ACTION" ref={lab.control("action")} />
                  {override && <Skin parts={partial} />}
                </Entity>
                <Entity id="editor">
                  <Layout
                    width={180}
                    height={40}
                    margin_left={180}
                    margin_top={100}
                    align_x={-1}
                    align_y={-1}
                  />
                  <TextInput text="Initial" ref={lab.control("editor")} />
                </Entity>
                <Entity id="switch">
                  <Layout
                    width={72}
                    height={32}
                    margin_left={400}
                    margin_top={104}
                    align_x={-1}
                    align_y={-1}
                  />
                  <Checkbox
                    skin="switch"
                    checked={false}
                    ref={lab.control("switch")}
                  />
                </Entity>
                <Entity id="slider">
                  <Layout
                    width={200}
                    height={32}
                    margin_left={20}
                    margin_top={190}
                    align_x={-1}
                    align_y={-1}
                  />
                  <Slider value={0.25} ref={lab.control("slider")} />
                </Entity>
                <Entity id="scroll">
                  <Layout
                    width={160}
                    height={66}
                    margin_left={300}
                    margin_top={175}
                    align_x={-1}
                    align_y={-1}
                  />
                  <ScrollView axis={1} ref={lab.control("scroll")} />
                  <Children>
                    <Entity id="scroll-content">
                      <Layout width={130} height={220} />
                      <Style red={0.05} green={0.08} blue={0.12} />
                      <Box width={130} height={220} />
                    </Entity>
                  </Children>
                </Entity>
              </Children>
            </Entity>
          </SkinProvider>
          <SkinProvider
            id="peer"
            skin={glass}
            contract={contract}
            font={lab.font}
            fontSize={16}
            reducedMotion
          >
            <Entity id="peer-button">
              <Font source={lab.font} font_size={16} />
              <Layout
                width={120}
                height={40}
                margin_left={400}
                margin_top={20}
                align_x={-1}
                align_y={-1}
              />
              <Button label="GLASS" ref={lab.control("peer")} />
            </Entity>
          </SkinProvider>
        </>
      ),
    });
  }
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: specimen(cybertron),
    themes: {},
    world: "client-skin-ownership",
  });
  const save = async (name: string, frame: RgbaImage) => {
    let text = "";
    for (let i = 0; i < frame.pixels.length; i += 0x8000)
      text += String.fromCharCode(...frame.pixels.subarray(i, i + 0x8000));
    await environment.capture({
      name,
      width: frame.width,
      height: frame.height,
      rgba: btoa(text),
    });
  };
  try {
    check(
      (await session.assetSettled("peer/shader")).status === "loaded",
      "Peer Glass resources failed",
    );
    await session.interact(present, async (interaction) => {
      const { client, control, input, capture } = interaction;
      const first = image(await capture());
      await save("ownership-cybertron-and-glass", first);
      const blank = { ...first, pixels: new Uint8Array(first.pixels.length) };
      check(
        differences(first, blank, [20, 20, 120, 40]) === 0,
        "Unskinned control emitted implicit parts",
      );
      const sample = (x: number, y: number) =>
        first.pixels[(Math.round(y * 2) * first.width + Math.round(x * 2)) * 4];
      check(
        sample(185, 30)! > 20 && sample(230, 40) === sample(185, 30),
        "Explicit background did not remain flat and label-free",
      );
      check(
        (await control("bare").action({ kind: "press" })).ok,
        "Unskinned control lost native semantics",
      );
      const names = ["action", "editor", "switch", "slider", "scroll", "peer"];
      const targets = names.map((name) => control(name).target);
      check(
        await control("switch").compareAndSet("checked", false, true),
        "Switch write failed",
      );
      check(
        await control("slider").compareAndSet("value", 0.25, 0.65),
        "Slider write failed",
      );
      check(
        (
          await control("scroll").action({
            kind: "scrollTo",
            offset: [0, 80],
          })
        ).ok,
        "Scroll action failed",
      );
      await input.send({
        kind: "pointerDown",
        pointer: 73n,
        point: [260 / 560, 120 / 270],
      });
      await input.send({
        kind: "pointerUp",
        pointer: 73n,
        point: [260 / 560, 120 / 270],
      });
      await capture();
      check(input.nativeText, "Editor did not take native focus");
      await input.editText(input.nativeText.fence, {
        kind: "selection",
        start: 0,
        end: 7,
      });
      await capture();
      await input.editText(input.nativeText!.fence, {
        kind: "text",
        text: "Retained",
      });
      await capture();
      await input.editText(input.nativeText!.fence, {
        kind: "selection",
        start: 1,
        end: 4,
      });
      const before = input.nativeText!;
      const values = await Promise.all(
        names.map((name) => control(name).read()),
      );
      await session.update({ specimen: specimen(glass) });
      check(
        (await session.assetSettled("selection/shader")).status === "loaded",
        "New Glass resources failed",
      );
      // Asset readiness, observer-triggered selection and rendered publication
      // are separate fences. A temporarily quiet frame can still show the old
      // skin; wait for the authored theme paint and its visible result together.
      const deadline = performance.now() + 30_000;
      let glassFrame = image(await capture());
      let selectionAttempts = 0;
      let paintAdmitted = false;
      let changedPixels = 0;
      for (;;) {
        const state = await client.inspect();
        const action = state.entities.find(
          (entity) => entity.id === control("action").target.entity,
        );
        const themeId = action?.components.find(
          (component) => component.component === client.components.GuiSkin!.id,
        )?.fields.theme;
        const theme = state.entities.find((entity) => entity.id === themeId);
        paintAdmitted = !!theme?.components.some(
          (component) =>
            component.component === client.components.CanvasPaint!.id,
        );
        changedPixels = differences(first, glassFrame, [20, 100, 120, 40]);
        if (
          (paintAdmitted && changedPixels > 500) ||
          performance.now() >= deadline
        ) {
          await environment.record({
            skinSelection: {
              selectionAttempts,
              paintAdmitted,
              changedPixels,
            },
            resources: state.resources.map((resource) => ({
              source: resource.source,
              status: resource.status,
              graphicsReady: resource.representation.graphicsReady,
            })),
            renderDiagnostics: state.renderDiagnostics.map((diagnostic) => ({
              entity: String(diagnostic.entity),
              reason: diagnostic.reason,
            })),
          });
          break;
        }
        selectionAttempts++;
        glassFrame = image(await capture());
      }
      await save("ownership-switched-to-glass", glassFrame);
      check(
        paintAdmitted && changedPixels > 500,
        `Skin switch did not change the button material: paint=${paintAdmitted}, changed=${changedPixels}, attempts=${selectionAttempts}`,
      );
      check(
        differences(first, glassFrame, [400, 20, 120, 40]) === 0,
        "Switching one provider changed its independent peer",
      );
      for (let i = 0; i < names.length; i++)
        check(
          sameTarget(targets[i]!, control(names[i]!).target),
          `Reskin replaced ${names[i]}`,
        );
      const after = input.nativeText;
      check(
        after &&
          sameTarget(after.fence.target, before.fence.target) &&
          after.text === before.text &&
          after.selectionStart === before.selectionStart &&
          after.selectionEnd === before.selectionEnd,
        "Reskin disturbed native focus or selection",
      );
      const fields = await Promise.all(
        names.map((name) => control(name).read()),
      );
      for (const [i, field] of [
        [1, "text"],
        [2, "checked"],
        [3, "value"],
        [4, "offset_y"],
      ] as const)
        check(
          fields[i]![field] === values[i]![field],
          `Reskin changed ${field}`,
        );
      await session.update({ specimen: specimen(broken) });
      check(
        (await session.assetSettled("selection/resources/1/shader")).status ===
          "failed",
        "Broken resource did not fail",
      );
      const failedFrame = image(await capture());
      check(
        differences(glassFrame, failedFrame, [0, 0, ...EXTENT]) === 0,
        "Failed skin replacement lost the last ready appearance",
      );
      // Drain the expected asynchronous asset error under the failing declaration.
      await session.update({ specimen: specimen(broken) });
      await session.update({ specimen: specimen(cybertron) });
      const restored = image(await capture());
      await save("ownership-restored-cybertron", restored);
      check(
        differences(glassFrame, restored, [20, 100, 120, 40]) > 500,
        "Cybertron restoration did not repaint",
      );
      for (let i = 0; i < names.length; i++)
        check(
          sameTarget(targets[i]!, control(names[i]!).target),
          `Restoration replaced ${names[i]}`,
        );
      // An explicit Skin added after the native declaration replaces its whole
      // appearance; removing it restores context without remounting the control.
      await session.update({ specimen: specimen(cybertron, true) });
      const overridden = image(await capture());
      check(
        differences(restored, overridden, [20, 100, 120, 40]) > 500,
        "Explicit skin did not replace automatic appearance",
      );
      const px = (x: number, y: number) =>
        overridden.pixels[(y * 2 * overridden.width + x * 2) * 4];
      check(
        px(25, 110) === px(75, 120),
        "Explicit partial skin retained inherited label/chrome",
      );
      await session.update({ specimen: specimen(cybertron) });
      const inherited = image(await capture());
      check(
        differences(restored, inherited, [20, 100, 120, 40]) === 0,
        "Removing the explicit skin did not restore context appearance",
      );
      check(
        sameTarget(targets[0]!, control("action").target),
        "Skin override changed the native control identity",
      );
      const narrowThumb: GuiSkinDefinition = {
        ...cybertron,
        themes: {
          ...cybertron.themes,
          slider: {
            ...cybertron.themes.slider!,
            metrics: {
              ...cybertron.themes.slider!.metrics,
              thumb_ratio: 0.4,
            },
          },
        },
      };
      await session.update({ specimen: specimen(narrowThumb) });
      await capture();
      // 32-high control: 12.8-wide thumb, its centre at 6.4 + 0.75 * 187.2.
      const point = [(20 + 6.4 + 0.75 * 187.2) / 560, 206 / 270] as const;
      await input.send({ kind: "pointerDown", pointer: 73n, point });
      await input.send({ kind: "pointerUp", pointer: 73n, point });
      await capture();
      check(
        Math.abs(Number((await control("slider").read()).value) - 0.75) < 0.011,
        "Authored thumb measurement disagrees with native pointer mapping",
      );
      await environment.record({
        bareNoPaint: true,
        automaticNativeSkins: true,
        explicitOverrideAndRestore: true,
        explicitPartOnly: true,
        independentSkins: true,
        stableControlIdentities: true,
        nativeSelectionRetained: true,
        valuesAndScrollRetained: true,
        failedSkinKeepsReadyAppearance: true,
        authoredMetricsDriveNativeHitGeometry: true,
      });
    });
  } finally {
    await session.close();
  }
}
