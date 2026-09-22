//! Tests for retained GUI triangle batches and safe GPU storage replacement.

use std::cell::RefCell;
use std::rc::Rc;

use super::{GuiBatchRenderCache, GuiBoxVertex, GuiPartClass, generate_box_vertices};
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

    fn glyph_atlas_texture<'a>(&'a self, page: &'a Self::GlyphAtlasPage) -> &'a Self::Texture {
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
        24,
        "large border-only box splits into 4 edge quads (24 vertices)"
    );

    for quad_idx in 0..4 {
        let q = &vertices[quad_idx * 6..(quad_idx + 1) * 6];
        assert_eq!(q[0].position, q[3].position);
        assert_eq!(q[2].position, q[4].position);
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
        .draw_box_batch(
            &1,
            entity,
            clip,
            GuiPartClass::Background,
            &boxes,
            &mvp,
            &mut stats1,
        )
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
        .draw_box_batch(
            &1,
            entity,
            clip,
            GuiPartClass::Background,
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
        .draw_box_batch(
            &1,
            entity,
            clip,
            GuiPartClass::Background,
            &boxes_initial,
            &mvp,
            &mut stats1,
        )
        .unwrap();
    assert_eq!(stats1.gui_rebuilds, 2);

    // Modify only box2 (e.g. hovered color or slider thumb position)
    let mut box2_modified = box2.clone();
    if let SurfaceRenderPrimitive::Box {
        style,
        ..
    } = &mut box2_modified
    {
        style.color = [1.0, 1.0, 0.0, 1.0];
    }
    let boxes_modified = [&box1, &box2_modified];

    let mut stats2 = RenderStats::default();
    cache
        .draw_box_batch(
            &1,
            entity,
            clip,
            GuiPartClass::Background,
            &boxes_modified,
            &mvp,
            &mut stats2,
        )
        .unwrap();

    assert_eq!(
        stats2.gui_rebuilds, 1,
        "only the modified primitive geometry rebuilds"
    );
    assert_eq!(stats2.gui_allocations, 0);
    assert_eq!(device.borrow().updated_batches.len(), 1);
    let expected_bytes = 12 * std::mem::size_of::<GuiBoxVertex>() as u32;
    assert_eq!(stats2.uploaded_bytes, expected_bytes);
}

#[test]
fn cursor_work_is_separated_from_background_batches() {
    let bg_class = GuiPartClass::from_identity(SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
        root_incarnation: 1,
        node: GuiNodeId(1),
        lifetime: 1,
        part: GuiPrimitivePart::Background,
    }));
    let fill_class = GuiPartClass::from_identity(SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
        root_incarnation: 1,
        node: GuiNodeId(2),
        lifetime: 1,
        part: GuiPrimitivePart::Fill,
    }));
    let focus_class = GuiPartClass::from_identity(SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
        root_incarnation: 1,
        node: GuiNodeId(1),
        lifetime: 1,
        part: GuiPrimitivePart::FocusRing,
    }));

    assert_eq!(bg_class, GuiPartClass::Background);
    assert_eq!(fill_class, GuiPartClass::Fill);
    assert_eq!(focus_class, GuiPartClass::FocusRing);
    assert_ne!(bg_class, fill_class);
    assert_ne!(bg_class, focus_class);
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
            GuiPartClass::Background,
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
            GuiPartClass::Background,
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
            GuiPartClass::Background,
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
            GuiPartClass::Background,
            &[&box2],
            &mvp,
            &mut stats,
        )
        .unwrap();

    let mut live = std::collections::BTreeSet::new();
    live.insert(entity1);
    // entity2 is destroyed and removed from live surfaces

    cache.finish_frame(&mut *device.borrow_mut(), &live, &mut stats);

    assert_eq!(
        device.borrow().deleted_batches.len(),
        1,
        "destroyed entity batch must be freed on GPU"
    );
    let batch_bytes = 6 * std::mem::size_of::<GuiBoxVertex>() as u32;
    assert_eq!(stats.gui_resident_bytes, batch_bytes);
}
