# Shared geometry

Headless geometry supplies Box, Sphere and Pill primitives, compound unions, affine placement and optional joint-pair mappings. `BoundingGeometry` provides conservative culling bounds; `PickingGeometry` independently enables interaction. They share evaluation machinery but never substitute for each other. Components own evaluated results, reconstructed after replacement/load.

Declarations choose inline geometry or an immutable geometry resource; leaving both empty derives geometry from the mesh. Queries and visualization consume final object and skeletal placement. Nonuniform scale and shear are preserved; unusable transforms or required geometry report failure. A tight authored picking volume is not automatically a conservative enclosure of a deformed mesh.

Culling requires trustworthy bounds. Missing, pending or unproven enclosures preserve visibility. The current explicit-enclosure proof requires one convex part to contain the generated bounds, so some valid compound enclosures remain uncullable. Skeletal mapping retains source/incarnation identity; replacing it requires an explicit rebind rather than silently reusing joint ordinals.

Presentation components retain a shared default BoundingGeometry through core component requirements; authored bounds take precedence. Geometry compiles resource/transform access, retains world enclosures, and publishes separate bounding/picking spatial domains. [Batch frustum queries](spatial/query.rs) share reusable caller storage across the flat and BVH implementations; unknown bounds remain candidates. Rigid placements update retained shapes and bounds without rebuilding unchanged geometry.

Visualization is optional presentation of these same evaluated shapes. It does not enable picking or alter culling, and its private renderer assets require no client registration. CPU triangle picking is unsupported.

Start with [component declarations](components.rs), [evaluation and skeletal mapping](update.rs) and [picking](picking.rs); the [definition codec](definition.rs) owns the asset format. The [rendering architecture](../../../../../../docs/architecture/rendering.md#bounding-and-picking-geometry) owns the contract; the [camera/picking guide](../../../../../../docs/development/cameras-and-picking.md) covers authoring. `python tools/ipp.py test geometry` adds native/browser queries and completed WebGL captures to focused geometry tests.
