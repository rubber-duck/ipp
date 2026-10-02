//! Tests for retained GUI shape batches, their per-Surface GPU storage and the
//! order their shapes and glyphs draw in.

use std::cell::RefCell;
use std::rc::Rc;

use std::collections::{BTreeMap, BTreeSet};

use super::super::gui_records::{GuiGlyphRecord, GuiRecord, GuiRecordKind, GuiShapeRecord};
use super::super::gui_storage::{GuiPiece, GuiPieceKey, GuiPieceSource};
use super::super::retained_surfaces::SurfacePaint;
use super::{
    GUI_PAINT_STROKE, GuiBatchRenderCache, MAX_BATCH_BOXES, RetainedSurfaceSubmission,
    STORAGE_RETRY_FRAMES, VOLATILE_FRAMES, generate_box_records, hash_box_inputs,
};
use crate::services::render::frame_statistics::RenderFrameWork;
use crate::{RenderDevice, RenderError};
use ipp_core::systems::canvas::{
    CanvasBoxShape, CanvasClip, CanvasPart, CanvasPrimitive, CanvasPrimitiveId,
    CanvasPrimitiveStyle, CanvasShapeFill, CanvasShapeGlow,
};

/// Records of one storage allocation or draw.
#[derive(Clone, Debug, PartialEq)]
enum MockRecords {
    Shapes(Vec<GuiShapeRecord>),
    Glyphs(Vec<GuiGlyphRecord>),
}

impl MockRecords {
    /// Shape records; empty for glyph records.
    fn shapes(&self) -> &[GuiShapeRecord] {
        match self {
            Self::Shapes(records) => records,
            Self::Glyphs(_) => &[],
        }
    }

    /// Glyph records; empty for shape records.
    fn glyphs(&self) -> &[GuiGlyphRecord] {
        match self {
            Self::Glyphs(records) => records,
            Self::Shapes(_) => &[],
        }
    }

    fn slice(&self, range: std::ops::Range<usize>) -> Self {
        match self {
            Self::Shapes(records) => Self::Shapes(records[range].to_vec()),
            Self::Glyphs(records) => Self::Glyphs(records[range].to_vec()),
        }
    }
}

/// One recorded draw.
#[derive(Clone, Debug)]
struct MockDraw {
    batch: usize,
    program: u32,
    records: MockRecords,
    atlas: bool,
}

#[derive(Default)]
struct MockGuiDevice {
    /// `(id, capacity)` of every storage allocation.
    created_batches: Vec<(usize, usize)>,
    /// `(id, first, len)` of every storage write.
    writes: Vec<(usize, usize, usize)>,
    deleted_batches: Vec<usize>,
    /// Every draw, in order.
    draws: Vec<MockDraw>,
    /// Current contents of every live storage allocation.
    contents: BTreeMap<usize, MockRecords>,
    next_id: usize,
    fail_writes: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct MockGuiBatch {
    id: usize,
}

impl RenderDevice for MockGuiDevice {
    fn viewport_limits(&self) -> Option<crate::ViewportLimits> {
        None
    }

    type Program = u32;
    type Mesh = u32;
    type Texture = u32;
    type SurfacePath = u32;
    type SurfaceCacheTarget = ();
    type SurfaceInstances = ();
    type ShadowMap = u32;
    type GuiBatch = MockGuiBatch;
    type GlyphAtlasPage = u32;

    fn set_lighting(
        &mut self,
        _program: &Self::Program,
        _model: &[f32; 16],
        _normal: &[f32; 16],
        _surface: &[f32; 3],
        _frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn create_shadow_map(&mut self, _size: u32) -> Result<Self::ShadowMap, RenderError> {
        Ok(0)
    }

    fn begin_shadow(
        &mut self,
        _map: &Self::ShadowMap,
        _slot: u32,
        _grid: u32,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn end_shadow(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    fn bind_shadow(
        &mut self,
        _program: &Self::Program,
        _map: &Self::ShadowMap,
        _frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn delete_shadow_map(&mut self, _map: Self::ShadowMap) {}

    fn create_program(
        &mut self,
        _vertex: &str,
        _fragment: &str,
    ) -> Result<Self::Program, RenderError> {
        Ok(1)
    }

    fn create_mesh(&mut self, _asset: &ipp_core::MeshAsset) -> Result<Self::Mesh, RenderError> {
        Ok(1)
    }

    fn create_texture(
        &mut self,
        _width: u32,
        _height: u32,
        _pixels: &[u8],
    ) -> Result<Self::Texture, RenderError> {
        Ok(1)
    }

    fn allocate_texture(
        &mut self,
        _width: u32,
        _height: u32,
    ) -> Result<Self::Texture, RenderError> {
        Ok(1)
    }

    fn upload_texture_rows(
        &mut self,
        _texture: &Self::Texture,
        _width: u32,
        _first_row: u32,
        _rows: u32,
        _pixels: &[u8],
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn begin_frame(
        &mut self,
        _width: u32,
        _height: u32,
        _clear: &[f32; 4],
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn draw(
        &mut self,
        _program: &Self::Program,
        _mesh: &Self::Mesh,
        _mvp: &[f32; 16],
        _material: &[f32; 3],
        _pose: Option<(&Self::Mesh, f32)>,
        _texture: Option<&Self::Texture>,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    fn delete_mesh(&mut self, _mesh: Self::Mesh) {}

    fn delete_texture(&mut self, _texture: Self::Texture) {}

    fn delete_program(&mut self, _program: Self::Program) {}

    fn create_gui_batch(
        &mut self,
        kind: GuiRecordKind,
        capacity: usize,
    ) -> Result<Self::GuiBatch, RenderError> {
        self.next_id += 1;
        let id = self.next_id;
        self.created_batches.push((id, capacity));
        self.contents.insert(
            id,
            match kind {
                GuiRecordKind::Shape => MockRecords::Shapes(vec![GuiShapeRecord::EMPTY; capacity]),
                GuiRecordKind::Glyph => MockRecords::Glyphs(vec![GuiGlyphRecord::EMPTY; capacity]),
            },
        );
        Ok(MockGuiBatch {
            id,
        })
    }

    fn write_gui_batch<R: GuiRecord>(
        &mut self,
        batch: &mut Self::GuiBatch,
        first: usize,
        records: &[R],
    ) -> Result<(), RenderError> {
        if self.fail_writes {
            return Err(RenderError::RenderDevice("injected write failure".into()));
        }

        self.writes.push((batch.id, first, records.len()));
        let written: Box<dyn std::any::Any> = Box::new(records.to_vec());
        let range = first..first + records.len();
        match self.contents.get_mut(&batch.id).expect("live storage") {
            MockRecords::Shapes(contents) => contents[range].copy_from_slice(
                written
                    .downcast_ref::<Vec<GuiShapeRecord>>()
                    .expect("shape records in shape storage"),
            ),
            MockRecords::Glyphs(contents) => contents[range].copy_from_slice(
                written
                    .downcast_ref::<Vec<GuiGlyphRecord>>()
                    .expect("glyph records in glyph storage"),
            ),
        }
        Ok(())
    }

    fn delete_gui_batch(&mut self, batch: Self::GuiBatch) {
        self.contents.remove(&batch.id);
        self.deleted_batches.push(batch.id);
    }

    fn draw_gui_batch(
        &mut self,
        program: &Self::Program,
        batch: &Self::GuiBatch,
        atlas: Option<&Self::Texture>,
        _mvp: &[f32; 16],
        first: usize,
        count: usize,
    ) -> Result<(), RenderError> {
        let records = self.contents[&batch.id].slice(first..first + count);
        self.draws.push(MockDraw {
            batch: batch.id,
            program: *program,
            records,
            atlas: atlas.is_some(),
        });
        Ok(())
    }

    fn create_glyph_atlas_page(
        &mut self,
        _width: u32,
        _height: u32,
    ) -> Result<Self::GlyphAtlasPage, RenderError> {
        self.next_id += 1;
        Ok(self.next_id as u32)
    }

    fn delete_glyph_atlas_page(&mut self, _page: Self::GlyphAtlasPage) {}

    fn begin_glyph_atlas_page(&mut self, _page: &Self::GlyphAtlasPage) -> Result<(), RenderError> {
        Ok(())
    }

    fn end_glyph_atlas_page(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    fn glyph_atlas_texture(page: &Self::GlyphAtlasPage) -> &Self::Texture {
        page
    }
}

fn sample_box_primitive(
    node: u32,
    part: CanvasPart,
    position: [f32; 2],
    size: [f32; 2],
    clip: Option<CanvasClip>,
) -> CanvasPrimitive {
    CanvasPrimitive::Box {
        style: CanvasPrimitiveStyle {
            identity: CanvasPrimitiveId {
                target: ipp_core::systems::canvas::CanvasTarget {
                    entity: ipp_core::EntityId::from_bits(u64::from(node)),
                    component: ipp_core::ComponentValue::CANVAS_BOX,
                    incarnation: 1,
                },
                part,
            },
            position,
            scale: [1.0, 1.0],
            color: [0.2, 0.4, 0.6, 1.0],
            opacity: 1.0,
            clip: clip.unwrap_or([
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::INFINITY,
            ]),
            layer: 0,
        },
        size,
        corner_radius: [0.05, 0.05],
        border_width: 0.01,
        border_color: [0.8, 0.8, 0.8, 1.0],
        fill: CanvasShapeFill::Solid([0.2, 0.4, 0.6, 1.0]),
        glow: None,
        shape: CanvasBoxShape::RECT,
    }
}

/// Clip of generated test geometry.
const CLIP: CanvasClip = [-100.0, -100.0, 100.0, 100.0];

/// Canvas content rectangle of the test Surfaces.
const DOMAIN: CanvasClip = [0.0, 0.0, 8.0, 2.0];

/// Programs the test draws pass for shapes and for glyphs.
const SHAPE_PROGRAM: u32 = 1;

const GLYPH_PROGRAM: u32 = 2;

/// One Surface submission whose boxes all share a clip, as the service performs it.
trait DrawBoxBatch {
    #[allow(clippy::too_many_arguments)]
    fn draw_box_batch(
        &mut self,
        program: &u32,
        entity: ipp_core::EntityId,
        clip: CanvasClip,
        paint: SurfacePaint,
        boxes: &[&CanvasPrimitive],
        mvp: &[f32; 16],
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError>;
}

impl DrawBoxBatch for GuiBatchRenderCache<MockGuiDevice> {
    fn draw_box_batch(
        &mut self,
        program: &u32,
        entity: ipp_core::EntityId,
        clip: CanvasClip,
        paint: SurfacePaint,
        boxes: &[&CanvasPrimitive],
        mvp: &[f32; 16],
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        let clipped: Vec<_> = boxes.iter().map(|&primitive| (primitive, clip)).collect();
        self.begin_surface(entity);
        self.push_boxes(paint, &clipped, stats);
        if !self.commit_surface(|_, _| &[], stats)? {
            return Ok(());
        }

        let pieces = self.piece_count();
        self.draw_pieces(
            program,
            &GLYPH_PROGRAM,
            0..pieces,
            DOMAIN,
            |_| None,
            mvp,
            stats,
        )
    }
}

#[test]
fn a_filled_box_is_one_record_over_its_padded_rectangle() {
    let style = CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: ipp_core::systems::canvas::CanvasTarget {
                entity: ipp_core::EntityId::from_bits(1),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Background,
        },
        position: [1.0, 2.0],
        scale: [1.0, 1.0],
        color: [1.0; 4],
        opacity: 1.0,
        clip: [
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ],
        layer: 0,
    };
    let size = [3.0, 4.0];
    let corner = [0.1, 0.2];
    let border_color = [0.0, 1.0, 0.0, 1.0];
    let fill = CanvasShapeFill::Solid([1.0, 0.0, 0.0, 1.0]);

    let records = generate_box_records(
        &style,
        &size,
        &corner,
        0.05,
        &border_color,
        &fill,
        None,
        &CanvasBoxShape::RECT,
        CLIP,
    );
    assert_eq!(records.len(), 1);

    // Quad corners in Surface coordinates with 0.002 conservative AA padding:
    // x0 = 1.0 - 0.002 = 0.998, y0 = 2.0 - 0.002 = 1.998
    // x1 = 4.0 + 0.002 = 4.002, y1 = 6.0 + 0.002 = 6.002
    assert_eq!(records[0].rect, [0.998, 1.998, 4.002, 6.002]);

    for v in &records {
        assert_eq!(v.placement, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(v.color0, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(v.color1, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(v.border_color, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(v.shape, [0.1, 0.2, 0.05, 0.0]);
        assert_eq!(v.gradient_coords, [0.0; 4]);
        assert_eq!(v.material_params, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(v.glow_color, [0.0; 4]);
        assert_eq!(v.clip, CLIP);
    }
}

#[test]
fn linear_gradient_fill_sets_coordinates_and_material_type() {
    let style = CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: ipp_core::systems::canvas::CanvasTarget {
                entity: ipp_core::EntityId::from_bits(1),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Background,
        },
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 0.5,
        clip: [
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ],
        layer: 0,
    };
    let fill = CanvasShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [2.0, 1.0],
        start_color: [1.0, 0.0, 0.0, 1.0],
        end_color: [0.0, 0.0, 1.0, 0.8],
    };

    let vertices = generate_box_records(
        &style,
        &[2.0, 1.0],
        &[0.0, 0.0],
        0.0,
        &[0.0; 4],
        &fill,
        None,
        &CanvasBoxShape::RECT,
        CLIP,
    );
    assert_eq!(vertices.len(), 1);

    for v in &vertices {
        assert_eq!(v.color0, [1.0, 0.0, 0.0, 0.5]);
        assert_eq!(v.color1, [0.0, 0.0, 1.0, 0.4]);
        assert_eq!(v.gradient_coords, [0.0, 0.0, 2.0, 1.0]);
        assert_eq!(v.material_params[0], 1.0);
    }
}

#[test]
fn radial_gradient_fill_sets_center_radius_and_material_type() {
    let style = CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: ipp_core::systems::canvas::CanvasTarget {
                entity: ipp_core::EntityId::from_bits(1),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Background,
        },
        position: [0.0, 0.0],
        scale: [2.0, 2.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: [
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ],
        layer: 0,
    };
    let fill = CanvasShapeFill::RadialGradient {
        center: [0.5, 0.5],
        radius: 0.25,
        start_color: [1.0, 1.0, 0.0, 1.0],
        end_color: [0.0, 1.0, 1.0, 0.0],
    };

    let vertices = generate_box_records(
        &style,
        &[1.0, 1.0],
        &[0.0, 0.0],
        0.0,
        &[0.0; 4],
        &fill,
        None,
        &CanvasBoxShape::RECT,
        CLIP,
    );
    assert_eq!(vertices.len(), 1);

    for v in &vertices {
        assert_eq!(v.color0, [1.0, 1.0, 0.0, 1.0]);
        assert_eq!(v.color1, [0.0, 1.0, 1.0, 0.0]);
        assert_eq!(v.gradient_coords, [1.0, 1.0, 0.5, 0.0]);
        assert_eq!(v.material_params[0], 2.0);
    }
}

#[test]
fn glow_expands_quad_padding_and_packs_glow_parameters() {
    let style = CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: ipp_core::systems::canvas::CanvasTarget {
                entity: ipp_core::EntityId::from_bits(1),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Background,
        },
        position: [2.0, 3.0],
        scale: [1.0, 1.0],
        color: [1.0; 4],
        opacity: 0.8,
        clip: [
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ],
        layer: 0,
    };
    let glow = CanvasShapeGlow {
        color: [0.3, 0.6, 0.9, 1.0],
        intensity: 1.5,
        radius: 0.05,
        inner_radius: 0.0,
        falloff: 2.0,
    };

    let vertices = generate_box_records(
        &style,
        &[1.0, 1.0],
        &[0.02, 0.02],
        0.0,
        &[0.0; 4],
        &CanvasShapeFill::Solid([0.0; 4]),
        Some(&glow),
        &CanvasBoxShape::RECT,
        CLIP,
    );
    assert_eq!(vertices.len(), 1);

    let expected_x0 = 2.0 - 0.052;
    let expected_y0 = 3.0 - 0.052;
    let expected_x1 = 3.0 + 0.052;
    let expected_y1 = 4.0 + 0.052;

    assert!((vertices[0].rect[0] - expected_x0).abs() < 1e-6);
    assert!((vertices[0].rect[1] - expected_y0).abs() < 1e-6);
    assert!((vertices[0].rect[2] - expected_x1).abs() < 1e-6);
    assert!((vertices[0].rect[3] - expected_y1).abs() < 1e-6);

    // The intensity joins the glow alpha; the inner reach is absent.
    for v in &vertices {
        assert_eq!(v.material_params[1], 0.0);
        assert_eq!(v.material_params[2], 0.05);
        assert_eq!(v.material_params[3], 2.0);
        assert_eq!(v.glow_color, [0.3, 0.6, 0.9, 0.8 * 1.5]);
    }
}

#[test]
fn border_only_box_splits_into_four_edge_strips_without_interior() {
    let style = CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: ipp_core::systems::canvas::CanvasTarget {
                entity: ipp_core::EntityId::from_bits(1),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Background,
        },
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0; 4],
        opacity: 1.0,
        clip: [
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ],
        layer: 0,
    };
    let border_color = [1.0, 1.0, 1.0, 1.0];
    let size = [10.0, 10.0];
    let corner = [0.1, 0.1];
    let border_width = 0.2;

    let glow = CanvasShapeGlow {
        color: [0.0, 1.0, 0.0, 1.0],
        intensity: 1.0,
        radius: 0.15,
        inner_radius: 0.0,
        falloff: 2.0,
    };
    for glow in [None, Some(&glow)] {
        let vertices = generate_box_records(
            &style,
            &size,
            &corner,
            border_width,
            &border_color,
            &CanvasShapeFill::Solid([0.0, 0.0, 0.0, 0.0]),
            glow,
            &CanvasBoxShape::RECT,
            CLIP,
        );
        assert_eq!(
            vertices.len(),
            4,
            "large border-only box splits into 4 edge quads"
        );

        for record in &vertices {
            assert!(record.rect[0] < record.rect[2] && record.rect[1] < record.rect[3]);
            assert_eq!(record.placement, vertices[0].placement);
        }
    }
}

#[test]
fn small_border_only_box_uses_single_quad() {
    let style = CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: ipp_core::systems::canvas::CanvasTarget {
                entity: ipp_core::EntityId::from_bits(1),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Background,
        },
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0; 4],
        opacity: 1.0,
        clip: [
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ],
        layer: 0,
    };
    let border_color = [1.0, 1.0, 1.0, 1.0];
    let size = [0.3, 0.3];
    let corner = [0.1, 0.1];
    let border_width = 0.1;

    let vertices = generate_box_records(
        &style,
        &size,
        &corner,
        border_width,
        &border_color,
        &CanvasShapeFill::Solid([0.0, 0.0, 0.0, 0.0]),
        None,
        &CanvasBoxShape::RECT,
        CLIP,
    );
    assert_eq!(
        vertices.len(),
        1,
        "small border-only box uses a single quad to avoid strip overhead"
    );
}

/// Unclipped style of node 1's background at `position` and `scale`.
fn shape_style(position: [f32; 2], scale: [f32; 2]) -> CanvasPrimitiveStyle {
    CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: ipp_core::systems::canvas::CanvasTarget {
                entity: ipp_core::EntityId::from_bits(1),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Background,
        },
        position,
        scale,
        color: [1.0; 4],
        opacity: 1.0,
        clip: [
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ],
        layer: 0,
    }
}

fn rect(cut: [f32; 4], accent: [f32; 4], accent_width: f32) -> CanvasBoxShape {
    CanvasBoxShape::Rect {
        corner_cut: cut,
        corner_accent: accent,
        corner_accent_width: accent_width,
        checker: None,
    }
}

/// Axis-aligned `[min_x, min_y, max_x, max_y]` of each generated quad.
fn quads(records: &[GuiShapeRecord]) -> Vec<[f32; 4]> {
    records.iter().map(|record| record.rect).collect()
}

/// Number of quads containing `point`; overlapping coverage would blend twice.
fn coverage(vertices: &[GuiShapeRecord], point: [f32; 2]) -> usize {
    quads(vertices)
        .iter()
        .filter(|quad| {
            point[0] >= quad[0] && point[0] <= quad[2] && point[1] >= quad[1] && point[1] <= quad[3]
        })
        .count()
}

fn assert_near(actual: [f32; 4], expected: [f32; 4]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() < 1e-5),
        "{actual:?} != {expected:?}"
    );
}

fn glow(radius: f32, inner_radius: f32) -> CanvasShapeGlow {
    CanvasShapeGlow {
        color: [0.0, 1.0, 1.0, 0.5],
        intensity: 2.0,
        radius,
        inner_radius,
        falloff: 1.0,
    }
}

const TRANSPARENT: CanvasShapeFill = CanvasShapeFill::Solid([0.0; 4]);

const OPAQUE: CanvasShapeFill = CanvasShapeFill::Solid([0.0, 0.1, 0.1, 1.0]);

#[test]
fn corner_cuts_and_accents_clamp_per_corner_without_growing_paint_bounds() {
    let style = shape_style([1.0, 2.0], [1.0, 1.0]);
    let vertices = generate_box_records(
        &style,
        &[10.0, 4.0],
        &[0.5, 0.5],
        0.25,
        &[1.0; 4],
        &OPAQUE,
        None,
        &rect([100.0, -3.0, 1.0, f32::NAN], [3.0; 4], 0.75),
        CLIP,
    );

    // Cuts never paint beyond the rectangle: one quad over it and its antialias pad.
    assert_eq!(vertices.len(), 1);
    assert_near(quads(&vertices)[0], [0.998, 1.998, 11.002, 6.002]);
    for vertex in &vertices {
        // The oversized top-left cut fills the 4-high left side; invalid cuts are
        // none. Accents of 3 along the 4-high sides meet halfway.
        assert_near(vertex.corner_cut, [4.0, 0.0, 1.0, 0.0]);
        assert_near(vertex.corner_accent, [2.0; 4]);
        assert_eq!(vertex.shape, [0.5, 0.5, 0.25, 0.75]);
        assert_eq!(vertex.material_params, [0.0, 0.0, 0.0, 1.0]);
    }

    // Degenerate uses: cuts of half the short side point a bar's ends, and two
    // cuts of the whole height of a box twice as wide as high make a triangle.
    for (size, cuts, expected) in [
        ([10.0, 2.0], [9.0; 4], [1.0; 4]),
        ([4.0, 2.0], [100.0, 100.0, 0.0, 0.0], [2.0, 2.0, 0.0, 0.0]),
        ([4.0, 2.0], [100.0, 0.0, 100.0, 0.0], [2.0, 0.0, 2.0, 0.0]),
    ] {
        let vertices = generate_box_records(
            &style,
            &size,
            &[0.0; 2],
            0.0,
            &[0.0; 4],
            &OPAQUE,
            None,
            &rect(cuts, [0.0; 4], 0.0),
            CLIP,
        );
        assert_near(vertices[0].corner_cut, expected);
    }
}

#[test]
fn inner_glow_reaches_inward_without_growing_paint_bounds() {
    let style = shape_style([0.0, 0.0], [2.0, 2.0]);
    let inner = generate_box_records(
        &style,
        &[5.0, 5.0],
        &[0.0; 2],
        0.0,
        &[0.0; 4],
        &OPAQUE,
        Some(&glow(0.0, 0.3)),
        &CanvasBoxShape::RECT,
        CLIP,
    );
    assert_eq!(inner.len(), 1);
    assert_near(quads(&inner)[0], [-0.002, -0.002, 10.002, 10.002]);
    for vertex in &inner {
        // Both reaches scale with the primitive; intensity joins the glow alpha.
        assert_eq!(vertex.material_params, [0.0, 0.6, 0.0, 1.0]);
        assert_eq!(vertex.glow_color, [0.0, 1.0, 1.0, 1.0]);
    }

    let both = generate_box_records(
        &style,
        &[5.0, 5.0],
        &[0.0; 2],
        0.0,
        &[0.0; 4],
        &OPAQUE,
        Some(&glow(0.25, 0.3)),
        &CanvasBoxShape::RECT,
        CLIP,
    );
    assert_near(quads(&both)[0], [-0.502, -0.502, 10.502, 10.502]);
    assert_eq!(both[0].material_params, [0.0, 0.6, 0.5, 1.0]);

    // Neither reach, or a non-finite one, is no glow.
    for glow in [glow(0.0, 0.0), glow(f32::NAN, 0.3), glow(0.25, -1.0)] {
        let vertices = generate_box_records(
            &style,
            &[5.0, 5.0],
            &[0.0; 2],
            0.0,
            &[0.0; 4],
            &OPAQUE,
            Some(&glow),
            &CanvasBoxShape::RECT,
            CLIP,
        );
        assert_eq!(vertices[0].material_params, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(vertices[0].glow_color, [0.0; 4]);
    }
}

#[test]
fn transparent_outlines_cover_inner_glow_and_omit_their_interior() {
    let style = shape_style([0.0, 0.0], [1.0, 1.0]);
    let vertices = generate_box_records(
        &style,
        &[20.0, 20.0],
        &[0.0; 2],
        0.25,
        &[1.0; 4],
        &TRANSPARENT,
        Some(&glow(0.0, 2.0)),
        &CanvasBoxShape::RECT,
        CLIP,
    );

    // The inner glow paints over the empty fill, so the strips reach its depth.
    assert_eq!(vertices.len(), 4);
    assert_near(quads(&vertices)[0], [-0.002, -0.002, 20.002, 2.0]);
    assert_eq!(coverage(&vertices, [10.0, 1.9]), 1);
    assert_eq!(coverage(&vertices, [10.0, 10.0]), 0);

    // An inner glow too deep for strips to pay off covers the whole box.
    let deep = generate_box_records(
        &style,
        &[20.0, 20.0],
        &[0.0; 2],
        0.25,
        &[1.0; 4],
        &TRANSPARENT,
        Some(&glow(0.0, 8.0)),
        &CanvasBoxShape::RECT,
        CLIP,
    );
    assert_eq!(deep.len(), 1);
}

#[test]
fn cut_and_accented_outlines_cover_their_corners_deeper_than_their_edges() {
    let style = shape_style([0.0, 0.0], [1.0, 1.0]);
    let pad = 0.002;
    // A cut top-left corner: its ring runs along the cut, sqrt(2) - 1 border
    // widths beyond the cut's end on each edge.
    let cut = generate_box_records(
        &style,
        &[20.0, 20.0],
        &[0.0; 2],
        0.5,
        &[1.0; 4],
        &TRANSPARENT,
        None,
        &rect([2.0, 0.0, 0.0, 0.0], [0.0; 4], 0.0),
        CLIP,
    );
    let reach = 2.0 + (std::f32::consts::SQRT_2 - 1.0) * 0.5;
    assert_eq!(cut.len(), 8);
    let pieces = quads(&cut);
    assert_near(pieces[0], [-pad, -pad, reach, reach]);
    // Other corners and the edges reach only the border width.
    assert_near(pieces[2], [19.5, 19.5, 20.0 + pad, 20.0 + pad]);
    assert_near(pieces[4], [reach, -pad, 19.5, 0.5]);
    for point in [[1.2, 1.0], [10.0, 0.25], [0.25, 10.0], [19.9, 19.9]] {
        assert_eq!(coverage(&cut, point), 1, "{point:?}");
    }
    assert_eq!(coverage(&cut, [10.0, 10.0]), 0);

    // Corner brackets alone paint no edge between their spans: only the four
    // accented corner squares remain.
    let brackets = rect([0.0; 4], [4.0; 4], 1.0);
    let alone = generate_box_records(
        &style,
        &[20.0, 20.0],
        &[0.0; 2],
        0.0,
        &[1.0; 4],
        &TRANSPARENT,
        None,
        &brackets,
        CLIP,
    );
    assert_eq!(alone.len(), 4);
    assert_near(quads(&alone)[1], [16.0, -pad, 20.0 + pad, 4.0]);
    assert_eq!(coverage(&alone, [3.9, 0.5]), 1);
    assert_eq!(coverage(&alone, [10.0, 0.5]), 0);
    assert_eq!(coverage(&alone, [10.0, 10.0]), 0);

    // An outer glow follows the whole contour, so the edges return.
    let glowing = generate_box_records(
        &style,
        &[20.0, 20.0],
        &[0.0; 2],
        0.0,
        &[1.0; 4],
        &TRANSPARENT,
        Some(&glow(0.5, 0.0)),
        &brackets,
        CLIP,
    );
    assert_eq!(glowing.len(), 8);
    assert_eq!(coverage(&glowing, [10.0, -0.4]), 1);
    assert_eq!(coverage(&glowing, [10.0, 10.0]), 0);

    // Mirroring the box mirrors its own corners onto the screen.
    let mirrored = generate_box_records(
        &shape_style([20.0, 0.0], [-1.0, 1.0]),
        &[20.0, 20.0],
        &[0.0; 2],
        0.5,
        &[1.0; 4],
        &TRANSPARENT,
        None,
        &rect([2.0, 0.0, 0.0, 0.0], [0.0; 4], 0.0),
        CLIP,
    );
    let pieces = quads(&mirrored);
    assert_near(pieces[0], [-pad, -pad, 0.5, 0.5]);
    assert_near(pieces[1], [20.0 - reach, -pad, 20.0 + pad, reach]);
}

#[test]
fn strokes_cover_their_segments_from_their_own_bounds() {
    let style = shape_style([1.0, 1.0], [1.0, 1.0]);
    let horizontal = CanvasBoxShape::Stroke {
        segments: [[0.0, 0.5, 1.0, 0.5], [0.5, 0.0, 0.5, 0.0]],
    };
    let fill = CanvasShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [10.0, 0.0],
        start_color: [1.0; 4],
        end_color: [0.0, 0.0, 0.0, 1.0],
    };
    let vertices = generate_box_records(
        &style,
        &[10.0, 10.0],
        &[0.0; 2],
        2.0,
        &[0.0; 4],
        &fill,
        Some(&glow(0.5, 0.0)),
        &horizontal,
        CLIP,
    );

    // Butt caps end at the part's edges; the thickness and glow extend across.
    assert_eq!(vertices.len(), 1);
    assert_near(quads(&vertices)[0], [0.498, 4.498, 11.502, 7.502]);
    for vertex in &vertices {
        assert_eq!(vertex.placement, [1.0, 5.0, 10.0, 2.0]);
        assert_eq!(vertex.shape, [0.0, 0.0, 2.0, 0.0]);
        // Segments are centres and half vectors from the stroke's own origin; the
        // zero-length second segment paints nothing.
        assert_eq!(vertex.corner_cut, [5.0, 1.0, 5.0, 0.0]);
        assert_eq!(vertex.corner_accent, [0.0; 4]);
        // The gradient stays anchored to the part rectangle.
        assert_eq!(vertex.gradient_coords, [0.0, -4.0, 10.0, -4.0]);
        assert_eq!(
            vertex.material_params,
            [1.0 + GUI_PAINT_STROKE, 0.0, 0.5, 1.0]
        );
    }

    // A diagonal stroke reaching the part's corners extends past them by its
    // half thickness.
    let check = CanvasBoxShape::Stroke {
        segments: [[0.0, 0.0, 1.0, 1.0], [0.0, 1.0, 1.0, 0.0]],
    };
    let vertices = generate_box_records(
        &style,
        &[10.0, 10.0],
        &[0.0; 2],
        2.0,
        &[0.0; 4],
        &OPAQUE,
        None,
        &check,
        CLIP,
    );
    let half = std::f32::consts::FRAC_1_SQRT_2;
    assert_near(
        quads(&vertices)[0],
        [
            1.0 - half - 0.002,
            1.0 - half - 0.002,
            11.0 + half + 0.002,
            11.0 + half + 0.002,
        ],
    );
    assert_eq!(vertices[0].material_params[0], GUI_PAINT_STROKE);
    assert_near(
        vertices[0].corner_accent,
        [5.0 + half, 5.0 + half, 5.0, -5.0],
    );

    // Without a segment of length nothing paints, while the primitive keeps its slot.
    let nothing = generate_box_records(
        &style,
        &[10.0, 10.0],
        &[0.0; 2],
        2.0,
        &[0.0; 4],
        &OPAQUE,
        None,
        &CanvasBoxShape::Stroke {
            segments: [[0.5; 4], [f32::NAN, 0.0, 1.0, 1.0]],
        },
        CLIP,
    );
    assert_eq!(nothing, vec![GuiShapeRecord::EMPTY]);
}

#[test]
fn shape_inputs_change_the_retained_hash() {
    let style = shape_style([0.0, 0.0], [1.0, 1.0]);
    let hash = |glow: Option<&CanvasShapeGlow>, shape: &CanvasBoxShape| {
        hash_box_inputs(
            &style,
            &[10.0, 10.0],
            &[0.0; 2],
            1.0,
            &[1.0; 4],
            &OPAQUE,
            glow,
            shape,
            CLIP,
        )
    };
    let base = hash(Some(&glow(1.0, 0.0)), &CanvasBoxShape::RECT);
    let variants = [
        hash(Some(&glow(1.0, 0.5)), &CanvasBoxShape::RECT),
        hash(
            Some(&glow(1.0, 0.0)),
            &rect([1.0, 0.0, 0.0, 0.0], [0.0; 4], 0.0),
        ),
        hash(
            Some(&glow(1.0, 0.0)),
            &rect([0.0; 4], [0.0, 0.0, 0.0, 2.0], 0.0),
        ),
        hash(Some(&glow(1.0, 0.0)), &rect([0.0; 4], [0.0; 4], 1.0)),
        hash(
            Some(&glow(1.0, 0.0)),
            &CanvasBoxShape::Stroke {
                segments: [[0.0; 4]; 2],
            },
        ),
        hash(
            Some(&glow(1.0, 0.0)),
            &CanvasBoxShape::Stroke {
                segments: [[0.0; 4], [0.0, 0.0, 1.0, 0.0]],
            },
        ),
    ];
    for (index, variant) in variants.into_iter().enumerate() {
        assert_ne!(variant, base, "variant {index}");
    }
}

#[test]
fn warm_frame_uploads_zero_geometry_bytes() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity = ipp_core::EntityId::from_bits(1);
    let box1 = sample_box_primitive(1, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    let box2 = sample_box_primitive(2, CanvasPart::Background, [1.5, 0.0], [1.0, 1.0], None);
    let boxes = [&box1, &box2];
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];

    // Frame 1: Cold upload
    let mut stats1 = RenderFrameWork::default();
    cache
        .draw_box_batch(
            &1,
            entity,
            clip,
            SurfacePaint::UNKNOWN,
            &boxes,
            &mvp,
            &mut stats1,
        )
        .unwrap();

    assert_eq!(stats1.statistics.gui_allocations, 1);
    assert_eq!(stats1.statistics.gui_rebuilds, 2);
    assert_eq!(stats1.statistics.gui_batches, 1);
    assert_eq!(stats1.summary.triangles, 4); // 2 boxes * 2 triangles
    let expected_bytes = 2 * std::mem::size_of::<GuiShapeRecord>() as u32;
    assert_eq!(stats1.statistics.uploaded_bytes, expected_bytes);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().writes, [(1, 0, 2)]);

    // Frame 2: Warm unchanged frame
    let mut stats2 = RenderFrameWork::default();
    cache
        .draw_box_batch(
            &1,
            entity,
            clip,
            SurfacePaint::UNKNOWN,
            &boxes,
            &mvp,
            &mut stats2,
        )
        .unwrap();

    assert_eq!(stats2.statistics.gui_allocations, 0);
    assert_eq!(stats2.statistics.gui_rebuilds, 0);
    assert_eq!(stats2.statistics.gui_batches, 1);
    assert_eq!(stats2.summary.triangles, 4);
    assert_eq!(
        stats2.statistics.uploaded_bytes, 0,
        "warm frame must upload 0 bytes"
    );
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().writes.len(), 1);
}

#[test]
fn local_change_replaces_batch_storage_and_rebuilds_only_affected_primitive() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity = ipp_core::EntityId::from_bits(1);
    let box1 = sample_box_primitive(1, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    let box2 = sample_box_primitive(2, CanvasPart::Background, [1.5, 0.0], [1.0, 1.0], None);
    let boxes_initial = [&box1, &box2];
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];

    let mut stats1 = RenderFrameWork::default();
    cache
        .draw_box_batch(
            &1,
            entity,
            clip,
            SurfacePaint::UNKNOWN,
            &boxes_initial,
            &mvp,
            &mut stats1,
        )
        .unwrap();
    assert_eq!(stats1.statistics.gui_rebuilds, 2);

    // Modify only box2 (e.g. hovered fill colour or slider thumb position)
    let mut box2_modified = box2.clone();
    if let CanvasPrimitive::Box {
        fill,
        ..
    } = &mut box2_modified
    {
        *fill = CanvasShapeFill::Solid([1.0, 1.0, 0.0, 1.0]);
    }
    let boxes_modified = [&box1, &box2_modified];

    let mut stats2 = RenderFrameWork::default();
    cache
        .draw_box_batch(
            &1,
            entity,
            clip,
            SurfacePaint::UNKNOWN,
            &boxes_modified,
            &mvp,
            &mut stats2,
        )
        .unwrap();

    assert_eq!(
        stats2.statistics.gui_rebuilds, 1,
        "only the modified primitive geometry rebuilds"
    );
    // The changed box turns volatile and leaves its stable neighbour's batch: that
    // batch rewrites its slot without it, and the changed box's own batch follows it
    // in the same storage, in one upload.
    assert_eq!(stats2.statistics.gui_allocations, 2);
    assert_eq!(device.borrow().writes[1..], [(1, 0, 2)]);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(stats2.summary.draw_calls, 1);
    let expected_bytes = 2 * std::mem::size_of::<GuiShapeRecord>() as u32;
    assert_eq!(stats2.statistics.uploaded_bytes, expected_bytes);
}

#[test]
fn tint_change_on_gradient_box_rebuilds_retained_geometry() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let mut panel = sample_box_primitive(1, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    if let CanvasPrimitive::Box {
        fill,
        ..
    } = &mut panel
    {
        *fill = CanvasShapeFill::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
            start_color: [0.1, 0.2, 0.3, 1.0],
            end_color: [0.3, 0.2, 0.1, 1.0],
        };
    }
    let draw = |cache: &mut GuiBatchRenderCache<MockGuiDevice>, primitive| {
        let mut stats = RenderFrameWork::default();
        cache
            .draw_box_batch(
                &1,
                ipp_core::EntityId::from_bits(1),
                [0.0, 0.0, 4.0, 2.0],
                SurfacePaint::UNKNOWN,
                &[primitive],
                &[0.0; 16],
                &mut stats,
            )
            .unwrap();
        stats
    };
    draw(&mut cache, &panel);

    let mut transitioning = panel.clone();
    if let CanvasPrimitive::Box {
        style,
        ..
    } = &mut transitioning
    {
        style.color = [1.0, 0.0, 0.0, 1.0];
    }
    let stats = draw(&mut cache, &transitioning);
    assert_eq!(stats.statistics.gui_rebuilds, 1);
    assert_eq!(stats.statistics.gui_allocations, 1);
    assert!(stats.statistics.uploaded_bytes > 0);
    assert_eq!(device.borrow().writes.len(), 2);
}

#[test]
fn boxes_of_every_part_class_share_one_batch() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let part_box = |node: u32, part: CanvasPart, x: f32| {
        sample_box_primitive(node, part, [x, 0.0], [0.4, 0.4], None)
    };

    // A row's background, slider track, fill, checkbox icon and focus ring paint in
    // this order under one clip; none of them changed recently.
    let row = [
        part_box(1, CanvasPart::Background, 0.0),
        part_box(2, CanvasPart::Background, 0.5),
        part_box(2, CanvasPart::Fill, 0.6),
        part_box(3, CanvasPart::Icon, 1.0),
        part_box(3, CanvasPart::FocusRing, 1.0),
    ];
    let boxes: Vec<_> = row.iter().collect();
    let mut stats = RenderFrameWork::default();
    cache
        .draw_box_batch(
            &1,
            ipp_core::EntityId::from_bits(1),
            [0.0, 0.0, 4.0, 2.0],
            SurfacePaint::UNKNOWN,
            &boxes,
            &[0.0; 16],
            &mut stats,
        )
        .unwrap();

    assert_eq!(stats.statistics.gui_batches, 1, "{stats:?}");
    assert_eq!(stats.summary.draw_calls, 1);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().writes, [(1, 0, 5)]);
}

#[test]
fn text_overlay_boxes_of_one_node_keep_their_own_geometry() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let overlay = |part: CanvasPart, x: f32, width: f32| {
        sample_box_primitive(7, part, [x, 0.0], [width, 0.1], None)
    };

    // A focused text input paints its selection highlight, then its caret bar;
    // both belong to node 7 but keep separate retained geometry.
    let selection = overlay(CanvasPart::Selection, 0.0, 1.5);
    let caret = overlay(CanvasPart::Caret, 2.0, 0.01);
    let boxes = [&selection, &caret];
    let clip = [0.0, 0.0, 4.0, 2.0];
    let expected: Vec<GuiShapeRecord> = boxes
        .iter()
        .flat_map(|primitive| {
            let CanvasPrimitive::Box {
                style,
                size,
                corner_radius,
                border_width,
                border_color,
                fill,
                glow,
                shape,
            } = primitive
            else {
                unreachable!();
            };
            generate_box_records(
                style,
                size,
                corner_radius,
                *border_width,
                border_color,
                fill,
                glow.as_ref(),
                shape,
                clip,
            )
        })
        .collect();

    let entity = ipp_core::EntityId::from_bits(1);
    let live = BTreeSet::from([entity]);
    let mut frame = || {
        let mut stats = RenderFrameWork::default();
        let before = device.borrow().draws.len();
        cache
            .draw_box_batch(
                &1,
                entity,
                clip,
                SurfacePaint::UNKNOWN,
                &boxes,
                &[0.0; 16],
                &mut stats,
            )
            .unwrap();
        cache.finish_frame(Some(&RetainedSurfaceSubmission {
            live: &live,
            submitted: &live,
        }));
        let drawn: Vec<GuiShapeRecord> = device.borrow().draws[before..]
            .iter()
            .flat_map(|draw| draw.records.shapes().to_vec())
            .collect();
        (stats, drawn)
    };

    let (cold, drawn) = frame();
    assert_eq!(drawn, expected);
    assert_eq!(cold.statistics.gui_rebuilds, 2);

    // An unchanged overlay frame neither rebuilds nor uploads.
    let (warm, drawn) = frame();
    assert_eq!(drawn, expected);
    assert_eq!(warm.statistics.gui_rebuilds, 0);
    assert_eq!(warm.statistics.uploaded_bytes, 0);
}

#[test]
fn unchanged_paint_revisions_skip_hashing_until_the_revision_changes() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let entity = ipp_core::EntityId::from_bits(1);
    let clip = [0.0, 0.0, 4.0, 2.0];
    let draw = |cache: &mut GuiBatchRenderCache<MockGuiDevice>,
                paint: SurfacePaint,
                boxes: &[&CanvasPrimitive]| {
        let mut stats = RenderFrameWork::default();
        cache
            .draw_box_batch(&1, entity, clip, paint, boxes, &[0.0; 16], &mut stats)
            .unwrap();
        stats
    };
    let first = sample_box_primitive(1, CanvasPart::Background, [0.0; 2], [1.0; 2], None);
    let second = sample_box_primitive(2, CanvasPart::Background, [1.5, 0.0], [1.0; 2], None);
    let revision = |revision, reusable| SurfacePaint {
        opacity: 1.0,
        revision,
        reusable,
    };

    assert_eq!(
        draw(&mut cache, revision(5, false), &[&first, &second])
            .statistics
            .gui_rebuilds,
        2
    );

    // A reusable revision promises unchanged inputs: the boxes are not hashed, so
    // even inputs that differ do not rebuild.
    let mut edited = second.clone();
    set_fill(&mut edited, 0.9);
    let reused = draw(&mut cache, revision(5, true), &[&first, &edited]);
    assert_eq!(
        (
            reused.statistics.gui_rebuilds,
            reused.statistics.uploaded_bytes
        ),
        (0, 0)
    );

    // A new revision hashes every box and rebuilds only the edited one.
    let rebuilt = draw(&mut cache, revision(6, false), &[&first, &edited]);
    assert_eq!(rebuilt.statistics.gui_rebuilds, 1);
    assert!(rebuilt.statistics.uploaded_bytes > 0);

    // Hashes from another revision are never reused.
    let mut again = edited.clone();
    set_fill(&mut again, 0.1);
    assert_eq!(
        draw(&mut cache, revision(7, true), &[&first, &again])
            .statistics
            .gui_rebuilds,
        1
    );
}

#[test]
fn root_replacement_prevents_reusing_a_node_batch() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity = ipp_core::EntityId::from_bits(1);
    let box_gen1 = sample_box_primitive(1, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];

    let mut stats1 = RenderFrameWork::default();
    cache
        .draw_box_batch(
            &1,
            entity,
            clip,
            SurfacePaint::UNKNOWN,
            &[&box_gen1],
            &mvp,
            &mut stats1,
        )
        .unwrap();
    assert_eq!(stats1.statistics.gui_allocations, 1);

    // The root is replaced: the same node identity under a new incarnation.
    let mut box_gen2 =
        sample_box_primitive(1, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    if let CanvasPrimitive::Box {
        style,
        ..
    } = &mut box_gen2
    {
        style.identity.target.incarnation = 2;
    }

    let mut stats2 = RenderFrameWork::default();
    cache
        .draw_box_batch(
            &1,
            entity,
            clip,
            SurfacePaint::UNKNOWN,
            &[&box_gen2],
            &mvp,
            &mut stats2,
        )
        .unwrap();

    // A new incarnation creates a new batch rather than reusing or overwriting stale handles
    assert_eq!(
        stats2.statistics.gui_allocations, 1,
        "new root incarnation requires new batch allocation"
    );
}

#[test]
fn finish_frame_prunes_unreferenced_batches_and_tracks_resident_bytes() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity1 = ipp_core::EntityId::from_bits(1);
    let entity2 = ipp_core::EntityId::from_bits(2);
    let box1 = sample_box_primitive(1, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    let box2 = sample_box_primitive(2, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];

    let mut stats = RenderFrameWork::default();
    cache
        .draw_box_batch(
            &1,
            entity1,
            clip,
            SurfacePaint::UNKNOWN,
            &[&box1],
            &mvp,
            &mut stats,
        )
        .unwrap();
    cache
        .draw_box_batch(
            &1,
            entity2,
            clip,
            SurfacePaint::UNKNOWN,
            &[&box2],
            &mvp,
            &mut stats,
        )
        .unwrap();

    let mut live = std::collections::BTreeSet::new();
    live.insert(entity1);
    // entity2 is destroyed and removed from live surfaces

    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &live,
    }));

    assert_eq!(
        device.borrow().deleted_batches,
        [2],
        "destroyed entity storage must be freed on GPU"
    );
    // A quad, the room its slot reserves to grow and room for appended work.
    let storage_bytes =
        device.borrow().created_batches[0].1 * std::mem::size_of::<GuiShapeRecord>();
    assert_eq!(
        storage_bytes,
        (1 + 4 + 1) * std::mem::size_of::<GuiShapeRecord>()
    );
    assert_eq!(cache.resident_bytes(), storage_bytes);
}

#[test]
fn culled_surfaces_keep_retained_batches_and_incomplete_frames_prune_nothing() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let shown = ipp_core::EntityId::from_bits(1);
    let culled = ipp_core::EntityId::from_bits(2);
    let panel = sample_box_primitive(1, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];
    let draw = |cache: &mut GuiBatchRenderCache<MockGuiDevice>, entity, stats: &mut _| {
        cache
            .draw_box_batch(
                &1,
                entity,
                clip,
                SurfacePaint::UNKNOWN,
                &[&panel],
                &mvp,
                stats,
            )
            .unwrap();
    };
    let live = BTreeSet::from([shown, culled]);

    let mut cold = RenderFrameWork::default();
    draw(&mut cache, shown, &mut cold);
    draw(&mut cache, culled, &mut cold);
    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &live,
    }));
    assert_eq!(device.borrow().created_batches.len(), 2);

    // One frame outside the frustum: the culled Surface is live but never submitted.
    draw(&mut cache, shown, &mut RenderFrameWork::default());
    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &BTreeSet::from([shown]),
    }));

    // A failed or cameraless frame cannot judge any key, even for destroyed Surfaces.
    cache.finish_frame(None);
    assert!(device.borrow().deleted_batches.is_empty());

    let mut visible_again = RenderFrameWork::default();
    draw(&mut cache, culled, &mut visible_again);
    assert_eq!(visible_again.statistics.uploaded_bytes, 0);
    assert_eq!(visible_again.statistics.gui_rebuilds, 0);
    assert_eq!(visible_again.statistics.gui_allocations, 0);
    assert_eq!(visible_again.statistics.gui_batches, 1);
    assert_eq!(device.borrow().created_batches.len(), 2);
    assert_eq!(device.borrow().writes.len(), 2);

    // Submitting a Surface without the work it retained releases that storage.
    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &BTreeSet::from([shown, culled]),
    }));
    assert_eq!(device.borrow().deleted_batches, [1]);
    assert_eq!(
        cache.resident_bytes(),
        (1 + 4 + 1) * std::mem::size_of::<GuiShapeRecord>()
    );
}

#[test]
fn failed_batch_replacement_releases_storage_instead_of_drawing_stale_vertices() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity = ipp_core::EntityId::from_bits(1);
    let panel = sample_box_primitive(1, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    let moved = sample_box_primitive(1, CanvasPart::Background, [0.5, 0.0], [1.0, 1.0], None);
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];
    let draw = |cache: &mut GuiBatchRenderCache<MockGuiDevice>, primitive| {
        cache.draw_box_batch(
            &1,
            entity,
            clip,
            SurfacePaint::UNKNOWN,
            &[primitive],
            &mvp,
            &mut RenderFrameWork::default(),
        )
    };

    draw(&mut cache, &panel).unwrap();
    device.borrow_mut().fail_writes = true;
    draw(&mut cache, &moved).unwrap();
    assert_eq!(device.borrow().deleted_batches, [1]);
    assert_eq!(cache.resident_bytes(), 0);
    assert_eq!(
        device.borrow().draws.len(),
        1,
        "stale storage is never drawn"
    );

    // The Surface backs off instead of paying the failing allocation every frame.
    device.borrow_mut().fail_writes = false;
    for _ in 1..STORAGE_RETRY_FRAMES {
        cache.finish_frame(None);
        draw(&mut cache, &moved).unwrap();
    }
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().draws.len(), 1);

    // Once the wait ends, the next frame allocates complete storage instead of
    // reusing the failed batch.
    cache.finish_frame(None);
    draw(&mut cache, &moved).unwrap();
    assert_eq!(device.borrow().created_batches.len(), 2);
    assert_eq!(device.borrow().draws.last().unwrap().batch, 2);
}

#[test]
fn repeated_storage_failures_double_the_retry_wait_until_a_commit_succeeds() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let entity = ipp_core::EntityId::from_bits(1);
    let panel = sample_box_primitive(1, CanvasPart::Background, [0.0, 0.0], [1.0, 1.0], None);
    let moved = sample_box_primitive(1, CanvasPart::Background, [0.5, 0.0], [1.0, 1.0], None);
    let mut stats = RenderFrameWork::default();
    let mut commit = |cache: &mut GuiBatchRenderCache<MockGuiDevice>, primitive| {
        cache.begin_surface(entity);
        cache.push_boxes(
            SurfacePaint::UNKNOWN,
            &[(primitive, [0.0, 0.0, 4.0, 2.0])],
            &mut stats,
        );
        let committed = cache.commit_surface(|_, _| &[], &mut stats).unwrap();
        cache.finish_frame(None);
        committed
    };

    // Record the frames on which a persistently failing Surface retries.
    device.borrow_mut().fail_writes = true;
    let mut attempts = Vec::new();
    for frame in 0..64 {
        let created = device.borrow().created_batches.len();
        assert!(!commit(&mut cache, &panel));
        if device.borrow().created_batches.len() > created {
            attempts.push(frame);
        }
    }
    assert_eq!(attempts, [0, 4, 12, 28, 60]);

    // A successful retry ends the back-off: the next failure waits the first interval.
    device.borrow_mut().fail_writes = false;
    while !commit(&mut cache, &panel) {}
    device.borrow_mut().fail_writes = true;
    assert!(!commit(&mut cache, &moved));
    device.borrow_mut().fail_writes = false;
    for _ in 1..STORAGE_RETRY_FRAMES {
        assert!(!commit(&mut cache, &moved));
    }
    assert!(commit(&mut cache, &moved));
}

/// Bytes of one filled box quad.
const BOX_BYTES: u32 = std::mem::size_of::<GuiShapeRecord>() as u32;

/// A long run of small filled boxes, one per GUI node.
fn box_run(nodes: std::ops::RangeInclusive<u32>) -> Vec<CanvasPrimitive> {
    nodes
        .map(|node| {
            let mut primitive = sample_box_primitive(
                node,
                CanvasPart::Background,
                [node as f32 * 0.01, 0.0],
                [0.005, 0.005],
                None,
            );
            if let CanvasPrimitive::Box {
                border_width,
                ..
            } = &mut primitive
            {
                *border_width = 0.0;
            }
            primitive
        })
        .collect()
}

fn set_fill(primitive: &mut CanvasPrimitive, red: f32) {
    if let CanvasPrimitive::Box {
        fill,
        ..
    } = primitive
    {
        *fill = CanvasShapeFill::Solid([red, 0.4, 0.6, 1.0]);
    }
}

/// Submit one complete frame of a single visible Surface.
fn draw_run_frame(
    cache: &mut GuiBatchRenderCache<MockGuiDevice>,
    boxes: &[CanvasPrimitive],
    clip: CanvasClip,
) -> RenderFrameWork {
    let entity = ipp_core::EntityId::from_bits(1);
    let refs: Vec<_> = boxes.iter().collect();
    let mut stats = RenderFrameWork::default();
    cache
        .draw_box_batch(
            &1,
            entity,
            clip,
            SurfacePaint::UNKNOWN,
            &refs,
            &[0.0; 16],
            &mut stats,
        )
        .unwrap();
    let live = BTreeSet::from([entity]);
    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &live,
    }));
    stats
}

const RUN_CLIP: CanvasClip = [0.0, 0.0, 8.0, 2.0];

#[test]
fn large_runs_split_into_bounded_batches_at_identity_boundaries() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let boxes = box_run(1..=400);

    let cold = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert!(cold.statistics.gui_batches >= 4, "{cold:?}");
    assert_eq!(
        cold.summary.draw_calls, 1,
        "one Surface storage draws as one range"
    );
    assert_eq!(cold.statistics.uploaded_bytes, 400 * BOX_BYTES);
    assert_eq!(
        device.borrow().writes.len(),
        cold.statistics.gui_batches as usize
    );
    assert!(
        device
            .borrow()
            .writes
            .iter()
            .all(|&(_, _, records)| records <= MAX_BATCH_BOXES)
    );

    let warm = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!(
        (
            warm.statistics.uploaded_bytes,
            warm.statistics.gui_allocations
        ),
        (0, 0)
    );
    assert_eq!(warm.statistics.gui_batches, cold.statistics.gui_batches);
}

#[test]
fn early_box_edits_insertions_and_removals_rebuild_only_nearby_batches() {
    // Boxes of the first `count` batches, written one per batch by a cold frame.
    let first_batches = |device: &Rc<RefCell<MockGuiDevice>>, count: usize| {
        device.borrow().writes[..count]
            .iter()
            .map(|&(_, _, records)| records as u32)
            .sum::<u32>()
    };

    // Resizing the first box rewrites only its own batch. Batch boundaries
    // follow identity hashes; this run's first batch holds more than a batch
    // minimum, so the boxes left behind stay one batch.
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let mut boxes = box_run(2..=401);
    let cold = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    let written = device.borrow().writes.len();
    if let CanvasPrimitive::Box {
        size,
        ..
    } = &mut boxes[0]
    {
        *size = [0.008, 0.008];
    }
    let resized = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!(resized.statistics.gui_rebuilds, 1);
    assert!(
        resized.statistics.uploaded_bytes <= first_batches(&device, 1) * BOX_BYTES,
        "{resized:?}"
    );
    assert!(device.borrow().writes.len() - written <= 2);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert!(resized.statistics.gui_batches <= cold.statistics.gui_batches + 1);

    // Inserting a box before the first one rewrites at most the batches around it.
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let mut boxes = box_run(1..=400);
    draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    let written = device.borrow().writes.len();
    let bound = (first_batches(&device, 2) + 1) * BOX_BYTES;
    boxes.insert(0, box_run(1000..=1000).remove(0));
    let inserted = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!(inserted.statistics.gui_rebuilds, 1);
    assert!(inserted.statistics.uploaded_bytes <= bound, "{inserted:?}");
    assert!(device.borrow().writes.len() - written <= 2);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert!(device.borrow().deleted_batches.is_empty());

    // Removing an early box likewise leaves every later batch untouched.
    boxes.remove(3);
    let written = device.borrow().writes.len();
    let removed = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert!(removed.statistics.uploaded_bytes <= bound, "{removed:?}");
    assert!(device.borrow().writes.len() - written <= 2);
}

#[test]
fn animating_one_box_in_a_large_run_uploads_only_that_box_each_frame() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let mut boxes = box_run(1..=400);
    let cold = draw_run_frame(&mut cache, &boxes, RUN_CLIP);

    // The first animated frame moves the box out of its batch: bounded by that batch.
    set_fill(&mut boxes[200], 0.0);
    let split = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert!(
        split.statistics.uploaded_bytes <= (MAX_BATCH_BOXES as u32 + 1) * BOX_BYTES,
        "{split:?}"
    );
    assert!(split.statistics.gui_batches <= cold.statistics.gui_batches + 2);

    // Every later frame replaces only the animated box's own batch.
    for frame in 1..=30 {
        set_fill(&mut boxes[200], frame as f32 / 30.0);
        let animated = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
        assert_eq!(
            animated.statistics.uploaded_bytes, BOX_BYTES,
            "frame {frame}"
        );
        assert_eq!(
            (
                animated.statistics.gui_rebuilds,
                animated.statistics.gui_allocations
            ),
            (1, 1)
        );
        assert_eq!(
            animated.statistics.gui_batches,
            split.statistics.gui_batches
        );
        assert_eq!(animated.summary.draw_calls, 1);
    }

    // At rest the box rejoins its neighbours once, after which frames upload nothing.
    let mut rest_bytes = 0;
    for _ in 0..VOLATILE_FRAMES + 2 {
        rest_bytes += draw_run_frame(&mut cache, &boxes, RUN_CLIP)
            .statistics
            .uploaded_bytes;
    }
    assert!(rest_bytes <= (MAX_BATCH_BOXES as u32 + 1) * BOX_BYTES);
    let settled = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!(settled.statistics.uploaded_bytes, 0);
    assert_eq!(settled.statistics.gui_batches, cold.statistics.gui_batches);
    assert_eq!(device.borrow().created_batches.len(), 1);
}

#[test]
fn clip_changes_rewrite_only_the_boxes_they_clip() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let boxes = box_run(1..=40);
    draw_run_frame(&mut cache, &boxes, RUN_CLIP);

    // Scrolling a container changes the clip its boxes carry in every record.
    let scrolled = [0.5, 0.0, 3.0, 1.0];
    let stats = draw_run_frame(&mut cache, &boxes, scrolled);
    assert_eq!(stats.statistics.gui_rebuilds, 40);
    assert_eq!(stats.statistics.uploaded_bytes, 40 * BOX_BYTES);
    assert_eq!(device.borrow().created_batches.len(), 1);
    let drawn = device.borrow().draws.last().unwrap().records.clone();
    assert!(
        drawn
            .shapes()
            .iter()
            .all(|record| *record == GuiShapeRecord::EMPTY || record.clip == scrolled)
    );

    // Clipping one box of the run differently rewrites only that box's batch.
    let refs: Vec<_> = boxes.iter().collect();
    let mut clipped: Vec<_> = refs
        .iter()
        .map(|&primitive| (primitive, scrolled))
        .collect();
    clipped[20].1 = [1.0, 0.0, 2.0, 1.0];
    let mut stats = RenderFrameWork::default();
    cache.begin_surface(ipp_core::EntityId::from_bits(1));
    cache.push_boxes(SurfacePaint::UNKNOWN, &clipped, &mut stats);
    cache.commit_surface(|_, _| &[], &mut stats).unwrap();
    assert_eq!(stats.statistics.gui_rebuilds, 1);
    assert!(
        stats.statistics.uploaded_bytes <= (MAX_BATCH_BOXES as u32 + 1) * BOX_BYTES,
        "{stats:?}"
    );
}

/// Glyph records of one test text batch: `count` quads from `x` along a row at `y`,
/// tinted and clipped.
fn glyph_quads(count: usize, x: f32, y: f32, clip: CanvasClip) -> Vec<GuiGlyphRecord> {
    (0..count)
        .map(|index| {
            let left = x + index as f32 * 0.01;
            GuiGlyphRecord {
                rect: [left, y, left + 0.008, y + 0.01],
                uv: [0.5, 0.5, 0.6, 0.6],
                color: [1.0; 4],
                clip,
            }
        })
        .collect()
}

/// Paint bounds of glyph records within their clip, as text batches carry them.
fn glyph_bounds(records: &[GuiGlyphRecord]) -> Option<[f32; 4]> {
    records
        .iter()
        .filter_map(|record| super::clipped_bounds(record.rect, record.clip))
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
}

/// One Surface's painter-order GUI work: box runs and text batches.
enum TestWork {
    Boxes(Vec<(CanvasPrimitive, CanvasClip)>),
    Text {
        node: u32,
        page: usize,
        revision: u64,
        records: Vec<GuiGlyphRecord>,
    },
}

fn text_identity(node: u32) -> CanvasPrimitiveId {
    CanvasPrimitiveId {
        target: ipp_core::systems::canvas::CanvasTarget {
            entity: ipp_core::EntityId::from_bits(u64::from(node)),
            component: ipp_core::ComponentValue::CANVAS_BOX,
            incarnation: 1,
        },
        part: CanvasPart::Label,
    }
}

/// One submitted Surface: its stats, its draws and the shapes and glyphs its work
/// should paint, each in painter order.
struct SubmittedWork {
    stats: RenderFrameWork,
    draws: Vec<MockDraw>,
    shapes: Vec<GuiShapeRecord>,
    glyphs: Vec<GuiGlyphRecord>,
}

impl SubmittedWork {
    /// Draws in order: `S` for shapes through the shape program without an atlas,
    /// `G` for glyphs through the glyph program with one.
    fn kinds(&self) -> String {
        self.draws
            .iter()
            .map(|draw| match (&draw.records, draw.program, draw.atlas) {
                (MockRecords::Shapes(_), SHAPE_PROGRAM, false) => 'S',
                (MockRecords::Glyphs(_), GLYPH_PROGRAM, true) => 'G',
                _ => '?',
            })
            .collect()
    }

    /// Drawn shape records in draw order, without the empty room between slots.
    fn drawn_shapes(&self) -> Vec<GuiShapeRecord> {
        self.draws
            .iter()
            .flat_map(|draw| draw.records.shapes().iter().copied())
            .filter(|record| *record != GuiShapeRecord::EMPTY)
            .collect()
    }

    /// Drawn glyph records in draw order, without the empty room between slots.
    fn drawn_glyphs(&self) -> Vec<GuiGlyphRecord> {
        self.draws
            .iter()
            .flat_map(|draw| draw.records.glyphs().iter().copied())
            .filter(|record| *record != GuiGlyphRecord::EMPTY)
            .collect()
    }
}

/// Submit `work` as one Surface and draw it.
fn submit_work(
    cache: &mut GuiBatchRenderCache<MockGuiDevice>,
    device: &Rc<RefCell<MockGuiDevice>>,
    work: &[TestWork],
) -> SubmittedWork {
    let mut stats = RenderFrameWork::default();
    let entity = ipp_core::EntityId::from_bits(1);
    cache.begin_surface(entity);
    let mut shapes = Vec::new();
    let mut glyphs = Vec::new();
    let mut text: BTreeMap<CanvasPrimitiveId, &[GuiGlyphRecord]> = BTreeMap::new();
    for item in work {
        match item {
            TestWork::Boxes(boxes) => {
                let refs: Vec<_> = boxes
                    .iter()
                    .map(|(primitive, clip)| (primitive, *clip))
                    .collect();
                cache.push_boxes(SurfacePaint::UNKNOWN, &refs, &mut stats);
                for (primitive, clip) in boxes {
                    let CanvasPrimitive::Box {
                        style,
                        size,
                        corner_radius,
                        border_width,
                        border_color,
                        fill,
                        glow,
                        shape,
                    } = primitive
                    else {
                        unreachable!();
                    };
                    shapes.extend(generate_box_records(
                        style,
                        size,
                        corner_radius,
                        *border_width,
                        border_color,
                        fill,
                        glow.as_ref(),
                        shape,
                        *clip,
                    ));
                }
            }
            TestWork::Text {
                node,
                page,
                revision,
                records,
            } => {
                let identity = text_identity(*node);
                cache.push_glyphs([GuiPiece {
                    key: GuiPieceKey::Glyphs(identity, 0),
                    hash: *revision,
                    len: records.len(),
                    page: Some(*page),
                    bounds: glyph_bounds(records),
                    source: GuiPieceSource::Glyphs(identity, 0),
                }]);
                text.insert(identity, records);
                glyphs.extend_from_slice(records);
            }
        }
    }

    cache
        .commit_surface(|identity, _| text[&identity], &mut stats)
        .unwrap();
    let before = device.borrow().draws.len();
    let pieces = cache.piece_count();
    let textures = [10, 11];
    cache
        .draw_pieces(
            &SHAPE_PROGRAM,
            &GLYPH_PROGRAM,
            0..pieces,
            DOMAIN,
            |page| textures.get(page),
            &[0.0; 16],
            &mut stats,
        )
        .unwrap();
    let draws = device.borrow().draws[before..].to_vec();
    SubmittedWork {
        stats,
        draws,
        shapes,
        glyphs,
    }
}

/// A row of three small boxes from `node`, along the top of the canvas.
fn row(node: u32, clip: CanvasClip) -> TestWork {
    TestWork::Boxes(
        box_run(node..=node + 2)
            .into_iter()
            .map(|primitive| (primitive, clip))
            .collect(),
    )
}

/// A three-glyph label of `node` at `x` on atlas `page`, below the boxes of [`row`].
fn label(node: u32, page: usize, x: f32, clip: CanvasClip) -> TestWork {
    TestWork::Text {
        node,
        page,
        revision: u64::from(node),
        records: glyph_quads(3, x, 0.5, clip),
    }
}

#[test]
fn a_panel_of_interleaved_backgrounds_and_labels_draws_its_shapes_then_its_glyphs() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let clips = [[0.0, 0.0, 1.0, 1.0], [0.2, 0.0, 0.8, 1.0]];

    // Box, text, box and text runs of one page under alternating clips: two draws.
    let work = [
        row(1, clips[0]),
        label(100, 0, 0.25, clips[1]),
        row(10, clips[1]),
        label(101, 0, 0.6, clips[0]),
    ];
    let submitted = submit_work(&mut cache, &device, &work);
    assert_eq!(submitted.kinds(), "SG", "{:?}", submitted.stats);
    assert_eq!(submitted.stats.summary.draw_calls, 2);
    assert_eq!(submitted.stats.statistics.gui_batches, 4);
    assert_eq!(submitted.stats.summary.triangles, 2 * (6 + 6));
    assert_eq!(submitted.drawn_shapes(), submitted.shapes);
    assert_eq!(submitted.drawn_glyphs(), submitted.glyphs);

    // Text on a second page splits the glyphs, never the shapes.
    let work = [
        row(1, clips[0]),
        label(100, 0, 0.25, clips[1]),
        label(102, 1, 0.4, clips[1]),
        row(10, clips[1]),
    ];
    let submitted = submit_work(&mut cache, &device, &work);
    assert_eq!(submitted.kinds(), "SGG");
    assert_eq!(submitted.drawn_shapes(), submitted.shapes);
    assert_eq!(submitted.drawn_glyphs(), submitted.glyphs);
}

#[test]
fn a_shape_painted_over_earlier_text_cuts_the_run_and_draws_after_that_text() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let clip = [0.0, 0.0, 8.0, 2.0];
    // A dialog over the first label, then the dialog's own title on top of it.
    let dialog = sample_box_primitive(50, CanvasPart::Background, [0.0, 0.4], [1.0, 0.3], None);
    let work = [
        row(1, clip),
        label(100, 0, 0.25, clip),
        TestWork::Boxes(vec![(dialog, clip)]),
        label(101, 0, 0.6, clip),
        row(10, clip),
    ];
    let submitted = submit_work(&mut cache, &device, &work);
    assert_eq!(submitted.kinds(), "SGSG", "{:?}", submitted.stats);
    assert_eq!(submitted.drawn_shapes(), submitted.shapes);
    assert_eq!(submitted.drawn_glyphs(), submitted.glyphs);

    // The label beneath the dialog draws before it and the dialog's title after it.
    let glyphs_of = |index: usize| submitted.draws[index].records.glyphs().to_vec();
    assert!(glyphs_of(1).contains(&submitted.glyphs[0]));
    assert!(
        submitted.draws[2].records.shapes()[..1]
            .iter()
            .all(|record| record.placement == [0.0, 0.4, 1.0, 0.3])
    );
    assert!(glyphs_of(3).contains(&submitted.glyphs[3]));

    // Without the overlap the same work is two draws again.
    let moved = sample_box_primitive(50, CanvasPart::Background, [2.0, 0.4], [1.0, 0.3], None);
    let work = [
        row(1, clip),
        label(100, 0, 0.25, clip),
        TestWork::Boxes(vec![(moved, clip)]),
        label(101, 0, 0.6, clip),
        row(10, clip),
    ];
    assert_eq!(submit_work(&mut cache, &device, &work).kinds(), "SG");
}

#[test]
fn text_edits_write_only_glyph_storage_and_shape_changes_only_shape_storage() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let clip = [0.0, 0.0, 8.0, 2.0];
    let live = BTreeSet::from([ipp_core::EntityId::from_bits(1)]);
    let mut submit = |work: &[TestWork]| {
        let written = device.borrow().writes.len();
        let submitted = submit_work(&mut cache, &device, work);
        cache.finish_frame(Some(&RetainedSurfaceSubmission {
            live: &live,
            submitted: &live,
        }));
        let writes = device.borrow().writes[written..].to_vec();
        (submitted, writes)
    };
    let panel = |fill: f32, revision: u64| {
        let mut boxes = box_run(1..=3);
        set_fill(&mut boxes[1], fill);
        vec![
            TestWork::Boxes(
                boxes
                    .into_iter()
                    .map(|primitive| (primitive, clip))
                    .collect(),
            ),
            TestWork::Text {
                node: 100,
                page: 0,
                revision,
                records: glyph_quads(5, 0.2, 0.5, clip),
            },
        ]
    };

    let (cold, writes) = submit(&panel(0.2, 1));
    assert_eq!(cold.kinds(), "SG");
    let storage = |kind: GuiRecordKind| {
        device
            .borrow()
            .contents
            .iter()
            .find(|(_, records)| {
                matches!(
                    (kind, records),
                    (GuiRecordKind::Shape, MockRecords::Shapes(_))
                        | (GuiRecordKind::Glyph, MockRecords::Glyphs(_))
                )
            })
            .map(|(id, _)| *id)
            .expect("live storage of the kind")
    };
    let (shapes, glyphs) = (storage(GuiRecordKind::Shape), storage(GuiRecordKind::Glyph));
    assert_eq!(writes.len(), 2);

    // Retexting the label writes its glyph records and nothing of the shapes.
    let (edited, writes) = submit(&panel(0.2, 2));
    assert!(!writes.is_empty());
    assert!(writes.iter().all(|&(id, _, _)| id == glyphs), "{writes:?}");
    assert_eq!(
        edited.stats.statistics.uploaded_bytes,
        5 * std::mem::size_of::<GuiGlyphRecord>() as u32
    );

    // A hovered box's new fill writes shape records and nothing of the text.
    let (hovered, writes) = submit(&panel(0.9, 2));
    assert!(!writes.is_empty());
    assert!(writes.iter().all(|&(id, _, _)| id == shapes), "{writes:?}");
    assert_eq!(hovered.stats.statistics.uploaded_bytes % BOX_BYTES, 0);

    // An unchanged frame writes nothing at all.
    let (warm, writes) = submit(&panel(0.9, 2));
    assert!(writes.is_empty());
    assert_eq!(warm.stats.statistics.uploaded_bytes, 0);
}

#[test]
fn drawn_ranges_hold_exactly_the_painter_order_work_across_edits() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let clip = [0.0, 0.0, 8.0, 2.0];

    // Deterministic edits: resize, recolour, insert, remove and retext, including runs
    // that outgrow their slots and force new storage.
    let mut seed = 0x2545_f491_u64;
    let mut next = move |bound: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % bound
    };
    let mut boxes = box_run(1..=120);
    let mut glyphs = [4usize, 9, 2];
    let mut revisions = [1u64, 2, 3];
    let mut fresh = 5000;
    for frame in 0..200 {
        match next(6) {
            0 => {
                let index = next(boxes.len() as u64) as usize;
                set_fill(&mut boxes[index], frame as f32 / 200.0);
            }
            1 if boxes.len() < 300 => {
                let index = next(boxes.len() as u64 + 1) as usize;
                fresh += 1;
                let count = 1 + next(12) as u32;
                boxes.splice(index..index, box_run(fresh..=fresh + count - 1));
                fresh += count;
            }
            2 if boxes.len() > 10 => {
                let index = next(boxes.len() as u64 - 5) as usize;
                boxes.drain(index..index + 1 + next(5) as usize);
            }
            3 => {
                let run = next(3) as usize;
                glyphs[run] = 1 + next(40) as usize;
                revisions[run] += 10;
            }
            _ => {}
        }

        // The boxes run along the top of the canvas and the text below them, so no
        // shape covers earlier text and every frame is two draws.
        let thirds = boxes.len() / 3;
        let with_clip = |range: std::ops::Range<usize>| {
            boxes[range]
                .iter()
                .map(|primitive| (primitive.clone(), clip))
                .collect()
        };
        let text = |run: usize| TestWork::Text {
            node: 100 + run as u32,
            page: 0,
            revision: revisions[run],
            records: glyph_quads(glyphs[run], run as f32, 0.5, clip),
        };
        let work = [
            TestWork::Boxes(with_clip(0..thirds)),
            text(0),
            TestWork::Boxes(with_clip(thirds..2 * thirds)),
            text(1),
            text(2),
            TestWork::Boxes(with_clip(2 * thirds..boxes.len())),
        ];
        let submitted = submit_work(&mut cache, &device, &work);
        assert_eq!(submitted.kinds(), "SG", "frame {frame}");
        assert!(
            submitted.drawn_shapes() == submitted.shapes,
            "frame {frame}: drawn shapes differ"
        );
        assert!(
            submitted.drawn_glyphs() == submitted.glyphs,
            "frame {frame}: drawn glyphs differ"
        );

        let live = BTreeSet::from([ipp_core::EntityId::from_bits(1)]);
        cache.finish_frame(Some(&RetainedSurfaceSubmission {
            live: &live,
            submitted: &live,
        }));
    }

    assert_eq!(
        device.borrow().contents.len(),
        2,
        "replaced storage is released: one shape and one glyph storage remain"
    );
}

/// Upload bytes of one glyph quad.
const GLYPH_QUAD_BYTES: u32 = std::mem::size_of::<GuiGlyphRecord>() as u32;

/// The retained-gui benchmark terminal: one text piece of 36 glyph quads per row, 12
/// rows, each row's revision changing when its text changes.
fn terminal_work(revisions: &[u64; 12]) -> Vec<TestWork> {
    let clip = [0.0, 0.0, 8.0, 2.0];
    revisions
        .iter()
        .enumerate()
        .map(|(row, &revision)| TestWork::Text {
            node: 100 + row as u32,
            page: 0,
            revision,
            records: glyph_quads(36, 0.0, row as f32 * 0.1, clip),
        })
        .collect()
}

#[test]
fn terminal_updates_upload_exactly_the_changed_rows_glyph_records() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let live = BTreeSet::from([ipp_core::EntityId::from_bits(1)]);
    let submit = |cache: &mut GuiBatchRenderCache<MockGuiDevice>, revisions: &[u64; 12]| {
        let stats = submit_work(cache, &device, &terminal_work(revisions)).stats;
        cache.finish_frame(Some(&RetainedSurfaceSubmission {
            live: &live,
            submitted: &live,
        }));
        stats
    };

    // The 64-byte glyph record makes a 12x36 screen 432 records, 27,648 bytes.
    assert_eq!(std::mem::size_of::<GuiGlyphRecord>(), 64);
    let screen_bytes = 12 * 36 * GLYPH_QUAD_BYTES;
    assert_eq!(screen_bytes, 27_648);

    let mut revisions = [1u64; 12];
    let cold = submit(&mut cache, &revisions);
    assert_eq!(cold.statistics.uploaded_bytes, screen_bytes);
    assert_eq!(cold.summary.draw_calls, 1);
    let storages = device.borrow().created_batches.len();

    let warm = submit(&mut cache, &revisions);
    assert_eq!(warm.statistics.uploaded_bytes, 0);

    // Scrolling or replacing the screen retexts every row at the same length: every
    // row rewrites in place and nothing beyond the glyph records is uploaded.
    for revision in &mut revisions {
        *revision += 1;
    }
    let full = submit(&mut cache, &revisions);
    assert_eq!(full.statistics.uploaded_bytes, screen_bytes);
    assert_eq!(full.statistics.gui_allocations, 12);
    assert_eq!(device.borrow().created_batches.len(), storages);

    // Typing retexts one row.
    revisions[5] += 1;
    let typed = submit(&mut cache, &revisions);
    assert_eq!(typed.statistics.uploaded_bytes, 36 * GLYPH_QUAD_BYTES);
    assert_eq!(typed.statistics.uploaded_bytes, 2_304);
    assert_eq!(typed.statistics.gui_allocations, 1);
    assert_eq!(device.borrow().created_batches.len(), storages);
}
