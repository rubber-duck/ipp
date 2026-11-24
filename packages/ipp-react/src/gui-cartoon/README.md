# Cartoon GUI skin

Cartoon gives the native GUI kit inked edges, opaque blueberry panels and two-tone gold buttons. Pressing a primary button flattens its lower edge. A selected button, segment, tab or step extends slightly beyond its rail. Dropdown options, multiple-choice lists, tree rows and grid rows use the same grass-green face and ink lettering, sized for their spacing. Each draws from four subtle tilts, each paired with a small independent slant on its lower stripe. Leaving an option picks a different tilt for its next selection. Each toast and alert picks a gentle tilt when it appears. The frame, lettering, icon and buttons rotate together around the centre of the card. Information and warning icons use a mint disk and an amber sign with inked punctuation. Progress bars and circular meters use a dark track and a two-tone green fill. Radio marks keep a circular ink rim and use the same green when selected. Tables use a shaded header and row spacing. Keyboard focus uses a gold ring.

The chart skin uses a blue engineering grid, cream axis labels and opaque legend cards. Spatial bars stack square-cornered crates with small individual turns, unequal plank widths and cleated lids. Only the end crate adjusts to the value, with fixed-height rails around its resizable side panel. Short ends read as a single board. Each stack uses its legend colour and ends at the measured value. A cream pencil stroke picks out the Cartesian axes. Planar bars and histogram bins stack square wooden crates with a small sideways offset per layer. The fitted end keeps its fixed-width rails, and very short values appear as one plank. Planar lines use the solid series paint. Height surfaces catch the light on individual facets. The gallery uses the skin’s optional paper paint for its Canvas sheet and base material for the tile under each spatial plot. These sheets supply their own printed grid. Spatial wall guides and axis labels remain visible.

The [KayKit Platformer](https://kaylousberg.com/game-assets/platformer), [Forest Nature](https://kaylousberg.com/game-assets/forest-nature-pack) and [Dungeon Remastered](https://kaylousberg.com/game-assets/dungeon-remastered) covers informed the silhouette, lettering weight and separation of light faces from dark edges. The skin uses original native paint recipes.

```tsx
import { cartoonSkin, CartoonTitle } from "@ipp/react/gui-cartoon";
import { GuiKit, Button, Column } from "@ipp/react/gui-kit";

const skin = cartoonSkin(contract);

<GuiKit id="menu" contract={contract} skin={skin} font={lilitaFont} iconFont={iconFont} fontSize={16}>
  <Column>
    <CartoonTitle text="TRAIL RUN" fontSize={48} />
    <Button id="continue" label="Continue" onPress={continueGame} />
  </Column>
</GuiKit>;
```

Use `CartoonTitle` for short display lettering with an ink outline and lower shadow. Supply a source for the built Lilita One font as `lilitaFont`. Its compact, soft-edged capitals suit headings and short commands. Noto Sans is available for longer text. Both follow the [shared font build](../../../../assets/fonts/README.md). The [gallery](../../../../examples/demos/README.md) lets you switch skin and typeface independently.
