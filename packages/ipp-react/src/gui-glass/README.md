# Glass GUI skin

Glass is a client-owned skin for native GUI controls and charts. It shares the same provider and runtime contracts as Cybertron; switching appearance preserves control identity, editing, focus and data. See the [GUI architecture](../../../../docs/architecture/gui.md#client-skin-ownership) and [visual language](DESIGN.md).

## Authoring

Create a skin with the receiving Host’s generated contract, then supply it through `SkinProvider` in each World that needs it. Native controls and kit components resolve their appearance from that context.

```tsx
import { SkinProvider } from "@ipp/react/gui";
import { Button, Panel } from "@ipp/react/gui-kit";
import { glassSkin } from "@ipp/react/gui-glass";

const skin = glassSkin(contract, {
  palette: { primary: "#CF9CFF", secondary: "#785CFF" },
});

<SkinProvider contract={contract} skin={skin} font={font}>
  <Panel id="settings">
    <Button label="Save" onPress={save} />
  </Panel>
</SkinProvider>;
```

[`glassSkin`](kit.tsx) owns the public factory options. [`GlassGuiKit`](kit.tsx) is a convenience provider; [`GlassSkin`](skin.tsx) and the explicit [control wrappers](actions.tsx) support direct material studies. The [gallery](../../../../examples/demos/README.md) shows application-owned skin selection.

## Material and motion

Appearance is parametric. Apps choose palette, optical finish and an authored light, or explicitly opt into scene lighting. The material responds to the presenting view; apps can animate light properties on the Host clock or map cursor input to them. Reduced motion remains an application preference.

The material simulates glass through local shading and alpha composition, without scene reflections or a backdrop blur pass. Connected controls share a surface instead of stacking individual beveled tiles. Native geometry, interaction and exact color-field values remain authoritative.

Panel backgrounds use a flat dark palette colour at `panelOpacity` (0.98 by default), keeping busy scenes quiet. Controls retain their glass reflections and polished edges.

Liquid glass is the default across the GUI kit: smoky capsule controls, softly rounded popup sheets and polished dial/progress tubes. Large panel backings stay flat and quiet. Native state, connected surfaces and exact color fields remain shared; `appearance: "optical"` selects the earlier study. Chart data materials retain their separate geometry-aware shaders.

Set `reflectionStrength` to 0 to compare controls without the soft face reflections; the default is 1. Curved edges, Fresnel shading and direct highlights remain.

The [public types](types.ts), [skin options](skin.tsx) and [material source](material.ts) define parameters and defaults. Renderer ownership is specified by [presentation-aware paints](../../../../docs/architecture/rendering.md#presentation-aware-paints).

## Native charts

The same provider styles native 2D/3D charts, callouts and legends. Camera-only Worlds can use [`PlotSkinProvider`](../plots/skin-provider.tsx). Explicit appearance overrides inherited styling; `skin={false}` disables inheritance entirely. Data, geometry and picking remain native.

See [chart authoring](../plots/README.md), the [explicit chart finishes](chart.tsx), and the [maintained rendering suite](../../../../tests/gui/glass-skin/README.md) for usage and validation entry points.
