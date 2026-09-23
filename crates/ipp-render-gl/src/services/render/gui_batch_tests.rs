//! Tests for retained GUI triangle batches and safe GPU storage replacement.

use std::cell::RefCell;
use std::rc::Rc;

use std::collections::BTreeSet;

use super::{
    GuiBatchRenderCache, GuiBoxVertex, MAX_BATCH_BOXES, RetainedSurfaceSubmission, VOLATILE_FRAMES,
    generate_box_vertices,
};
use crate::{GlyphVertex, RenderDevice, RenderError, RenderStats};
use ipp_core::systems::gui::GuiNodeId;
use ipp_core::systems::surface::{
    GuiPrimitiveId, GuiPrimitivePart, GuiShapeFill, GuiShapeGlow, SurfaceClipRect,
    SurfacePrimitiveIdentity, SurfacePrimitiveStyle, SurfaceRenderPrimitive,
};

#[derive(Default)]
struct MockGuiDevice {
    created_batches: Vec<(usize, usize)>, // (id, vertex_count)
    updated_batches: Vec<(usize, usize)>, // (id, vertex_count)
    deleted_batches: Vec<usize>,
    draws: Vec<(usize, [f32; 4])>,
    next_id: usize,
    fail_updates: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct MockGuiBatch {
    id: usize,
    vertex_count: usize,
}

impl RenderDevice for MockGuiDevice {
    type Program = u32;
    type Mesh = u32;
    type Texture = u32;
    type SurfacePath = u32;
    type SurfaceCacheTarget = ();
    #[cfg(feature = "shadows")]
    type ShadowMap = u32;
    type GuiBatch = MockGuiBatch;
    type GlyphBatch = MockGuiBatch;
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

    fn create_gui_batch(
        &mut self,
        vertices: &[GuiBoxVertex],
    ) -> Result<Self::GuiBatch, RenderError> {
        self.next_id += 1;
        let id = self.next_id;
        self.created_batches.push((id, vertices.len()));
        Ok(MockGuiBatch {
            id,
            vertex_count: vertices.len(),
        })
    }

    fn update_gui_batch(
        &mut self,
        batch: &mut Self::GuiBatch,
        vertices: &[GuiBoxVertex],
    ) -> Result<(), RenderError> {
        if self.fail_updates {
            return Err(RenderError::RenderDevice("injected update failure".into()));
        }

        batch.vertex_count = vertices.len();
        self.updated_batches.push((batch.id, vertices.len()));
        Ok(())
    }

    fn delete_gui_batch(&mut self, batch: Self::GuiBatch) {
        self.deleted_batches.push(batch.id);
    }

    fn draw_gui_batch(
        &mut self,
        _program: &Self::Program,
        batch: &Self::GuiBatch,
        _mvp: &[f32; 16],
        clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.draws.push((batch.id, *clip));
        Ok(())
    }

    fn create_glyph_batch(
        &mut self,
        vertices: &[GlyphVertex],
    ) -> Result<Self::GlyphBatch, RenderError> {
        self.next_id += 1;
        let id = self.next_id;
        self.created_batches.push((id, vertices.len()));
        Ok(MockGuiBatch {
            id,
            vertex_count: vertices.len(),
        })
    }

    fn update_glyph_batch(
        &mut self,
        batch: &mut Self::GlyphBatch,
        vertices: &[GlyphVertex],
    ) -> Result<(), RenderError> {
        batch.vertex_count = vertices.len();
        self.updated_batches.push((batch.id, vertices.len()));
        Ok(())
    }

    fn delete_glyph_batch(&mut self, batch: Self::GlyphBatch) {
        self.deleted_batches.push(batch.id);
    }

    fn draw_glyph_batch(
        &mut self,
        _program: &Self::Program,
        batch: &Self::GlyphBatch,
        _atlas: &Self::Texture,
        _mvp: &[f32; 16],
        clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.draws.push((batch.id, *clip));
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

    let vertices = generate_box_vertices(&style, &size, &corner, 0.05, &border_color, &fill, None);
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
        .draw_box_batch(&1, entity, clip, &boxes, &mvp, &mut stats1)
        .unwrap();

    assert_eq!(stats1.gui_allocations, 1);
    assert_eq!(stats1.gui_rebuilds, 2);
    assert_eq!(stats1.gui_batches, 1);
    assert_eq!(stats1.triangles, 4); // 2 boxes * 2 triangles
    let expected_bytes = 12 * std::mem::size_of::<GuiBoxVertex>() as u32;
    assert_eq!(stats1.uploaded_bytes, expected_bytes);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().created_batches[0].1, 12);

    // Frame 2: Warm unchanged frame
    let mut stats2 = RenderStats::default();
    cache
        .draw_box_batch(&1, entity, clip, &boxes, &mvp, &mut stats2)
        .unwrap();

    assert_eq!(stats2.gui_allocations, 0);
    assert_eq!(stats2.gui_rebuilds, 0);
    assert_eq!(stats2.gui_batches, 1);
    assert_eq!(stats2.triangles, 4);
    assert_eq!(stats2.uploaded_bytes, 0, "warm frame must upload 0 bytes");
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().updated_batches.len(), 0);
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
        .draw_box_batch(&1, entity, clip, &boxes_initial, &mvp, &mut stats1)
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
        .draw_box_batch(&1, entity, clip, &boxes_modified, &mvp, &mut stats2)
        .unwrap();

    assert_eq!(
        stats2.gui_rebuilds, 1,
        "only the modified primitive geometry rebuilds"
    );
    // The changed box turns volatile and leaves its stable neighbour's batch: that
    // batch replaces its storage without it, and the changed box gets its own batch.
    assert_eq!(stats2.gui_allocations, 2);
    assert_eq!(device.borrow().updated_batches, [(1, 6)]);
    assert_eq!(device.borrow().created_batches.len(), 2);
    let expected_bytes = 12 * std::mem::size_of::<GuiBoxVertex>() as u32;
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
    assert!(device.borrow().updated_batches.is_empty());
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
            &boxes,
            &[0.0; 16],
            &mut stats,
        )
        .unwrap();

    assert_eq!(stats.gui_batches, 1, "{stats:?}");
    assert_eq!(stats.draw_calls, 1);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().created_batches[0].1, 5 * 6);
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
        .draw_box_batch(&1, entity, clip, &[&box_gen1], &mvp, &mut stats1)
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
        .draw_box_batch(&1, entity, clip, &[&box_gen2], &mvp, &mut stats2)
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
        .draw_box_batch(&1, entity1, clip, &[&box1], &mvp, &mut stats)
        .unwrap();
    cache
        .draw_box_batch(&1, entity2, clip, &[&box2], &mvp, &mut stats)
        .unwrap();

    let mut live = std::collections::BTreeSet::new();
    live.insert(entity1);
    // entity2 is destroyed and removed from live surfaces

    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &live,
    }));

    assert_eq!(
        device.borrow().deleted_batches.len(),
        1,
        "destroyed entity batch must be freed on GPU"
    );
    let batch_bytes = 6 * std::mem::size_of::<GuiBoxVertex>();
    assert_eq!(cache.resident_bytes(), batch_bytes);
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
            .draw_box_batch(&1, entity, clip, &[&panel], &mvp, stats)
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
    assert!(device.borrow().updated_batches.is_empty());

    // Submitting a Surface without a batch it retained releases that batch.
    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &BTreeSet::from([shown, culled]),
    }));
    assert_eq!(device.borrow().deleted_batches, [1]);
    assert_eq!(
        cache.resident_bytes(),
        6 * std::mem::size_of::<GuiBoxVertex>()
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
            &[primitive],
            &mvp,
            &mut RenderStats::default(),
        )
    };

    draw(&mut cache, &panel).unwrap();
    device.borrow_mut().fail_updates = true;
    assert!(draw(&mut cache, &moved).is_err());
    assert_eq!(device.borrow().deleted_batches, [1]);
    assert_eq!(cache.resident_bytes(), 0);
    assert_eq!(
        device.borrow().draws.len(),
        1,
        "stale storage is never drawn"
    );

    // The next frame allocates complete storage instead of reusing the failed batch.
    device.borrow_mut().fail_updates = false;
    draw(&mut cache, &moved).unwrap();
    assert_eq!(device.borrow().created_batches.len(), 2);
    assert_eq!(device.borrow().draws.last().unwrap().0, 2);
}

/// Bytes of one filled box quad.
const BOX_BYTES: u32 = 6 * std::mem::size_of::<GuiBoxVertex>() as u32;

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
        .draw_box_batch(&1, entity, clip, &refs, &[0.0; 16], &mut stats)
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
    assert_eq!(cold.uploaded_bytes, 400 * BOX_BYTES);
    assert!(
        device
            .borrow()
            .created_batches
            .iter()
            .all(|(_, vertices)| *vertices <= MAX_BATCH_BOXES * 6)
    );

    let warm = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!((warm.uploaded_bytes, warm.gui_allocations), (0, 0));
    assert_eq!(warm.gui_batches, cold.gui_batches);
}

#[test]
fn early_box_edits_insertions_and_removals_rebuild_only_nearby_batches() {
    let first_batches = |device: &Rc<RefCell<MockGuiDevice>>, count: usize| {
        device.borrow().created_batches[..count]
            .iter()
            .map(|(_, vertices)| *vertices as u32 / 6)
            .sum::<u32>()
    };

    // Resizing the first box rebuilds only its own batch.
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let mut boxes = box_run(1..=400);
    let cold = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    let created = device.borrow().created_batches.len();
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
    assert!(
        device.borrow().updated_batches.len() + device.borrow().created_batches.len() - created
            <= 2
    );
    assert!(resized.gui_batches <= cold.gui_batches + 1);

    // Inserting a box before the first one creates at most the batches around it.
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let mut boxes = box_run(1..=400);
    draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    let created = device.borrow().created_batches.len();
    let bound = (first_batches(&device, 2) + 1) * BOX_BYTES;
    boxes.insert(0, box_run(1000..=1000).remove(0));
    let inserted = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert_eq!(inserted.gui_rebuilds, 1);
    assert!(inserted.uploaded_bytes <= bound, "{inserted:?}");
    assert!(device.borrow().updated_batches.is_empty());
    assert!(device.borrow().created_batches.len() - created <= 2);
    assert!(device.borrow().deleted_batches.len() <= 2);

    // Removing an early box likewise leaves every later batch untouched.
    boxes.remove(3);
    let created = device.borrow().created_batches.len();
    let removed = draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    assert!(removed.uploaded_bytes <= bound, "{removed:?}");
    assert!(
        device.borrow().updated_batches.len() + device.borrow().created_batches.len() - created
            <= 2
    );
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
}

#[test]
fn clip_only_changes_reuse_vertex_storage() {
    let device = Rc::new(RefCell::new(MockGuiDevice::default()));
    let mut cache = GuiBatchRenderCache::new(device.clone());
    let boxes = box_run(1..=40);
    draw_run_frame(&mut cache, &boxes, RUN_CLIP);
    let created = device.borrow().created_batches.len();

    let scrolled = [0.5, 0.0, 3.0, 1.0];
    let stats = draw_run_frame(&mut cache, &boxes, scrolled);
    assert_eq!(
        (
            stats.uploaded_bytes,
            stats.gui_rebuilds,
            stats.gui_allocations
        ),
        (0, 0, 0)
    );
    assert_eq!(device.borrow().created_batches.len(), created);
    assert!(device.borrow().updated_batches.is_empty());
    assert!(
        device.borrow().draws[created..]
            .iter()
            .all(|(_, clip)| *clip == scrolled)
    );
}
