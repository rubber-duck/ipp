# GUI skin motion

A control's parts move between the appearances its interaction key resolves: its state (disabled over pressed over hovered over idle), its checked variant (a checkbox's value, a button's selection) and whether its focus ring shows. GUI prepares each transition; [`AnimationSystem`'s sampler](../../animation/gui_motion.rs) advances it on the Host clock. The [architecture](../../../../../../../docs/architecture/gui.md#text-and-skins) owns the design; this module owns the contract below.

## Timing rows

Timing comes from [`GuiMotionPart`](component.rs) rows in the [`GuiThemeMotion`](component.rs) companion of the theme a `GuiSkin` names, then from the motion rows of the control's [default look](../presentation/looks.rs), its kind's or the dial's, each property resolving independently along the part's chain exactly as appearance properties do. A row is keyed by a qualified part identity and sets any of:

- `duration`: Host seconds of the transition into that appearance;
- `easing`: 0 linear, 1 smoothstep, 2 ease-out cubic ([`GuiMotionEasing`](component.rs)); absent is linear;
- `exit`: Host seconds of the transition out of that state into one of lower precedence, in place of the destination's duration.

[`GuiMotionRows::timing`](timing.rs) picks the rows of one change:

1. A change of checked variant takes the destination's variant chain (state and variant, state, base).
2. Otherwise leaving a state for one of lower precedence takes the left state's `exit`, with that state's easing, where its state chain declares one.
3. Otherwise the destination's state chain (state, base) gives the duration and easing; variant rows time only variant changes.

No resolved duration, a zero duration or reduced motion make the change immediate, and so does a change that leaves every animated value where it was. A focus ring or a check mark without rows therefore changes at once. A transition needs no clip: its destination is the resolved appearance.

The built-in looks state the reference sheet's rules this way: hover in 80 ms (the hovered row) and out 120 ms (the base row, the transition into idle); a press immediate with its release over 100 ms (the pressed row's zero duration and its exit); disable immediate both ways (zero duration and exit); a checked fill or a selection over 100 ms (variant rows); the switch block over 160 ms with an ease-out cubic (its variant rows). Each exported look carries its motion rows, so a theme made from one moves as the look does.

## Channels and material

A transition animates the part's colour, opacity, scale, horizontal alignment, line colour, line width and glow intensity ([`GuiMotionValues`](runtime.rs)), from the displayed values to the destination's, with paint's neutral value where one end sets nothing. Every other property is material: it adopts the destination when the transition starts, keeps the origin's value where the destination sets none, so a glow keeps its reach while it fades out, and keeps the origin's paint asset until the transition ends. Gradient stops and geometry, corner shapes, fill modes, strokes and glow reach and colour do not change between the built-in looks' states, and a stepped shape value cannot interpolate; animating them would add per-frame work no look uses.

A part that paints only while visible fades toward zero opacity: an unchecked indicator its look leaves unstyled, and a focus ring that no longer shows. The painter keeps drawing either while its channel is above zero.

A control with several focus parts paints each part's own primitives with that part's pointer state: a range slider's thumbs, each through its own Icon, and a colour control's field, hue rail and alpha rail, each through its own Track with the field's Marker or a rail's thumb Icon. Its key also records each part's state, and each of those primitives moves on a channel of its own ([`focus_part_channel`](runtime.rs)) in place of the control's one Icon; the focus ring changes at once as focus moves between parts. A numeric text input with step parts likewise records the state of its decrement and increment parts, each disabled while its direction is at its bound, and each part and its mark move on channels of their own; its field takes only the pointers over the rest of it.

## Ownership and lifetime

GUI records each control's last key in schema-ignored state of its `GuiBehavior` ([`GuiMotionRuntime`](runtime.rs)). When a dirty control's key changes, it resolves the previous and new appearance of each part and writes a request (origin, destination, timing and a new transition identity) into that part's channels; an interrupted part's origin is its current sample, so a reversal continues from where it is without a snap. When the key is unchanged, the destination and material of a transition under way follow theme, override, inherited font and tree edits, keeping its origin, timing and clock; a settled part has no channels, and paint resolves the edit directly.

`AnimationSystem` is the sole sampler. It restarts a part's clock when its transition identity changes, advances it by each Host frame, writes the eased sample and marks the transition settled when the elapsed time reaches the authored duration. GUI releases settled channels at its next preparation. Generic fields, ordinary controllers and persistence cannot address the channels. Restored and replaced controls start without them: preparation first records their key, so the replacement itself appears at once. Exact control and `GuiBehavior` incarnations fence every request and sample.

A World that does not select `AnimationSystem` records no keys and creates no channels: every part paints its resolved appearance and every change is immediate. `GuiThemeMotion` therefore requires the Animation System.

## Reduced motion

[`GuiPreferences`](../preferences.rs) is GUI System state: `reduced_motion` makes every transition immediate, and turning it on releases every transition under way to its destination. Hosts and clients change it with the `GuiPreferencesUpdate` System command, or the protocol's `GuiPreferencesUpdateCommand`, at the mutation boundary; the `GuiPreferences` System query reads it, as does the protocol's `guiPreferences` inspection collection, and the World saves it like other System state. The React kit's `GuiKit` sends its one reduced-motion setting as this preference and holds the kit's own spinners, pulses and toast fades still under the same setting.

## Work

Preparation visits only changed controls: committed control, `GuiBehavior` and skin changes, routed interaction and focus changes, eligibility the GUI System re-evaluated, and theme, font and tree edits for controls with transitions under way. An unchanged key costs a key comparison and no appearance resolution. Static and settled controls keep only their key, inline in `GuiBehavior`, and cost nothing per frame. `WorldContext::gui_motion_work` reports the latest frame's inspected controls, resolved part destinations, transitions started, parts visited and samples written; the maintained motion suite checks static, settled, unrelated-change and single-transition work separately.
