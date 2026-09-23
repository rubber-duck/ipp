//! Tests for retained GUI triangle batches and their per-Surface GPU storage.

use std::cell::RefCell;
use std::rc::Rc;

use std::collections::{BTreeMap, BTreeSet};

use super::super::gui_storage::{GuiPiece, GuiPieceKey, GuiPieceSource};
use super::super::retained_surfaces::SurfacePaint;
use super::{
    GUI_FILL_GLYPH, GuiBatchRenderCache, GuiVertex, MAX_BATCH_BOXES, RetainedSurfaceSubmission,
    VOLATILE_FRAMES, generate_box_vertices,
};
use crate::{RenderDevice, RenderError, RenderStats};
use ipp_core::systems::gui::GuiNodeId;
use ipp_core::systems::surface::{
    GuiPrimitiveId, GuiPrimitivePart, GuiShapeFill, GuiShapeGlow, SurfaceClipRect,
    SurfacePrimitiveIdentity, SurfacePrimitiveStyle, SurfaceRenderPrimitive,
};

#[derive(Default)]
struct MockGuiDevice {
    /// `(id, capacity)` of every storage allocation.
    created_batches: Vec<(usize, usize)>,
    /// `(id, first, len)` of every storage write.
    writes: Vec<(usize, usize, usize)>,
    deleted_batches: Vec<usize>,
    /// Drawn vertices of every draw, and whether it bound an atlas.
    draws: Vec<(usize, Vec<GuiVertex>, bool)>,
    /// Current contents of every live storage allocation.
    contents: BTreeMap<usize, Vec<GuiVertex>>,
    next_id: usize,
    fail_writes: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct MockGuiBatch {
    id: usize,
}

impl RenderDevice for MockGuiDevice {
    type Program = u32;
    type Mesh = u32;
    type Texture = u32;
    type SurfacePath = u32;
    type SurfaceCacheTarget = ();
    type SurfaceInstances = ();
    #[cfg(feature = "shadows")]
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

    #[cfg(feature = "shadows")]
    fn create_shadow_map(&mut self, _size: u32) -> Result<Self::ShadowMap, RenderError> {
        Ok(0)
    }

    #[cfg(feature = "shadows")]
    fn begin_shadow(
        &mut self,
        _map: &Self::ShadowMap,
        _slot: u32,
        _grid: u32,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn end_shadow(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn bind_shadow(
        &mut self,
        _program: &Self::Program,
        _map: &Self::ShadowMap,
        _frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
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
        #[cfg(feature = "mesh-poses")] _pose: Option<(&Self::Mesh, f32)>,
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

    fn create_gui_batch(&mut self, capacity: usize) -> Result<Self::GuiBatch, RenderError> {
        self.next_id += 1;
        let id = self.next_id;
        self.created_batches.push((id, capacity));
        self.contents.insert(id, vec![GuiVertex::EMPTY; capacity]);
        Ok(MockGuiBatch {
            id,
        })
    }

    fn write_gui_batch(
        &mut self,
        batch: &mut Self::GuiBatch,
        first: usize,
        vertices: &[GuiVertex],
    ) -> Result<(), RenderError> {
        if self.fail_writes {
            return Err(RenderError::RenderDevice("injected write failure".into()));
        }

        self.writes.push((batch.id, first, vertices.len()));
        self.contents.get_mut(&batch.id).expect("live storage")[first..first + vertices.len()]
            .copy_from_slice(vertices);
        Ok(())
    }

    fn delete_gui_batch(&mut self, batch: Self::GuiBatch) {
        self.contents.remove(&batch.id);
        self.deleted_batches.push(batch.id);
    }

    fn draw_gui_batch(
        &mut self,
        _program: &Self::Program,
        batch: &Self::GuiBatch,
        atlas: Option<&Self::Texture>,
        _mvp: &[f32; 16],
        first: usize,
        count: usize,
    ) -> Result<(), RenderError> {
        let drawn = self.contents[&batch.id][first..first + count].to_vec();
        self.draws.push((batch.id, drawn, atlas.is_some()));
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
    lifetime: u32,
    part: GuiPrimitivePart,
    position: [f32; 2],
    size: [f32; 2],
    clip: Option<SurfaceClipRect>,
) -> SurfaceRenderPrimitive {
    SurfaceRenderPrimitive::Box {
        style: SurfacePrimitiveStyle {
            identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                root_incarnation: 1,
                node: GuiNodeId(node),
                lifetime,
                part,
            }),
            position,
            scale: [1.0, 1.0],
            color: [0.2, 0.4, 0.6, 1.0],
            opacity: 1.0,
            clip,
        },
        size,
        corner_radius: [0.05, 0.05],
        border_width: 0.01,
        border_color: [0.8, 0.8, 0.8, 1.0],
        fill: GuiShapeFill::Solid([0.2, 0.4, 0.6, 1.0]),
        glow: None,
    }
}

/// Clip of generated test geometry.
const CLIP: SurfaceClipRect = [-100.0, -100.0, 100.0, 100.0];

/// One Surface submission whose boxes all share a clip, as the service performs it.
trait DrawBoxBatch {
    #[allow(clippy::too_many_arguments)]
    fn draw_box_batch(
        &mut self,
        program: &u32,
        entity: ipp_core::EntityId,
        clip: SurfaceClipRect,
        paint: SurfacePaint,
        boxes: &[&SurfaceRenderPrimitive],
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError>;
}

impl DrawBoxBatch for GuiBatchRenderCache<MockGuiDevice> {
    fn draw_box_batch(
        &mut self,
        program: &u32,
        entity: ipp_core::EntityId,
        clip: SurfaceClipRect,
        paint: SurfacePaint,
        boxes: &[&SurfaceRenderPrimitive],
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        let clipped: Vec<_> = boxes.iter().map(|&primitive| (primitive, clip)).collect();
        self.begin_surface(entity);
        self.push_boxes(paint, &clipped, stats);
        self.commit_surface(|_, _| &[], stats)?;
        let pieces = self.piece_count();
        self.draw_pieces(program, 0..pieces, |_| None, mvp, stats)
    }
}

#[test]
fn box_vertices_form_two_counter_clockwise_triangles_per_quad() {
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: GuiNodeId(1),
            lifetime: 1,
            part: GuiPrimitivePart::Background,
        }),
        position: [1.0, 2.0],
        scale: [1.0, 1.0],
        color: [1.0, 0.0, 0.0, 1.0],
        opacity: 1.0,
        clip: None,
    };
    let size = [3.0, 4.0];
    let corner = [0.1, 0.2];
    let border_color = [0.0, 1.0, 0.0, 1.0];
    let fill = GuiShapeFill::Solid([1.0, 0.0, 0.0, 1.0]);

    let vertices = generate_box_vertices(
        &style,
        &size,
        &corner,
        0.05,
        &border_color,
        &fill,
        None,
        CLIP,
    );
    assert_eq!(vertices.len(), 6);

    // Quad corners in Surface coordinates with 0.002 conservative AA padding:
    // x0 = 1.0 - 0.002 = 0.998, y0 = 2.0 - 0.002 = 1.998
    // x1 = 4.0 + 0.002 = 4.002, y1 = 6.0 + 0.002 = 6.002
    assert_eq!(vertices[0].position, [0.998, 1.998]); // TL
    assert_eq!(vertices[1].position, [0.998, 6.002]); // BL
    assert_eq!(vertices[2].position, [4.002, 6.002]); // BR

    assert_eq!(vertices[3].position, [0.998, 1.998]); // TL
    assert_eq!(vertices[4].position, [4.002, 6.002]); // BR
    assert_eq!(vertices[5].position, [4.002, 1.998]); // TR

    // All vertices carry identical shape uniforms
    for v in &vertices {
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
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: GuiNodeId(1),
            lifetime: 1,
            part: GuiPrimitivePart::Background,
        }),
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 0.5,
        clip: None,
    };
    let fill = GuiShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [2.0, 1.0],
        start_color: [1.0, 0.0, 0.0, 1.0],
        end_color: [0.0, 0.0, 1.0, 0.8],
    };

    let vertices = generate_box_vertices(
        &style,
        &[2.0, 1.0],
        &[0.0, 0.0],
        0.0,
        &[0.0; 4],
        &fill,
        None,
        CLIP,
    );
    assert_eq!(vertices.len(), 6);

    for v in &vertices {
        assert_eq!(v.color0, [1.0, 0.0, 0.0, 0.5]);
        assert_eq!(v.color1, [0.0, 0.0, 1.0, 0.4]);
        assert_eq!(v.gradient_coords, [0.0, 0.0, 2.0, 1.0]);
        assert_eq!(v.material_params[0], 1.0);
    }
}

#[test]
fn radial_gradient_fill_sets_center_radius_and_material_type() {
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: GuiNodeId(1),
            lifetime: 1,
            part: GuiPrimitivePart::Background,
        }),
        position: [0.0, 0.0],
        scale: [2.0, 2.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: None,
    };
    let fill = GuiShapeFill::RadialGradient {
        center: [0.5, 0.5],
        radius: 0.25,
        start_color: [1.0, 1.0, 0.0, 1.0],
        end_color: [0.0, 1.0, 1.0, 0.0],
    };

    let vertices = generate_box_vertices(
        &style,
        &[1.0, 1.0],
        &[0.0, 0.0],
        0.0,
        &[0.0; 4],
        &fill,
        None,
        CLIP,
    );
    assert_eq!(vertices.len(), 6);

    for v in &vertices {
        assert_eq!(v.color0, [1.0, 1.0, 0.0, 1.0]);
        assert_eq!(v.color1, [0.0, 1.0, 1.0, 0.0]);
        assert_eq!(v.gradient_coords, [1.0, 1.0, 0.5, 0.0]);
        assert_eq!(v.material_params[0], 2.0);
    }
}

#[test]
fn glow_expands_quad_padding_and_packs_glow_parameters() {
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: GuiNodeId(1),
            lifetime: 1,
            part: GuiPrimitivePart::Background,
        }),
        position: [2.0, 3.0],
        scale: [1.0, 1.0],
        color: [0.0, 0.0, 0.0, 1.0],
        opacity: 0.8,
        clip: None,
    };
    let glow = GuiShapeGlow {
        color: [0.3, 0.6, 0.9, 1.0],
        intensity: 1.5,
        radius: 0.05,
        falloff: 2.0,
    };

    let vertices = generate_box_vertices(
        &style,
        &[1.0, 1.0],
        &[0.02, 0.02],
        0.0,
        &[0.0; 4],
        &GuiShapeFill::Solid([0.0; 4]),
        Some(&glow),
        CLIP,
    );
    assert_eq!(vertices.len(), 6);

    let expected_x0 = 2.0 - 0.052;
    let expected_y0 = 3.0 - 0.052;
    let expected_x1 = 3.0 + 0.052;
    let expected_y1 = 4.0 + 0.052;

    assert!((vertices[0].position[0] - expected_x0).abs() < 1e-6);
    assert!((vertices[0].position[1] - expected_y0).abs() < 1e-6);
    assert!((vertices[2].position[0] - expected_x1).abs() < 1e-6);
    assert!((vertices[2].position[1] - expected_y1).abs() < 1e-6);

    for v in &vertices {
        assert_eq!(v.material_params[1], 1.5);
        assert_eq!(v.material_params[2], 0.05);
        assert_eq!(v.material_params[3], 2.0);
        assert_eq!(v.glow_color, [0.3, 0.6, 0.9, 0.8]);
    }
}

#[test]
fn border_only_box_splits_into_four_edge_strips_without_interior() {
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: GuiNodeId(1),
            lifetime: 1,
            part: GuiPrimitivePart::Background,
        }),
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [0.0, 0.0, 0.0, 0.0],
        opacity: 1.0,
        clip: None,
    };
    let border_color = [1.0, 1.0, 1.0, 1.0];
    let size = [10.0, 10.0];
    let corner = [0.1, 0.1];
    let border_width = 0.2;

    let glow = GuiShapeGlow {
        color: [0.0, 1.0, 0.0, 1.0],
        intensity: 1.0,
        radius: 0.15,
        falloff: 2.0,
    };
    for glow in [None, Some(&glow)] {
        let vertices = generate_box_vertices(
            &style,
            &size,
            &corner,
            border_width,
            &border_color,
            &GuiShapeFill::Solid([0.0, 0.0, 0.0, 0.0]),
            glow,
            CLIP,
        );
        assert_eq!(
            vertices.len(),
            24,
            "large border-only box splits into 4 edge quads (24 vertices)"
        );

        for quad_idx in 0..4 {
            let q = &vertices[quad_idx * 6..(quad_idx + 1) * 6];
            assert_eq!(q[0].position, q[3].position);
            assert_eq!(q[2].position, q[4].position);
        }
    }
}

#[test]
fn small_border_only_box_uses_single_quad() {
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: GuiNodeId(1),
            lifetime: 1,
            part: GuiPrimitivePart::Background,
        }),
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [0.0, 0.0, 0.0, 0.0],
        opacity: 1.0,
        clip: None,
    };
    let border_color = [1.0, 1.0, 1.0, 1.0];
    let size = [0.3, 0.3];
    let corner = [0.1, 0.1];
    let border_width = 0.1;

    let vertices = generate_box_vertices(
        &style,
        &size,
        &corner,
        border_width,
        &border_color,
        &GuiShapeFill::Solid([0.0, 0.0, 0.0, 0.0]),
        None,
        CLIP,
    );
    assert_eq!(
        vertices.len(),
        6,
        "small border-only box uses a single quad to avoid strip overhead"
    );
}

#[test]
fn warm_frame_uploads_zero_geometry_bytes() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity = ipp_core::EntityId::from_bits(1);
    let box1 = sample_box_primitive(
        1,
        1,
        GuiPrimitivePart::Background,
        [0.0, 0.0],
        [1.0, 1.0],
        None,
    );
    let box2 = sample_box_primitive(
        2,
        1,
        GuiPrimitivePart::Background,
        [1.5, 0.0],
        [1.0, 1.0],
        None,
    );
    let boxes = [&box1, &box2];
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];

    // Frame 1: Cold upload
    let mut stats1 = RenderStats::default();
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

    assert_eq!(stats1.gui_allocations, 1);
    assert_eq!(stats1.gui_rebuilds, 2);
    assert_eq!(stats1.gui_batches, 1);
    assert_eq!(stats1.triangles, 4); // 2 boxes * 2 triangles
    let expected_bytes = 12 * std::mem::size_of::<GuiVertex>() as u32;
    assert_eq!(stats1.uploaded_bytes, expected_bytes);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().writes, [(1, 0, 12)]);

    // Frame 2: Warm unchanged frame
    let mut stats2 = RenderStats::default();
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

    assert_eq!(stats2.gui_allocations, 0);
    assert_eq!(stats2.gui_rebuilds, 0);
    assert_eq!(stats2.gui_batches, 1);
    assert_eq!(stats2.triangles, 4);
    assert_eq!(stats2.uploaded_bytes, 0, "warm frame must upload 0 bytes");
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().writes.len(), 1);
}

#[test]
fn local_change_replaces_batch_storage_and_rebuilds_only_affected_primitive() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity = ipp_core::EntityId::from_bits(1);
    let box1 = sample_box_primitive(
        1,
        1,
        GuiPrimitivePart::Background,
        [0.0, 0.0],
        [1.0, 1.0],
        None,
    );
    let box2 = sample_box_primitive(
        2,
        1,
        GuiPrimitivePart::Background,
        [1.5, 0.0],
        [1.0, 1.0],
        None,
    );
    let boxes_initial = [&box1, &box2];
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];

    let mut stats1 = RenderStats::default();
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
    assert_eq!(stats1.gui_rebuilds, 2);

    // Modify only box2 (e.g. hovered fill colour or slider thumb position)
    let mut box2_modified = box2.clone();
    if let SurfaceRenderPrimitive::Box {
        fill,
        ..
    } = &mut box2_modified
    {
        *fill = GuiShapeFill::Solid([1.0, 1.0, 0.0, 1.0]);
    }
    let boxes_modified = [&box1, &box2_modified];

    let mut stats2 = RenderStats::default();
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
        stats2.gui_rebuilds, 1,
        "only the modified primitive geometry rebuilds"
    );
    // The changed box turns volatile and leaves its stable neighbour's batch: that
    // batch rewrites its slot without it, and the changed box's own batch follows it
    // in the same storage, in one upload.
    assert_eq!(stats2.gui_allocations, 2);
    assert_eq!(device.borrow().writes[1..], [(1, 0, 12)]);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(stats2.draw_calls, 1);
    let expected_bytes = 12 * std::mem::size_of::<GuiVertex>() as u32;
    assert_eq!(stats2.uploaded_bytes, expected_bytes);
}

#[test]
fn colour_only_change_on_gradient_box_keeps_retained_geometry() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let mut panel = sample_box_primitive(
        1,
        1,
        GuiPrimitivePart::Background,
        [0.0, 0.0],
        [1.0, 1.0],
        None,
    );
    if let SurfaceRenderPrimitive::Box {
        fill,
        ..
    } = &mut panel
    {
        *fill = GuiShapeFill::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
            start_color: [0.1, 0.2, 0.3, 1.0],
            end_color: [0.3, 0.2, 0.1, 1.0],
        };
    }
    let draw = |cache: &mut GuiBatchRenderCache<MockGuiDevice>, primitive| {
        let mut stats = RenderStats::default();
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

    // A colour transition reaches the style lane, which a gradient box never paints.
    let mut transitioning = panel.clone();
    if let SurfaceRenderPrimitive::Box {
        style,
        ..
    } = &mut transitioning
    {
        style.color = [1.0, 0.0, 0.0, 1.0];
    }
    let stats = draw(&mut cache, &transitioning);
    assert_eq!(stats.gui_rebuilds, 0);
    assert_eq!(stats.gui_allocations, 0);
    assert_eq!(stats.uploaded_bytes, 0);
    assert_eq!(device.borrow().writes.len(), 1);
}

#[test]
fn boxes_of_every_part_class_share_one_batch() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let part_box = |node: u32, part: GuiPrimitivePart, x: f32| {
        sample_box_primitive(node, 1, part, [x, 0.0], [0.4, 0.4], None)
    };

    // A row's background, slider track, fill, checkbox icon and focus ring paint in
    // this order under one clip; none of them changed recently.
    let row = [
        part_box(1, GuiPrimitivePart::Background, 0.0),
        part_box(2, GuiPrimitivePart::Background, 0.5),
        part_box(2, GuiPrimitivePart::Fill, 0.6),
        part_box(3, GuiPrimitivePart::Icon, 1.0),
        part_box(3, GuiPrimitivePart::FocusRing, 1.0),
    ];
    let boxes: Vec<_> = row.iter().collect();
    let mut stats = RenderStats::default();
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

    assert_eq!(stats.gui_batches, 1, "{stats:?}");
    assert_eq!(stats.draw_calls, 1);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().writes, [(1, 0, 5 * 6)]);
}

#[test]
fn unchanged_paint_revisions_skip_hashing_until_the_revision_changes() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let entity = ipp_core::EntityId::from_bits(1);
    let clip = [0.0, 0.0, 4.0, 2.0];
    let draw = |cache: &mut GuiBatchRenderCache<MockGuiDevice>,
                paint: SurfacePaint,
                boxes: &[&SurfaceRenderPrimitive]| {
        let mut stats = RenderStats::default();
        cache
            .draw_box_batch(&1, entity, clip, paint, boxes, &[0.0; 16], &mut stats)
            .unwrap();
        stats
    };
    let first = sample_box_primitive(1, 1, GuiPrimitivePart::Background, [0.0; 2], [1.0; 2], None);
    let second = sample_box_primitive(
        2,
        1,
        GuiPrimitivePart::Background,
        [1.5, 0.0],
        [1.0; 2],
        None,
    );
    let revision = |revision, reusable| SurfacePaint {
        revision,
        reusable,
    };

    assert_eq!(
        draw(&mut cache, revision(5, false), &[&first, &second]).gui_rebuilds,
        2
    );

    // A reusable revision promises unchanged inputs: the boxes are not hashed, so
    // even inputs that differ do not rebuild.
    let mut edited = second.clone();
    set_fill(&mut edited, 0.9);
    let reused = draw(&mut cache, revision(5, true), &[&first, &edited]);
    assert_eq!((reused.gui_rebuilds, reused.uploaded_bytes), (0, 0));

    // A new revision hashes every box and rebuilds only the edited one.
    let rebuilt = draw(&mut cache, revision(6, false), &[&first, &edited]);
    assert_eq!(rebuilt.gui_rebuilds, 1);
    assert!(rebuilt.uploaded_bytes > 0);

    // Hashes from another revision are never reused.
    let mut again = edited.clone();
    set_fill(&mut again, 0.1);
    assert_eq!(
        draw(&mut cache, revision(7, true), &[&first, &again]).gui_rebuilds,
        1
    );
}

#[test]
fn lifetime_fencing_prevents_reusing_recreated_node_batch() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity = ipp_core::EntityId::from_bits(1);
    let box_gen1 = sample_box_primitive(
        1,
        1, // lifetime 1
        GuiPrimitivePart::Background,
        [0.0, 0.0],
        [1.0, 1.0],
        None,
    );
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];

    let mut stats1 = RenderStats::default();
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
    assert_eq!(stats1.gui_allocations, 1);

    // Node is deleted and recreated: lifetime increments to 2
    let box_gen2 = sample_box_primitive(
        1,
        2, // lifetime 2
        GuiPrimitivePart::Background,
        [0.0, 0.0],
        [1.0, 1.0],
        None,
    );

    let mut stats2 = RenderStats::default();
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

    // Lifetime difference creates a new batch rather than reusing or overwriting stale handles
    assert_eq!(
        stats2.gui_allocations, 1,
        "new lifetime requires new batch allocation"
    );
}

#[test]
fn finish_frame_prunes_unreferenced_batches_and_tracks_resident_bytes() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity1 = ipp_core::EntityId::from_bits(1);
    let entity2 = ipp_core::EntityId::from_bits(2);
    let box1 = sample_box_primitive(
        1,
        1,
        GuiPrimitivePart::Background,
        [0.0, 0.0],
        [1.0, 1.0],
        None,
    );
    let box2 = sample_box_primitive(
        2,
        1,
        GuiPrimitivePart::Background,
        [0.0, 0.0],
        [1.0, 1.0],
        None,
    );
    let clip = [0.0, 0.0, 4.0, 2.0];
    let mvp = [0.0; 16];

    let mut stats = RenderStats::default();
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
    let storage_bytes = device.borrow().created_batches[0].1 * std::mem::size_of::<GuiVertex>();
    assert_eq!(
        storage_bytes,
        (6 + 24 + 12) * std::mem::size_of::<GuiVertex>()
    );
    assert_eq!(cache.resident_bytes(), storage_bytes);
}

#[test]
fn culled_surfaces_keep_retained_batches_and_incomplete_frames_prune_nothing() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let shown = ipp_core::EntityId::from_bits(1);
    let culled = ipp_core::EntityId::from_bits(2);
    let panel = sample_box_primitive(
        1,
        1,
        GuiPrimitivePart::Background,
        [0.0, 0.0],
        [1.0, 1.0],
        None,
    );
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

    let mut cold = RenderStats::default();
    draw(&mut cache, shown, &mut cold);
    draw(&mut cache, culled, &mut cold);
    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &live,
    }));
    assert_eq!(device.borrow().created_batches.len(), 2);

    // One frame outside the frustum: the culled Surface is live but never submitted.
    draw(&mut cache, shown, &mut RenderStats::default());
    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &BTreeSet::from([shown]),
    }));

    // A failed or cameraless frame cannot judge any key, even for destroyed Surfaces.
    cache.finish_frame(None);
    assert!(device.borrow().deleted_batches.is_empty());

    let mut visible_again = RenderStats::default();
    draw(&mut cache, culled, &mut visible_again);
    assert_eq!(visible_again.uploaded_bytes, 0);
    assert_eq!(visible_again.gui_rebuilds, 0);
    assert_eq!(visible_again.gui_allocations, 0);
    assert_eq!(visible_again.gui_batches, 1);
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
        (6 + 24 + 12) * std::mem::size_of::<GuiVertex>()
    );
}

#[test]
fn failed_batch_replacement_releases_storage_instead_of_drawing_stale_vertices() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());

    let entity = ipp_core::EntityId::from_bits(1);
    let panel = sample_box_primitive(
        1,
        1,
        GuiPrimitivePart::Background,
        [0.0, 0.0],
        [1.0, 1.0],
        None,
    );
    let moved = sample_box_primitive(
        1,
        1,
        GuiPrimitivePart::Background,
        [0.5, 0.0],
        [1.0, 1.0],
        None,
    );
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
            &mut RenderStats::default(),
        )
    };

    draw(&mut cache, &panel).unwrap();
    device.borrow_mut().fail_writes = true;
    assert!(draw(&mut cache, &moved).is_err());
    assert_eq!(device.borrow().deleted_batches, [1]);
    assert_eq!(cache.resident_bytes(), 0);
    assert_eq!(
        device.borrow().draws.len(),
        1,
        "stale storage is never drawn"
    );

    // The next frame allocates complete storage instead of reusing the failed batch.
    device.borrow_mut().fail_writes = false;
    draw(&mut cache, &moved).unwrap();
    assert_eq!(device.borrow().created_batches.len(), 2);
    assert_eq!(device.borrow().draws.last().unwrap().0, 2);
}

/// Bytes of one filled box quad.
const BOX_BYTES: u32 = 6 * std::mem::size_of::<GuiVertex>() as u32;

/// A long run of small filled boxes, one per GUI node.
fn box_run(nodes: std::ops::RangeInclusive<u32>) -> Vec<SurfaceRenderPrimitive> {
    nodes
        .map(|node| {
            let mut primitive = sample_box_primitive(
                node,
                1,
                GuiPrimitivePart::Background,
                [node as f32 * 0.01, 0.0],
                [0.005, 0.005],
                None,
            );
            if let SurfaceRenderPrimitive::Box {
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

fn set_fill(primitive: &mut SurfaceRenderPrimitive, red: f32) {
    if let SurfaceRenderPrimitive::Box {
        fill,
        ..
    } = primitive
    {
        *fill = GuiShapeFill::Solid([red, 0.4, 0.6, 1.0]);
    }
}

/// Submit one complete frame of a single visible Surface.
fn draw_run_frame(
    cache: &mut GuiBatchRenderCache<MockGuiDevice>,
    boxes: &[SurfaceRenderPrimitive],
    clip: SurfaceClipRect,
) -> RenderStats {
    let entity = ipp_core::EntityId::from_bits(1);
    let refs: Vec<_> = boxes.iter().collect();
    let mut stats = RenderStats::default();
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

const RUN_CLIP: SurfaceClipRect = [0.0, 0.0, 8.0, 2.0];

#[test]
fn large_runs_split_into_bounded_batches_at_identity_boundaries() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let boxes = box_run(1..=400);

    let cold = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert!(cold.gui_batches >= 4, "{cold:?}");
    assert_eq!(cold.draw_calls, 1, "one Surface storage draws as one range");
    assert_eq!(cold.uploaded_bytes, 400 * BOX_BYTES);
    assert_eq!(device.borrow().writes.len(), cold.gui_batches as usize);
    assert!(
        device
            .borrow()
            .writes
            .iter()
            .all(|&(_, _, vertices)| vertices <= MAX_BATCH_BOXES * 6)
    );

    let warm = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!((warm.uploaded_bytes, warm.gui_allocations), (0, 0));
    assert_eq!(warm.gui_batches, cold.gui_batches);
}

#[test]
fn early_box_edits_insertions_and_removals_rebuild_only_nearby_batches() {
    // Vertices of the first `count` batches, written one per batch by a cold frame.
    let first_batches = |device: &Rc<RefCell<MockGuiDevice>>, count: usize| {
        device.borrow().writes[..count]
            .iter()
            .map(|&(_, _, vertices)| vertices as u32 / 6)
            .sum::<u32>()
    };

    // Resizing the first box rewrites only its own batch.
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let mut boxes = box_run(1..=400);
    let cold = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    let written = device.borrow().writes.len();
    if let SurfaceRenderPrimitive::Box {
        size,
        ..
    } = &mut boxes[0]
    {
        *size = [0.008, 0.008];
    }
    let resized = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!(resized.gui_rebuilds, 1);
    assert!(
        resized.uploaded_bytes <= first_batches(&device, 1) * BOX_BYTES,
        "{resized:?}"
    );
    assert!(device.borrow().writes.len() - written <= 2);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert!(resized.gui_batches <= cold.gui_batches + 1);

    // Inserting a box before the first one rewrites at most the batches around it.
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let mut boxes = box_run(1..=400);
    draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    let written = device.borrow().writes.len();
    let bound = (first_batches(&device, 2) + 1) * BOX_BYTES;
    boxes.insert(0, box_run(1000..=1000).remove(0));
    let inserted = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!(inserted.gui_rebuilds, 1);
    assert!(inserted.uploaded_bytes <= bound, "{inserted:?}");
    assert!(device.borrow().writes.len() - written <= 2);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert!(device.borrow().deleted_batches.is_empty());

    // Removing an early box likewise leaves every later batch untouched.
    boxes.remove(3);
    let written = device.borrow().writes.len();
    let removed = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert!(removed.uploaded_bytes <= bound, "{removed:?}");
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
        split.uploaded_bytes <= (MAX_BATCH_BOXES as u32 + 1) * BOX_BYTES,
        "{split:?}"
    );
    assert!(split.gui_batches <= cold.gui_batches + 2);

    // Every later frame replaces only the animated box's own batch.
    for frame in 1..=30 {
        set_fill(&mut boxes[200], frame as f32 / 30.0);
        let animated = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
        assert_eq!(animated.uploaded_bytes, BOX_BYTES, "frame {frame}");
        assert_eq!((animated.gui_rebuilds, animated.gui_allocations), (1, 1));
        assert_eq!(animated.gui_batches, split.gui_batches);
        assert_eq!(animated.draw_calls, 1);
    }

    // At rest the box rejoins its neighbours once, after which frames upload nothing.
    let mut rest_bytes = 0;
    for _ in 0..VOLATILE_FRAMES + 2 {
        rest_bytes += draw_run_frame(&mut cache, &boxes, RUN_CLIP).uploaded_bytes;
    }
    assert!(rest_bytes <= (MAX_BATCH_BOXES as u32 + 1) * BOX_BYTES);
    let settled = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!(settled.uploaded_bytes, 0);
    assert_eq!(settled.gui_batches, cold.gui_batches);
    assert_eq!(device.borrow().created_batches.len(), 1);
}

#[test]
fn clip_changes_rewrite_only_the_boxes_they_clip() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let boxes = box_run(1..=40);
    draw_run_frame(&mut cache, &boxes, RUN_CLIP);

    // Scrolling a container changes the clip its boxes carry in every vertex.
    let scrolled = [0.5, 0.0, 3.0, 1.0];
    let stats = draw_run_frame(&mut cache, &boxes, scrolled);
    assert_eq!(stats.gui_rebuilds, 40);
    assert_eq!(stats.uploaded_bytes, 40 * BOX_BYTES);
    assert_eq!(device.borrow().created_batches.len(), 1);
    let drawn = device.borrow().draws.last().unwrap().1.clone();
    assert!(
        drawn
            .iter()
            .all(|vertex| *vertex == GuiVertex::EMPTY || vertex.clip == scrolled)
    );

    // Clipping one box of the run differently rewrites only that box's batch.
    let refs: Vec<_> = boxes.iter().collect();
    let mut clipped: Vec<_> = refs
        .iter()
        .map(|&primitive| (primitive, scrolled))
        .collect();
    clipped[20].1 = [1.0, 0.0, 2.0, 1.0];
    let mut stats = RenderStats::default();
    cache.begin_surface(ipp_core::EntityId::from_bits(1));
    cache.push_boxes(SurfacePaint::UNKNOWN, &clipped, &mut stats);
    cache.commit_surface(|_, _| &[], &mut stats).unwrap();
    assert_eq!(stats.gui_rebuilds, 1);
    assert!(
        stats.uploaded_bytes <= (MAX_BATCH_BOXES as u32 + 1) * BOX_BYTES,
        "{stats:?}"
    );
}

/// Glyph quad vertices of one test text batch, tinted and clipped.
fn glyph_quads(count: usize, x: f32, clip: SurfaceClipRect) -> Vec<GuiVertex> {
    (0..count * 6)
        .map(|index| GuiVertex {
            position: [x + (index / 6) as f32 * 0.01, 0.0],
            color0: [1.0, 1.0, 1.0, 1.0],
            gradient_coords: [0.5, 0.5, 0.0, 0.0],
            material_params: [GUI_FILL_GLYPH, 0.0, 0.0, 1.0],
            clip,
            ..GuiVertex::EMPTY
        })
        .collect()
}

/// One Surface's painter-order GUI work: box runs and text batches.
enum TestWork {
    Boxes(Vec<(SurfaceRenderPrimitive, SurfaceClipRect)>),
    Text {
        node: u32,
        page: usize,
        revision: u64,
        vertices: Vec<GuiVertex>,
    },
}

fn text_identity(node: u32) -> SurfacePrimitiveIdentity {
    SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
        root_incarnation: 1,
        node: GuiNodeId(node),
        lifetime: 1,
        part: GuiPrimitivePart::Label,
    })
}

/// One submitted Surface: its stats, its draws' vertices and whether each bound an
/// atlas, and the painter-order vertices its work should paint.
struct SubmittedWork {
    stats: RenderStats,
    draws: Vec<(Vec<GuiVertex>, bool)>,
    expected: Vec<GuiVertex>,
}

/// Submit `work` as one Surface and draw it.
fn submit_work(
    cache: &mut GuiBatchRenderCache<MockGuiDevice>,
    device: &Rc<RefCell<MockGuiDevice>>,
    work: &[TestWork],
) -> SubmittedWork {
    let mut stats = RenderStats::default();
    let entity = ipp_core::EntityId::from_bits(1);
    cache.begin_surface(entity);
    let mut expected = Vec::new();
    let mut text: BTreeMap<SurfacePrimitiveIdentity, &[GuiVertex]> = BTreeMap::new();
    for item in work {
        match item {
            TestWork::Boxes(boxes) => {
                let refs: Vec<_> = boxes
                    .iter()
                    .map(|(primitive, clip)| (primitive, *clip))
                    .collect();
                cache.push_boxes(SurfacePaint::UNKNOWN, &refs, &mut stats);
                for (primitive, clip) in boxes {
                    let SurfaceRenderPrimitive::Box {
                        style,
                        size,
                        corner_radius,
                        border_width,
                        border_color,
                        fill,
                        glow,
                    } = primitive
                    else {
                        unreachable!();
                    };
                    expected.extend(generate_box_vertices(
                        style,
                        size,
                        corner_radius,
                        *border_width,
                        border_color,
                        fill,
                        glow.as_ref(),
                        *clip,
                    ));
                }
            }
            TestWork::Text {
                node,
                page,
                revision,
                vertices,
            } => {
                let identity = text_identity(*node);
                cache.push_glyphs([GuiPiece {
                    key: GuiPieceKey::Glyphs(identity, 0),
                    hash: *revision,
                    len: vertices.len(),
                    page: Some(*page),
                    source: GuiPieceSource::Glyphs(identity, 0),
                }]);
                text.insert(identity, vertices);
                expected.extend_from_slice(vertices);
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
            &1,
            0..pieces,
            |page| textures.get(page),
            &[0.0; 16],
            &mut stats,
        )
        .unwrap();
    let draws = device.borrow().draws[before..]
        .iter()
        .map(|(_, vertices, atlas)| (vertices.clone(), *atlas))
        .collect();
    SubmittedWork {
        stats,
        draws,
        expected,
    }
}

#[test]
fn boxes_and_text_under_different_clips_draw_once_per_atlas_page() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let clips = [[0.0, 0.0, 1.0, 1.0], [0.2, 0.0, 0.8, 1.0]];
    let row = |node: u32, clip: SurfaceClipRect| {
        TestWork::Boxes(
            box_run(node..=node + 2)
                .into_iter()
                .map(|primitive| (primitive, clip))
                .collect(),
        )
    };
    let text = |node: u32, page: usize, clip| TestWork::Text {
        node,
        page,
        revision: u64::from(node),
        vertices: glyph_quads(3, 0.1 * node as f32, clip),
    };

    // Box, text, box and text runs of one page under alternating clips: one draw.
    let work = [
        row(1, clips[0]),
        text(100, 0, clips[1]),
        row(10, clips[1]),
        text(101, 0, clips[0]),
    ];
    let SubmittedWork {
        stats,
        draws,
        expected,
    } = submit_work(&mut cache, &device, &work);
    assert_eq!(stats.draw_calls, 1, "{stats:?}");
    assert_eq!(stats.gui_batches, 4);
    assert!(draws[0].1, "the range samples its atlas page");
    let painted: Vec<_> = draws[0]
        .0
        .iter()
        .copied()
        .filter(|vertex| *vertex != GuiVertex::EMPTY)
        .collect();
    assert_eq!(painted, expected, "painter order with per-vertex clips");

    // Text on a second page ends the range; later work continues from there.
    let work = [
        row(1, clips[0]),
        text(100, 0, clips[1]),
        text(102, 1, clips[1]),
        row(10, clips[1]),
    ];
    let SubmittedWork {
        stats,
        draws,
        ..
    } = submit_work(&mut cache, &device, &work);
    assert_eq!(stats.draw_calls, 2, "{stats:?}");
    assert!(draws.iter().all(|(_, atlas)| *atlas));
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
            vertices: glyph_quads(glyphs[run], run as f32, clip),
        };
        let work = [
            TestWork::Boxes(with_clip(0..thirds)),
            text(0),
            TestWork::Boxes(with_clip(thirds..2 * thirds)),
            text(1),
            text(2),
            TestWork::Boxes(with_clip(2 * thirds..boxes.len())),
        ];
        let SubmittedWork {
            stats,
            draws,
            expected,
        } = submit_work(&mut cache, &device, &work);
        assert_eq!(stats.draw_calls, 1, "frame {frame}");
        let painted: Vec<_> = draws[0]
            .0
            .iter()
            .copied()
            .filter(|vertex| *vertex != GuiVertex::EMPTY)
            .collect();
        assert!(painted == expected, "frame {frame}: drawn work differs");

        let live = BTreeSet::from([ipp_core::EntityId::from_bits(1)]);
        cache.finish_frame(Some(&RetainedSurfaceSubmission {
            live: &live,
            submitted: &live,
        }));
    }

    assert_eq!(
        device.borrow().contents.len(),
        1,
        "replaced storage is released"
    );
}
