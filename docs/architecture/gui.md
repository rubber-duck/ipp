# GUI on Surfaces

[Architecture overview](../architecture.md) · [Runtime strategy](../plans/runtime-and-rendering.md#gui-implementation-approach) · [React strategy](../plans/react-reconciler.md#gui-declarations) · [Persistence strategy](../plans/ecs-serialization.md#gui-state)

## Ownership and scope

React declares interface structure and application behavior. The optional GUI capability owns headless layout, interaction, committed control values and immediate feedback; platform adapters supply native input services. GUI evaluates into retained [Surface presentation](rendering.md#surface-presentation), without a React round trip or a second renderer.

A GuiRoot belongs to a Surface entity and owns lightweight root-local nodes, not widget entities. Exactly one producer supplies its content: authored Surface items or GUI output. Conflicting authoring is rejected rather than silently replacing either producer. Surface dimensions and entity placement remain ordinary authored state. GUI layout and rendering output never issue client commands to rewrite Surface items.

V1 supports in-world panels, mouse/keyboard/touch, layout containers, text/drawing/image leaves, buttons, checkboxes, sliders and single-line text inputs. It includes semantic metadata, but no screen-reader bridge. CSS compatibility, arbitrary constraint solvers, screen-space windowing, XR, simultaneous independent viewers, rich/multiline editing, complex shaping/bidi/emoji, arbitrary in-plane clip transforms, momentum and multi-finger gestures are outside this scope.

## Identity and authoritative state

Nodes and named skin parts have stable identities independent of tree order and generated primitive indices. Handles include their World/session and root/node lifetimes. Reordering preserves identity; removal, replacement and teardown invalidate property access, animation, interaction and pending work before reuse. Logical GUI ancestry is separate from entity parenting.

GuiRoot uses component-owned typed storage and the ordinary named-property, animation and sparse StateOverlay mechanisms. Each ordinary value has one authoritative store. Derived layout, paint, hit regions and semantics are reconstructible outputs; pending interaction bookkeeping and native text composition do not become another component mirror. Numeric animation retains prepared access with lifecycle invalidation. Computed geometry is not a generic writable property.

Controls own their committed values locally. Ordinary React commits do not replay initial values over newer edits. External replacement/reset is explicit and revision-aware; stale operations report a conflict without silently erasing subsequent input. Runtime defaults are declared before input dispatch. Application callbacks observe committed effects and cannot retroactively cancel them.

## Layout and presentation

Constraint layout shares the [Surface content convention](rendering.md#surface-presentation): a top-left origin, +X right and +Y down. Explicit logical units per metre provide unit scaling only, with no GUI-specific axis flip or origin translation. The Surface rectangle sets root constraints; Surface owns placement into entity/World space independently of camera motion. Row, Column, Stack, Padding, Align, SizedBox and ScrollView compose min/max constraints, flex and text measurement without an iterative cross-node solver.

Animation precedes layout. Layout properties cause reflow; visual translation/scale move paint and hit regions together. Retained output changes only with relevant inputs. Camera-only changes update projection and placement without remeasuring text or reflowing the tree.

Paint and hit testing share evaluated layout, visual transforms and intersected rectangular clips. GUI prepares ordered parameterized shapes, glyphs, complex drawings and bitmaps through the shared Surface preparation boundary. Ordinary controls prefer [rounded-rectangle materials and retained presentation](rendering.md#gui-shapes-and-retained-presentation); RenderService owns coverage, GPU packing, glyph reuse and compatible contiguous batching under ordinary scene depth, transparency and colour rules. Layout supplies shape dimensions independently of corner and border dimensions. Decorative glow affects paint bounds only; it does not create a larger hit target. Missing resources affect their dependent work, and normal asset readiness/recovery rebuilds it.

[Optional Surface texture caching](rendering.md#optional-surface-texture-caching) operates on evaluated paint under the rendering quality policy. It does not throttle GUI evaluation or input; focus and active interaction select current direct presentation so controls do not remain visually delayed by a distant cache's refresh cadence.

## Evaluation and input

GUI separates mutation/control, layout and input-routing responsibilities through declared System dependencies. [The runtime schedule](runtime.md#frame-order) remains fixed; adding GUI does not add subsystem execution branches to Host or World. AnimationSystem remains the sole sampler, including skin transitions.

Input arrives through ordered, session-fenced ingress. Routing uses one source tick's current layout and final camera/geometry state, selects targets and queues ordered actions for the next mutation boundary before animation. It does not mutate control values, reflow between events or recursively evaluate the World. Routing maintains the pending focus/capture/gesture decisions needed to interpret later input in order. Actions revalidate liveness and interaction scope before application; committed effects report both source and effect ticks. A withheld World evaluation or paused Host delays progress under existing scheduling policy; this is not a wall-clock latency promise.

One active input context per World supports multiple panels and pointer identities. Other clients may author or observe without acquiring input ownership. Uncaptured input selects the nearest eligible front-facing panel and reverse-painter eligible node within clips. Explicitly marked scene picking geometry blocks input; visual occlusion alone does not. Comparisons use World-space distances and deterministic ties. Captured input retains its target under its live context. Unhandled input is observable for ordinary scene controls without duplicate dispatch.

Runtime interaction owns hover, press/cancel, capture, focus scopes and tree-order keyboard traversal. Touch arbitration resolves tap versus slider/scroll drag and cancels the losing press; nested scrolling passes unused movement outward. Hiding, disabling or removing a node, losing platform focus, and releasing/replacing an input context cancel affected interactions. Session teardown fences queued work before identities can be reused.

## Text and skins

Text measurement and rendering share immutable font metrics and original glyph identities. Preserve Unicode source text and edit at grapheme boundaries, while rendering guarantees remain basic LTR Latin. Wrapping, glyph placement, caret and selection geometry use the same headless metrics. Runtime and browser text offsets require explicit conversion against the corresponding text revision.

Committed text is distinct from provisional composition. The platform adapter owns native focus, clipboard, IME, candidate placement and trusted-gesture soft-keyboard access; its temporary native buffer synchronizes through revision- and focus-fenced edits. Platform failure does not transfer text authority to the DOM or imply a successful edit.

Skins declare stable named parts with parameterized shape materials and ordinary immutable curve/font/bitmap/animation references. Material values use the existing typed-property and animation ownership rather than a separate styling evaluator. Slider fill and thumb follow the same committed value and rail geometry within the runtime. Resolve disabled over pressed over hovered over idle, with checked/value variants and a separate focus channel. Each property group has one resolved animation owner; overlapping skin channels do not compete to write the same property. Existing AnimationSystem transitions supply interruption continuity. Ready skin replacement preserves node identity, focus and values; pending replacements retain usable prior appearance.

## Client and persistence boundaries

The optional `@ipp/react/gui` entry point extends the existing reconciler; browser composition stays in `@ipp/react/web`. Keys retain runtime nodes, refs resolve after acknowledgement, and render-time work has no transport side effects. Capture/bubble callbacks follow the logical runtime ancestor path. JavaScript propagation controls only callbacks; native browser default policies must also be chosen before asynchronous runtime responses.

The semantic tree exposes roles, names, committed values, states, evaluated bounds and supported actions. Semantic actions use the same validated control path as other input.

The semantic tree is also the machine-client contract: autonomous agents and other programmatic clients observe and actuate GUI through semantic snapshots and actions, not by synthesizing pointer input or scraping paint. Snapshots carry revisions alongside values, bounds and states so observe-act loops can detect change and address actions to a known revision. What machine clients rely on is semantic invariance, not record layout: node and part identity, revision monotonicity, invalidation before reuse and conflict-instead-of-overwrite hold across visual-only change, so a reskin or relayout never retargets a handle. Record layouts themselves follow the pre-stabilization policy: clients regenerate against the runtime schema pairing and adapt to the shape they receive. Platform accessibility adapters remain separate work; the agent contract implies machine operability, not screen-reader support.

Eligible authored structure, identity metadata, asset references and committed values follow existing [snapshot ownership](protocol-and-schema.md#snapshots-and-world-replacement). Exclude derived caches and transient input contexts, focus, capture, selection, provisional composition, pending input/actions and GUI interaction playback. This exclusion is specific to GUI-owned transient playback; ordinary animation controllers retain their existing persistence semantics. Restored Worlds reconstruct GUI output and use fresh runtime handles.
