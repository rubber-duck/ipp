//! Camera-independent local geometry at the algorithm/retained-presentation boundary.

use super::PlotSystem;
use crate::services::asset_management::formats::drawing::FillRule;
use crate::services::asset_management::formats::quadratic::QuadraticContour;
use crate::services::data::DataRowId;
use std::sync::Arc;

/// Exact identity of a data mark, independent of source row position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlotRowIdentity {
    /// Stable authored series row slot.
    pub series: u32,
    /// Full source-row identity.
    pub row_id: DataRowId,
}

/// Local closed contours, shared with the existing analytic path renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotPath {
    /// Local top-left/Y-down control-point enclosure.
    pub bounds: [f32; 4],
    /// Ordered closed quadratic contours; algorithms expand strokes into contours.
    pub contours: Arc<[QuadraticContour]>,
    /// Winding convention for this paint.
    pub fill_rule: FillRule,
}

/// One compact plane-local paint item. Parts are stable within this component.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotPrimitive {
    /// Retained part identity, unrelated to painter order.
    pub part: u32,
    /// Local origin in top-left/Y-down plane coordinates.
    pub position: [f32; 2],
    /// Straight linear RGBA.
    pub color: [f32; 4],
    /// Paint geometry or source text to be measured by the shared font path.
    pub kind: PlotPrimitiveKind,
}

/// Shared compact geometry vocabulary; no primitive requires its own entity.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum PlotPrimitiveKind {
    /// Existing retained Canvas box pipeline.
    Box {
        size: [f32; 2],
        border_width: f32,
        border_color: [f32; 4],
    },
    /// Runtime-derived closed analytic path.
    Path(Arc<PlotPath>),
    /// Shared headless string layout, measured once per geometry change.
    Text {
        text: Arc<str>,
        font_size: f32,
    },
}

/// Retained ordinary triangle geometry, independent of GPU resources or asset IDs.
/// Split large results into chunks with at most 65535 vertices (u16 mesh indices).
#[derive(Clone, Debug, PartialEq)]
pub struct PlotMesh {
    /// Stable part identity within the chart.
    pub part: u32,
    /// Entity-local positions in metres.
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex linear RGB; empty means white.
    pub colors: Vec<[f32; 3]>,
    /// Per-vertex local normals; empty selects unlit geometry.
    pub normals: Vec<[f32; 3]>,
    /// Counter-clockwise triangles.
    pub indices: Vec<u16>,
    /// Straight linear material multiplier; alpha determines ordinary transparency.
    pub color: [f32; 4],
}

/// View orientation for immutable physical plane paint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlotPlaneFacing {
    /// Retained entity-local basis, used for grid and axis geometry.
    #[default]
    Fixed,
    /// Selected camera right/down basis about the retained data/world anchor.
    /// Only the presentation matrix changes; paint and glyph streams remain retained.
    Camera,
}

/// View-local arrangement role; this is derived presentation, never authored state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PlotPlaneLayout {
    /// Fixed grid/axis paint has no label arrangement.
    #[default]
    None,
    /// A Cartesian tick with X/Y/Z lane preference (0/1/2).
    Tick(u8),
    /// A Cartesian title placed outside its tick lane.
    Title(u8),
    /// A client-authored annotation panel about its data anchor.
    Callout,
    /// Retained unit strip joining its exact anchor to an arranged callout.
    Connector {
        /// Stable callout plane part in this same chart publication.
        panel: u32,
        /// Authored panel attachment in camera-right/down local units.
        endpoint: [f32; 2],
        /// Physical strip width before chart scaling.
        width: f32,
    },
}

/// Selected-view station policy over immutable chart-local paint.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PlotPlanePlacement {
    /// Preserve the authored physical station.
    #[default]
    Fixed,
    /// Move a Cartesian grid to the far bound of its normal axis.
    Grid {
        /// Physical chart enclosure.
        extent: [f32; 3],
        /// Normal axis X/Y/Z (0/1/2).
        normal: u8,
    },
    /// Preserve increasing coordinates on selected-view enclosure edges.
    Axis {
        /// Physical chart enclosure.
        extent: [f32; 3],
        /// Increasing data axis X/Y/Z (0/1/2).
        axis: u8,
        /// Adapt to this view; otherwise retain its last displayed perimeter station.
        adaptive: bool,
    },
    /// Pie bisector at the slice's own height, independent of authored offset direction.
    Radial {
        /// Chart-local slice centre at that slice's top height.
        center: [f32; 3],
        /// Outer rim on the slice bisector at the same height.
        rim: [f32; 3],
        /// Authored offset magnitude adds physical radial clearance, never a screen side.
        spacing: f32,
    },
}

/// Text/path/shape content on a physical scene-depth plane owned by Plot itself.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotPlane {
    /// Stable chart-local plane identity.
    pub part: u32,
    /// Plane-local top-left/Y-down content to entity-local affine, column-major.
    pub model: [f32; 16],
    /// Presentation orientation without changing the retained anchor or paint.
    pub facing: PlotPlaneFacing,
    /// View-local arrangement of immutable paint.
    pub layout: PlotPlaneLayout,
    /// Selected-view position; never changes local paint or source-row identity.
    pub placement: PlotPlanePlacement,
    /// Local content clipping rectangle.
    pub clip: [f32; 4],
    /// Ordered ordinary plane primitives.
    pub primitives: Vec<PlotPrimitive>,
}

/// Independent analytic hit geometry; no CPU triangle picking is introduced.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotHit {
    /// Reported data mark identity.
    pub row: PlotRowIdentity,
    /// Compact geometry in chart-local coordinates.
    pub shape: PlotHitShape,
}

/// Geometry used for row picking, independently of presentation visibility/materials.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum PlotHitShape {
    /// Canvas-local rectangle.
    Rect([f32; 4]),
    /// Canvas-local circle.
    Circle {
        center: [f32; 2],
        radius: f32,
    },
    /// Canvas-local clockwise radial sector, angles in radians from twelve o'clock.
    Sector {
        center: [f32; 2],
        inner_radius: f32,
        radius: f32,
        start: f32,
        sweep: f32,
    },
    /// Entity-local 3D box.
    Box {
        min: [f32; 3],
        max: [f32; 3],
    },
    /// Exact radial prism: clockwise +Y rotation from +Z, X/Z centre and absolute Y limits.
    RadialPrism {
        center: [f32; 3],
        radius: f32,
        start: f32,
        sweep: f32,
        min_y: f32,
        max_y: f32,
    },
    /// Entity-local 3D sphere.
    Sphere {
        center: [f32; 3],
        radius: f32,
    },
}

/// Complete local result. A successful result replaces all retained geometry atomically;
/// errors suppress stale output and never acknowledge the binding dirty flag.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlotPreparedGeometry {
    /// Ordered 2D primitives inserted into the ordinary Canvas entity walk.
    pub canvas: Vec<PlotPrimitive>,
    /// Ordinary scene meshes, retained across camera/transform changes.
    pub meshes: Vec<PlotMesh>,
    /// Shared scene-depth label/axis planes.
    pub planes: Vec<PlotPlane>,
    /// Independent compact row-picking shapes.
    pub hits: Vec<PlotHit>,
    /// Conservative chart-local 3D enclosure when scene geometry is present.
    pub bounds: Option<[[f32; 3]; 2]>,
}

impl Default for PlotMesh {
    fn default() -> Self {
        Self {
            part: 0,
            positions: Vec::new(),
            colors: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            color: [1.0; 4],
        }
    }
}

impl PlotPrimitive {
    /// A filled local box using the ordinary Canvas shape pipeline.
    pub fn box_fill(part: u32, position: [f32; 2], size: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            part,
            position,
            color,
            kind: PlotPrimitiveKind::Box {
                size,
                border_width: 0.0,
                border_color: [0.0; 4],
            },
        }
    }

    /// A generated analytic contour in top-left/Y-down local coordinates.
    pub fn path(part: u32, path: PlotPath, color: [f32; 4]) -> Self {
        Self {
            part,
            position: [0.0; 2],
            color,
            kind: PlotPrimitiveKind::Path(Arc::new(path)),
        }
    }

    /// Source text; font identity and glyph layout are resolved by PlotSystem.
    pub fn text(
        part: u32,
        position: [f32; 2],
        text: impl Into<Arc<str>>,
        font_size: f32,
        color: [f32; 4],
    ) -> Self {
        Self {
            part,
            position,
            color,
            kind: PlotPrimitiveKind::Text {
                text: text.into(),
                font_size,
            },
        }
    }
}

/// Completed local primitive data and entity placement for one Plot component lifetime.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotPublishedChart {
    /// Exact ordinary producing component lifetime.
    pub target: crate::systems::canvas::CanvasTarget,
    /// Derived local output reused across camera and placement edits.
    pub geometry: Arc<PlotPreparedGeometry>,
    /// Completed final entity-local to World-local transform.
    pub model: [f32; 16],
    /// Prepared scene meshes through the ordinary material/draw contract.
    pub meshes: Vec<PlotPublishedMesh>,
    /// Prepared shared plane paint (font glyphs included).
    pub planes: Arc<[PlotPublishedPlane]>,
}

/// Retained geometry beside an ordinary complete scene draw.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotPublishedMesh {
    /// Complete ordinary mesh/material/placement draw inputs.
    pub render: crate::systems::render::PublishedRenderItem,
    /// Stable part and immutable local CPU mesh streams.
    pub geometry: Arc<PlotPreparedGeometry>,
    /// Mesh ordinal in the immutable complete local result.
    pub mesh_index: usize,
}

/// Shared local paint on a scene plane; neither a child World nor a Surface component.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotPublishedPlane {
    /// Stable local plane identity.
    pub part: u32,
    /// Plane-local content to entity-local physical mapping.
    pub model: [f32; 16],
    /// Selected view orientation, applied by RenderService after composition.
    pub facing: PlotPlaneFacing,
    /// View-local arrangement of immutable paint.
    pub layout: PlotPlaneLayout,
    /// Selected-view position; never changes local paint or source-row identity.
    pub placement: PlotPlanePlacement,
    /// Plane-local clip.
    pub clip: [f32; 4],
    /// Actual glyph/panel paint bounds after font preparation, in plane-local units.
    pub bounds: Option<[f32; 4]>,
    /// Ordinary prepared primitive paint.
    pub primitives: Arc<[crate::systems::canvas::CanvasPrimitive]>,
}

/// Completed immutable Plot output retained with its World publication.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotPublication {
    /// Entity/component ordered derived chart inputs.
    pub charts: Vec<PlotPublishedChart>,
}

impl PlotSystem {
    pub(super) fn publish(
        &self,
        world: &crate::WorldContext<'_>,
        output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), crate::ErrorReason> {
        use crate::systems::render::{PbrMaterial, PublishedRenderItem, RenderItem, UnlitMaterial};
        let mut charts = Vec::with_capacity(self.state.charts.len());
        for chart in self.state.charts.values() {
            for resource in chart
                .canvas
                .iter()
                .chain(
                    chart
                        .planes
                        .iter()
                        .flat_map(|plane| plane.primitives.iter()),
                )
                .filter_map(|primitive| primitive.resource())
            {
                output.retain(resource);
            }
            let model = if chart.geometry.meshes.is_empty() && chart.planes.is_empty() {
                [
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ]
            } else {
                match world.world_matrix(chart.target.entity) {
                    Ok(model) => model,
                    Err(_) => continue,
                }
            };
            let normal = crate::systems::geometry::GeometryShapeTransform::from_matrix(model)?
                .render_normal_matrix();
            let transform = world
                .world
                .components
                .transform(chart.target.entity.index() as usize)
                .copied()
                .unwrap_or_default();
            let meshes = chart
                .geometry
                .meshes
                .iter()
                .enumerate()
                .map(|(mesh_index, mesh)| PlotPublishedMesh {
                    render: PublishedRenderItem {
                        item: RenderItem {
                            particle: None,
                            solid_fallback: false,
                            custom_material: false,
                            normals: !mesh.normals.is_empty(),
                            texture_weights: false,
                            skinned: false,
                            entity: chart.target.entity,
                            transform,
                            model,
                            normal,
                            material: UnlitMaterial {
                                r: mesh.color[0],
                                g: mesh.color[1],
                                b: mesh.color[2],
                            },
                            pbr: (!mesh.normals.is_empty()).then_some(PbrMaterial {
                                r: mesh.color[0],
                                g: mesh.color[1],
                                b: mesh.color[2],
                                metallic: 0.0,
                                roughness: 0.6,
                                receive_shadows: false,
                                cast_shadows: false,
                            }),
                            mesh: crate::MeshKey {
                                asset: 0,
                                variant: mesh.part,
                            },
                            pose: None,
                            texture: None,
                        },
                        incarnation: chart.target.incarnation,
                        custom: None,
                        palette: None,
                    },
                    geometry: chart.geometry.clone(),
                    mesh_index,
                })
                .collect();
            charts.push(PlotPublishedChart {
                target: chart.target,
                geometry: chart.geometry.clone(),
                model,
                meshes,
                planes: chart.planes.clone(),
            });
        }
        output.chunk(
            Self::ID,
            PlotPublication {
                charts,
            },
        );
        Ok(())
    }
}
