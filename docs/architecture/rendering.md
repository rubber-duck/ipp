# Rendering and Interaction

[Architecture overview](../architecture.md) · [Implementation strategy](../plans/runtime-and-rendering.md#rendering-and-recovery)

## Shared render service

Each World's RenderSystem prepares final evaluated inputs; the Host's RenderService owns GPU execution. Contexts/surfaces, connections and Worlds have independent lifetimes. Submissions retain inputs or finish before the next mutation; rendering never advances simulation or reevaluates geometry. GPU handles, programs and packing stay outside persistent World state; shader source belongs to immutable assets.

Current GL composition supports one context per service, multiple Worlds and synchronous submission. Independent contexts over one catalog require further representation support. [Asset providers](assets.md#shader-recipes-and-graphics-loading) own graphics loading and readiness; [recovery](assets.md#retention-and-recovery) preserves immutable identity and usable CPU data.

## Compile-time GL devices

WebGL 2 and GLES 3 share preparation through compile-time devices. Bindings/dialects stay behind that boundary; Hosts own context libraries/shims. Future non-GL backends preserve World contracts without implementing the GL interface. Validate actual device capabilities; unsupported requirements need explicit errors or defined fallbacks.

## Material contract

Intended V1 scope includes unlit and forward metallic-roughness PBR, image-based lighting, directional/point/spot lights, spotlight shadows, skinning and optional MSAA. [Current scope](../development/building.md#toolchain-and-scope) distinguishes delivery.

Materials/lights are ordinary components; instance edits never mutate shared assets. Unlit/debug rendering is independent of lighting. Meshes retain supplied attributes only; required missing streams fail and optional streams use defined defaults. [Immutable recipes](assets.md#shader-recipes-and-graphics-loading) own program identity; disabled features omit snippets/composition.

Deformation proceeds from mesh-pose interpolation through skinning to final object placement, consistently across surface/depth passes and conservative headless bounds. Shadow, forward, resolve and presentation remain separate responsibilities.

## Lights and shadows

Select bounded influential lights per draw from unrestricted World lights. Use trustworthy evaluated bounds and instanced group bounds, stable selections near the cutoff and deterministic entity ties. Unknown bounds permit approximate ranking, never geometric rejection. Selection/history remain renderer state.

Allocate device-bounded shadows across the frame, prioritizing lights selected by visible draws and mapping draw indices to stable atlas slots. Unallocated lights still illuminate. Allocation failure reduces shadow participation with diagnostics without failing the World/session.

## Custom materials

Custom materials own dynamic properties independently of immutable backend-specific shader definitions. Definitions validate required names/types, permit extra properties and supply standardized interfaces without a generic shader-language translator. Omitted vertex bodies use standard deformation; missing fragments make that backend unavailable. Custom vertices own clip-space output and must preserve surface/shadow deformation through pass-specific transforms and shared helpers.

Select usable materials in custom → PBR → unlit order, with solid unlit red as the final custom fallback. Missing resources, unsupported requirements and compilation failures preserve authored state and report diagnostics. Surface and requested shadow passes use the same candidate; programs/failures follow the asset lifecycle independently of values.

Materials declare opaque, cutout or blended rendering and optional light/shadow participation. Lighting supplies inputs without imposing a shading model. Opaque/cutout draws write depth and group by shader/material state, with front-to-back depth inside each group. Particle instances retain their submission groups. Blended draws only depth-test and sort back-to-front with stable entity ties. Blend linearly before one display conversion. Blended custom materials do not cast shadows; cutout/discard applies to shadow passes.

Arbitrary vertex deformation disables culling unless authored bounds promise conservative enclosure. Picking still uses authored geometry. The [material guide](../../crates/ipp-render-gl/src/services/render/CUSTOM_MATERIALS.md) owns shader interfaces and packing.

## Cameras

Each World explicitly selects at most one camera, with no fallback. Absent/unusable selection clears without World draws and fails camera-dependent queries. Selection changes at mutation boundaries; removal is not repaired by diagnostics. Valid edits/selections recover, teardown releases selection, and invalid projection preserves selection/session.

Rendering/picking share final headless camera/projection state. Rendered queries use current Host surface dimensions; headless callers supply dimensions. Clients interpret gestures; Rust navigation writes base state under ordinary partial-mutation/overlay rules, never feeding evaluated state back into authoring.

## Bounding and picking geometry

Shared headless geometry supports Box, Sphere, Pill, placed compound unions and skeletal mappings.

| Component | Contract |
| --- | --- |
| `BoundingGeometry` | Conservative enclosure including deformation/scaling; required for culling rejection |
| `PickingGeometry` | Independent interaction shapes; presence enables picking |

Neither substitutes for the other. [Immutable definitions](assets.md#geometry-definitions) may be shared; instances own final-pose shapes, including joint-pair pills. Skeleton replacement invalidates mappings before ordinals can be reused.

Renderable presentation components require BoundingGeometry through core component dependencies. Authored bounds take precedence over a shared generated fallback; removing the authored producer restores the fallback while a dependent remains. Unknown or unproven bounds remain eligible for rendering, including pending resources and arbitrary vertex deformation. Particle bounds enclose the evaluated effect.

Geometry components retain derived world bounds beside their evaluated shapes. GeometrySystem owns spatial indexing over those results and publishes queries after final geometry evaluation. Consumers bind prepared geometry access rather than deriving enclosures independently. Bounding and picking query domains preserve their separate contracts.

Spatial queries accept batches and return conservative geometry/query matches into reusable caller storage. Index topology and traversal order are implementation details; object identity and order-sensitive tie-breaking remain independent. Frustum traversal narrows active-query bitmasks uniformly, including single-bit masks. Flat scans and spatial hierarchies share the query contract; backend selection and rebuilding occur outside active queries. Unknown render bounds remain explicit candidates rather than disappearing from an index-only traversal.

## Geometry picking

Picking is independent of renderer, materials, visibility and visualization. Missing required geometry fails rather than reporting hit/miss. Compare World-space distances with deterministic ties; results identify session, correlation and evaluated tick, not a historical displayed frame. Projection/view-plane queries support client-owned gestures without core drag state. CPU triangle picking is excluded; future GPU picking answers rendered-surface occupancy.

## Global render state

Settings use atomic sparse updates and independent [change events](protocol-and-schema.md#system-commands-queries-and-events). Omission preserves fields; rejection/no-op emits nothing. World replacement restores defaults; display overrides preserve authored components/selection. Uniform ambient fill is per-World, defaults to zero and affects PBR independently of punctual lighting. It is distinct from image-based lighting and leaves unlit/debug rendering unaffected.

## Geometry visualization

Bounding/picking components opt into visualization with the active camera, normal depth and unlit color; no separate debug component exists. Global overrides affect presentation only. The renderer privately owns demanded debug assets, without client resource records/events. Recovery regenerates them; allocation failure skips affected draws. Declarations remain valid headlessly; visualization is standard rendering functionality.

## Residency and recovery

Visibility/quality select per-entity variant demand; aggregate residency for observation without asset quotas. Unused allocations may be released. Allocation failure skips affected draws while preserving CPU data/other resources; retry after unload or context restoration.

Context loss suspends GPU work and restores programs/context state before demanded resources under the [immutable recovery contract](assets.md#retention-and-recovery). Resource/draw, context, World-evaluation and fatal Host failures have distinct scopes. Recoverable failures skip affected work or use fallback without destroying the Host; context loss reaches recovery even from material preparation. Validate uploads/allocations and diagnose draws at pass boundaries, with exhaustive checks available explicitly and [hot-path logging policy](runtime.md#diagnostic-logging) unchanged.

## World and color conventions

| Domain | Convention |
| --- | --- |
| World | Right-handed; +X right, +Y up, camera-forward −Z; metres/radians |
| Surface content | Top-left origin; +X right, +Y down; metres; shared by raw items and GUI |
| Transforms | Translation, xyzw quaternion, scale; column vectors/matrices; local `T × R × S` |
| Faces | Counter-clockwise front faces |
| Import/backend | Explicit foreign-coordinate and clip-space conversion |
| Color | Linear RGB with sRGB primaries; linear coverage alpha; explicit texture/output conversion |
| Initial opaque output | Linear RGB → sRGB, without tone mapping |

## Particle presentation

Sprites own unlit appearance/alignment; particle meshes use immutable geometry and sibling materials, including custom materials. GL owns instance buffers and packing outside schemas/cache formats. Default vertices specialize ordinary/instanced inputs; custom vertices implement the instancing interface, with shared parameters per effect.

Alpha particles participate in scene transparency order with deterministic ties, splitting batches as needed; additive particles need no depth sorting. Bounds must enclose the effect rather than its source mesh. Particle picking is outside initial scope.

## Surface presentation

An optional Surface component owns an ordered collection of text, drawing and bitmap items. Surface content uses metres with a top-left origin, +X right and +Y down; its width and height define the content rectangle. Raw items and GUI share this convention for placement, clips, glyph runs and interaction. Surface items have component-local identities independent of their painter's order and are not separate entities.

Surface owns the mapping from this 2D content space to its centred entity-local XY plane, with +Z facing forward; normal entity transforms then place it in the World. Rendering, conservative bounds and plane interaction use the same mapping. The World retains its right-handed, +Y-up convention. Source font/drawing conventions are normalized at the shared Surface asset/preparation boundary rather than through caller-specific flips.

With [GUI content ownership](gui.md#ownership-and-scope), a Surface instead consumes retained derived primitives from its GuiRoot. Both paths share preparation and GPU execution; generated output never replaces authored item state. GUI adds per-primitive intersected rectangular clips and resizable rectangle/rounded-box backgrounds under the same painter-order, depth and colour contracts.

Item structure and content belong to the component. Item properties use the ordinary named-property, animation and sparse StateOverlay mechanisms; reordering preserves bindings and removal invalidates them before reuse. Numeric animation retains prepared access rather than rebuilding the collection. Persistence preserves underlying item state and typed asset references without retaining a second component mirror.

Fonts and drawings share quadratic contour evaluation. Basic string layout is headless; explicit glyph runs preserve client-owned shaping and positioning. Offline converters own font/SVG interpretation, while renderer-owned providers prepare GPU representations under the ordinary immutable asset lifecycle. RenderSystem submits evaluated Surface inputs independently of mesh submissions, and RenderService owns curve coverage, bitmap sampling and GPU packing.

Surfaces render unlit and double-sided at presentation resolution with scene depth testing; rear views show the same live content mirrored as through glass, while +Z remains the semantic front for interaction. Items compose in explicit painter's order with batching limited to compatible contiguous work; separate Surfaces participate in ordinary scene transparency ordering. Rectangular clipping is evaluated in Surface coordinates. Missing resources suppress only affected items, and context recovery preserves logical state. Raw Surface interaction uses geometry and plane queries; [GUI interaction](gui.md#evaluation-and-input) additionally owns node hit testing and explicit scene blockers, independently of rendering.
