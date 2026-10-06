# Skin lab

A look-and-compare loop for GUI skins on the [shared development Host](../../../docs/development/shared-host.md). [lab.tsx](lab.tsx) is a client module of that Host: it opens specimens, each a row of controls in named interaction states, reloads their theme rows before every capture and writes each capture beside the matching [reference crop](#reference-crops). The shared-Host guide owns starting the Host, sessions, concurrency and the sharing rules; this page covers only the skin layer.

Specimens are [ordinary declarations](specimen.ts) independent of Node and of the Host tool, so a later maintained scenario can capture them too.

## Commands

```sh
# Is the shared Host running? Its owner starts it; see the guide.
node tools/shared-host/shared-host.mjs host status

# Open a session for a specimen; the name must be unique on the Host.
node tools/shared-host/shared-host.mjs session start tests/gui/skin-lab/lab.tsx a01-button --name button

# Edit tests/gui/skin-lab/themes/button.ts, then capture. The theme and
# specimen modules are reloaded before every capture.
node tools/shared-host/shared-host.mjs session capture button
node tools/shared-host/shared-host.mjs session capture button --states hover --zoom hover

# Done for now.
node tools/shared-host/shared-host.mjs session stop button

# One-shot: connect, declare, capture and disconnect, any number of specimens.
node tools/shared-host/shared-host.mjs run tests/gui/skin-lab/lab.tsx a01-button a02-checkbox --out /tmp/compare
```

`session start` and `run` take specimen names, the file names in `specimens/`. Capture options: `--states A,B` captures only those states; `--zoom A,B` or `--zoom all` adds enlarged pairs (`--zoom-factor N`, default 4); `--raw` adds each state's whole capture; `--references DIR` replaces the [reference crops](#reference-crops) directory. A session with several specimens captures the named ones, or all of them.

Files, under `target/shared-host-captures/<name>/` unless `--out DIR` is given:

| File | Content |
| --- | --- |
| `<specimen>.png` | The specimen canvas; each state's cell comes from that state's own capture |
| `<specimen>.compare.png` | The reference crop region on the left, the capture on the right, same scale |
| `<specimen>.<state>.zoom.png` | One state cell, reference and capture, enlarged |
| `<specimen>.<state>.raw.png` | The whole capture a state was taken from |

The printed summary says whether the theme or specimen module was reloaded and which commands that wrote (a row edit is one `setField` on the existing theme entity), and for each pinned state the routing outcome of every input step plus the focus and pointer feedback observed before the capture, for example `pointer=button-hover(hovered)`. Use it to tell an unpinned state from a skin that does not yet style it. `left after release` reports feedback that outlived a state's release; it should never appear.

## Writing a specimen

A specimen is `specimens/<name>.tsx` whose default export is `defineSpecimen({...})`, with its themes in `themes/<theme>.ts`. Copy the nearest existing specimen.

- **Extent and scale.** The canvas extent is in logical units, one unit per pixel of the unscaled reference sheet, captured at device scale 2 like the row crops. Keep it at most 1024 units wide. `reference.origin` is the crop pixel at the canvas origin; place controls where the sheet has them so the side-by-side and zooms line up. Sheets b and c draw their sections at other scales: their specimens lay out at the crop's own scale, take the factor from [scale.ts](scale.ts), multiply every length they add by it and pass theme rows through `scaledTheme`, so skin lengths stay in controls-sheet units. A sheet row wider than the canvas lays out at a fraction of its sheet's size and sets `reference.scale` to the crop pixels per unit, as the vertical slider row does; its side-by-side and zooms then show the reference larger than the capture.
- **States.** Each state names a cell (a logical rectangle including its glow) and optional `pin` steps; give each state its own control, and keep cells apart, since a pinned state's cell is pasted from its own capture over whatever another state's cell shows there. Unpinned cells come from one shared capture; each pinned state is captured on its own and its cell copied into the composite. `hover`, `press`, `drag` and `click` are real pointer input; `action` is a `GuiAction` on a control named with `lab.control(name)`; `selectText` sets the native selection or caret of the text input an earlier `click` focused, and `typeText` replaces that selection as typing does (`d02`); `wait` lets Host-clock time pass, such as a tooltip's delay, which the capture would otherwise settle before; `settle` waits for settled paint, so a later step meets what the client declared in response to an earlier one, such as the list a trigger's press opened (`f02`). Pointer, capture and focus feedback is released after every pinned state; `restore` actions undo value changes.
- **Themes.** Controls without a theme paint the runtime's default look of their kind, or the dial's for a slider presented as a dial ([built-in looks](../../../crates/ipp-core/src/world/systems/gui/presentation/looks.rs)), which speaks the [design language](#design-language); the control theme modules therefore have no rows, and the specimens declare the 16-unit body type the language's lengths are drawn at. Theme rows sit on the default look property by property. Rows use the generated `GuiTheme.parts` field names, including the cut corners, corner accents, inner glow and stroke marks of the box primitive ([examples](../../react/gui-authoring/pages/gui-paint.tsx)); [theme.ts](theme.ts) only adds the paint key (`part`, `state`, `variant`), and an unknown field name fails the capture. Row lengths are absolute logical units. A theme can instead name a built-in look of the contract with its `em`, so its lengths follow each control's font, as the switch (`{ look: "switch" }`) and amber button specimens do; `scale` draws a look's lengths at that scale in absolute units and `rows` replace its properties at their paint keys. Such a theme also carries the look's motion rows, so its controls move between states as the look does; row themes leave timing to each kind's default look. Specimens place their content with the colours and lengths of [themes/palette.ts](themes/palette.ts) and [themes/geometry.ts](themes/geometry.ts), which read the design language's tokens that the connected runtime exports (`GUI_SKIN_TOKENS`, imported from `@ipp/host-contract`, which the shared-Host tool resolves to the Host's generated client): a new colour or length goes once into the core's tokens beside the [built-in looks](../../../crates/ipp-core/src/world/systems/gui/presentation/looks.rs), with its role, rather than into one theme. `lab.skin(name)` attaches a theme to a control and declares nothing while that theme is empty, which shows the runtime default look. Keep rows out of the specimen module: theme modules are loaded separately so their edits stay row writes on the live theme entities, while a specimen edit re-declares the specimen's own entities.
- **Kit compositions.** Client compositions such as panels, window controls, data grids, toasts, alerts, badges, progress bars, expanders, radio groups, segmented controls, tabs, trees, context menus, popovers, tooltips, dialogs, selection controls, knobs, labelled and range sliders and slider scales come from the [GUI kit](../../../packages/ipp-react/README.md#gui-kit), not from lab helpers: the session declares a `GuiKit` at the lab's body size, so a specimen renders kit components directly, places them with `placed` from [kit.tsx](kit.tsx) and nests a `GuiKit` with the sheet's `fontSize` to draw at another sheet's scale (`b01` to `b03`, `c02`, `c03`, `d01`, `d03` to `d05`, `e01` to `e03`, `f01` to `f04`, `g01` to `g03`, `h01` to `h03`, `i01`, `i03` to `i06`). Their looks are the kit's, built from the exported tokens, so they need no theme module; a look change belongs in the kit. Kit animations never settle for a capture, so specimens of running spinners, rings and bars nest `<GuiKit reducedMotion>`, which holds the kit's motion beneath it still without sending the World's reduced-motion preference (`d07`, `f02`, `f04`, `i01`, `i02`). A top-level overlay such as the toast stack or a dialog is declared inside an entity but outside its `Children`, which makes it a root of the canvas (`i03`, `i04`). Light overlays open through the state's own pins, such as a click and the Menu key for a context menu (`g02`) or a click on a popover's trigger (`g03`), and close when its input is released; a hint needs a `wait` for its delay (`h03`). An anchored light overlay declared open shows open in the shared capture (`f01`, `f03`); one holding a focusable control takes focus as it opens, which closes the other light overlays, so such open states are pinned one by one (`f02`, `f04`). A kit component's `ref` is its control's handle, so `lab.control(name)` names it for `action` pins, such as focus on a range's thumb (`d05`, `e03`). Pins that only move focus, such as Tab and arrows in a group that does not select on arrows, repeat across captures; a press on an item of a selecting group selects it, so such a pin presses the item that is already selected.
- **Helpers.** [kit.tsx](kit.tsx) places entities at rectangles, draws the sheet background and labels with the shared font and places plain Buttons for the control specimens; [scroll-kit.tsx](scroll-kit.tsx) composes the scroll specimens' lists. A slider specimen's thumb pins follow the runtime's value-to-position mapping (`crates/ipp-core/src/world/systems/gui/local/controls/slider.rs`) for the kit slider's depth, whose thumb is one em of the kit's body size.

Captures wait for settled paint, so each shows where its transitions end. The a06 thumb pin follows the runtime's default scroll bar geometry; update `thumbCentre` in [scroll-geometry.ts](scroll-geometry.ts) if it changes.

## Design language

The reference sheets are image-model output: they agree on one design and vary at random in its details. The language keeps what the sheets agree on and what their written rules say; a difference between components stays only with a stated reason. Its values are the runtime's tokens, defined once with the [built-in looks](../../../crates/ipp-core/src/world/systems/gui/presentation/looks.rs) and exported through the generated contract; the lab reads them in [themes/palette.ts](themes/palette.ts) and [themes/geometry.ts](themes/geometry.ts).

**Shape.** Zero radius and 45° cuts in one pattern, paired on the top-left and bottom-right corners, the same in every state. The frame cut goes on full-size controls and content frames (primary buttons, fields, switch rails, list, tree and empty-state frames, floating menus, popovers, dialogs and toasts); the part cut on small controls, parts and secondary buttons. Circles only where the circle is the function: dial, radio mark, progress ring, spinner, the colour field's marker.

**Containers.** Panels and sections hold controls rather than act, so they are uncut: a thin accent frame with corner accents on all four corners, filled with the page; minimised, the same frame unlit. What sits on a container's lines takes its square corners: docked header buttons, the focused grid cell.

**Colour and lines.** Lit is accent, unlit is neutral. One idle and one lit line weight. Accent draws lit edges, container frames and heading rules; neutral draws idle frames and panel divisions; the quiet line draws what repeats between data (row separators, grid lines) and rail outlines, so the data reads first. Text: accent for titles and labels, text for content, neutral for secondary and disabled. Tints are role colours at a stated alpha. Status: information and success are the accent, warning amber, error magenta, inactive neutral, always with an icon and a word. Text selection keeps its own blue so selected text stays legible.

**States**, identical on every interactive part; Disabled > Pressed > Hover > Idle, focus independent:

- Idle: the idle line around the surface; value marks (checked fill, on block, slider value and thumb, scroll thumb) are lit at rest.
- Hover: the lit line, glow at half strength.
- Pressed: the pressed part (button or checkbox box, thumb, switch block) fills with the accent under the hover edge; its content takes the surface colour.
- Focus: the lit line with the full glow on the control's own border, never a second outline or a changed fill; brighter than hover so the keyboard target stays visible.
- Checked, on or selected: the accent fill, content in the surface colour. A selected row: the row tint and an accent bar on its leading edge; the active option of a list or menu: the row tint under the lit hover edge; an option that toggles, as in a multi-select, marks its selection with the check mark instead of the bar.
- Disabled: whatever would be lit draws in neutral; geometry and value stay.

Glow has one shape in two reaches by scale class: control frames, and half that for moving parts.

**Motion**, from the sheet's transition rules: hover fades in over 80 ms and out over 120 ms; a press is immediate and its release fades over 100 ms; focus, a check mark, a drag and disabling are immediate; a checked fill or a selection fades over 100 ms; the switch block travels its 40 units in 160 ms with an ease-out cubic, either way, and reverses from where it is. Reduced motion snaps every change. The built-in looks carry these as their motion rows.

**Hierarchy.** Primary buttons: frame cut, accent label. Secondary buttons (icon and window controls, CLEAR, Cancel): part cut, text label, at any size. The amber variant, for warnings and destructive actions, swaps the accent and the idle line for amber.

**Sizes**, on the sheet's 4-unit grid (its switch: rail 72:32, block 24, clearance 4, travel 40):

| Size | Used by |
| --- | --- |
| Control 40 | buttons, text fields, dropdowns, steppers, segmented controls, inline alerts |
| Small 32 | checkbox and radio, switch rail, secondary text buttons, badges |
| Docked 24 | header and title-bar buttons, switch block |
| Part 16, bar 8 | slider thumbs in both orientations; scroll bar, inset 8 |
| Dial 80 | an unsized dial, square, its tick ring half an em inside |
| Row 36, dense 24 | grid, tree, menu and option rows; logs |
| Inset 16 | content from the frame that holds it |
| Type 13, 16, 24 | small, body, display; icons 24 |

A toast is a control row in 8-unit margins (56). A colour control is a 144 field beside 24-wide rails over a 24 swatch, one em apart inside a half-em margin: 240 x 200 with its alpha rail; its marker is 12 across and its thumbs are bars 8 tall.

**Exceptions:**

- Scroll tracks point their ends: the one rail drawn inside a frame along its edge, they would otherwise read as a second box.
- The slider thumb is an outline at rest: solid, it would merge into the value bar it ends.
- The colour control's field, rails and swatch are uncut, square data in the quiet line: a cut would remove the extreme colours at the field's corners. Their edges light outward only, so no glow tints the colours, and disabled dims them instead of drawing them in neutral; the swatch keeps its colour. Its marker and rail thumbs are outlines in text with a dark halo, legible over any colour, and never lit: in the accent they would vanish over the colours nearest it, so a drag shows on the surface it holds.

## Reference crops

The crops are local and ignored by Git: `.resources/skin-reference/` of the primary checkout (override with `--references` or `IPP_SKIN_REFERENCES`); `crop.py` there regenerates them from the three sheets, and the `crop-*.py` scripts from the supplement sheets. Row crops `a01`…`a07`, `b01`…`b03` and `c01`…`c07`, and the supplement sheets' component crops such as `h02` and `i01`, are 2x; the `z*-` close-ups use other scales, so compare them with the shared-Host `compare` command and explicit boxes.
