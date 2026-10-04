//! Retained GUI draw order through real Worlds and the recording render device:
//! a run of canvas work draws its shapes, then its glyphs, and cuts where a later
//! shape covers earlier text; text edits write only glyph storage and shape
//! changes only shape storage; context recovery rebuilds both. The maintained
//! default-skin scenario owns the image evidence.

mod support;

use ipp_core::components::CanvasStyle;
use ipp_core::components::rows::Rows;
use ipp_core::systems::canvas::{CanvasBox, CanvasGlyphRow, CanvasGlyphRun};
use ipp_core::{Command, ComponentValue, EntityId, EntityRef, HostRuntime, WorldId};
use ipp_render_gl::RenderService;
use std::rc::Rc;
use support::canvas::{self, CanvasSurface};
use support::*;

/// Distance of the Surface in front of the camera: the one-metre canvas then
/// spans about 87 of the 100 pixels, so labels of 0.15 units project into the
/// glyph atlas's bands.
const SURFACE_Z: f32 = 4.0;

fn font() -> Vec<u8> {
    glyph_font(4, 1000, 600.0)
}

/// A label of `ids` at `position`, 0.15 units per em, its glyphs 0.09 wide.
fn label(ids: &[u32], position: [f32; 2]) -> Vec<ComponentValue> {
    let mut glyphs = Rows::new();
    for (index, &glyph_id) in ids.iter().enumerate() {
        glyphs
            .push(CanvasGlyphRow {
                glyph_id,
                position: [0.1 * index as f32, 0.0],
                color: None,
            })
            .unwrap();
    }
    vec![
        ComponentValue::CanvasGlyphRun(CanvasGlyphRun {
            source: "fixture:///font.ippf".into(),
            font_size: 0.15,
            glyphs,
            ..Default::default()
        }),
        ComponentValue::CanvasStyle(CanvasStyle {
            x: position[0],
            y: position[1],
            ..Default::default()
        }),
    ]
}

/// An opaque box at `rect` `[x, y, width, height]` tinted `red`.
fn filled(rect: [f32; 4], red: f32) -> Vec<ComponentValue> {
    vec![
        ComponentValue::CanvasStyle(CanvasStyle {
            x: rect[0],
            y: rect[1],
            red,
            ..Default::default()
        }),
        ComponentValue::CanvasBox(CanvasBox {
            width: rect[2],
            height: rect[3],
            ..Default::default()
        }),
    ]
}

struct Scene {
    host: HostRuntime,
    renderer: RenderService<TestDevice>,
    state: Rc<DeviceState>,
    world: WorldId,
    surface: CanvasSurface,
    /// Canvas content in painter order; the first is the Surface's own content.
    entities: Vec<EntityId>,
}

impl Scene {
    /// A Surface in front of the camera painting `content` in order.
    fn new(content: Vec<Vec<ComponentValue>>) -> Self {
        let mut host = support::task_scheduler::host();
        host.io_mut().register_stream("fixture://").unwrap();
        let (context, renderer, state) = setup(&mut host);
        let world = context.id();
        drop(context);
        let mut content = content.into_iter();
        let surface = CanvasSurface::new(&mut host, world, SURFACE_Z, content.next().unwrap());
        let mut entities = vec![surface.content];
        for values in content {
            entities.push(canvas::add_content(&mut host, surface.output, values));
        }
        resolve_text(&mut host, world, &font());
        Self {
            host,
            renderer,
            state,
            world,
            surface,
            entities,
        }
    }

    /// Render one frame: its retained GUI draws in order, `X` for shapes and `T`
    /// for glyphs, and its statistics.
    fn frame(&mut self) -> (String, FrameStats) {
        self.state.surface_events.borrow_mut().clear();
        let stats = render_frame(&mut self.renderer, &mut self.host, self.world, 100, 100).unwrap();
        let draws = self
            .state
            .surface_events
            .borrow()
            .chars()
            .filter(|event| matches!(event, 'X' | 'T'))
            .collect();
        (draws, stats)
    }

    /// Replace components of content entity `index`.
    fn edit(&mut self, index: usize, values: Vec<ComponentValue>) {
        let entity = self.entities[index];
        canvas::apply(
            &mut self.host,
            self.surface.output.world().id(),
            values
                .into_iter()
                .map(|value| Command::insert_value(EntityRef::Handle(entity), value))
                .collect(),
        );
    }

    /// Shape and glyph records written so far.
    fn written(&self) -> (usize, usize) {
        (
            self.state.gui_shapes_written.borrow().len(),
            self.state.gui_glyphs_written.borrow().len(),
        )
    }
}

/// A panel with a title, a button and its label, and a box beside the title.
fn panel() -> Vec<Vec<ComponentValue>> {
    vec![
        filled([0.0, 0.0, 1.0, 1.0], 0.1),
        label(&[0, 1, 2], [0.1, 0.1]),
        filled([0.1, 0.4, 0.5, 0.2], 0.3),
        label(&[3, 2], [0.15, 0.45]),
        filled([0.6, 0.1, 0.3, 0.1], 0.5),
    ]
}

#[test]
fn an_ordinary_panel_draws_its_shapes_then_its_glyphs() {
    let mut scene = Scene::new(panel());

    let (cold, stats) = scene.frame();
    assert_eq!(cold, "XT", "{stats:?}");
    assert_eq!(stats.gui_batches, 5);
    assert_eq!(stats.draw_calls, 2);

    let (warm, stats) = scene.frame();
    assert_eq!(warm, "XT");
    assert_eq!(stats.uploaded_bytes, 0);
}

#[test]
fn a_shape_painted_over_earlier_text_cuts_the_run() {
    let mut content = panel();
    // A dialog over the title, with its own label.
    content.push(filled([0.05, 0.05, 0.5, 0.3], 0.7));
    content.push(label(&[1, 1], [0.15, 0.2]));
    let mut scene = Scene::new(content);

    let (covered, stats) = scene.frame();
    assert_eq!(covered, "XTXT", "{stats:?}");
    assert_eq!(stats.draw_calls, 4);

    // Moved beside the panel's text the dialog joins the run again.
    scene.edit(5, filled([0.6, 0.65, 0.35, 0.3], 0.7));
    scene.edit(6, label(&[1, 1], [0.65, 0.7]));
    let (beside, _) = scene.frame();
    assert_eq!(beside, "XT");
}

#[test]
fn text_edits_write_glyph_storage_and_shape_changes_shape_storage() {
    let mut scene = Scene::new(panel());
    scene.frame();
    scene.frame();
    let warm = scene.written();

    // Retexting the title rewrites its glyph records and none of the shapes.
    scene.edit(1, label(&[2, 1, 0], [0.1, 0.1]));
    let (draws, stats) = scene.frame();
    assert_eq!(draws, "XT");
    let edited = scene.written();
    assert_eq!(edited.0, warm.0, "no shape record was written");
    assert_eq!(edited.1 - warm.1, 3, "the title's three glyphs");
    assert_eq!(
        stats.uploaded_bytes as usize,
        3 * std::mem::size_of::<ipp_render_gl::GuiGlyphRecord>()
    );

    // A button's new tint rewrites shape records and none of the glyphs.
    scene.edit(2, filled([0.1, 0.4, 0.5, 0.2], 0.9));
    let (draws, stats) = scene.frame();
    assert_eq!(draws, "XT");
    let hovered = scene.written();
    assert_eq!(hovered.1, edited.1, "no glyph record was written");
    assert!(hovered.0 > edited.0);
    assert_eq!(
        stats.uploaded_bytes as usize % std::mem::size_of::<ipp_render_gl::GuiShapeRecord>(),
        0
    );
}

#[test]
fn context_recovery_rebuilds_both_storages_in_the_same_order() {
    let mut content = panel();
    content.push(filled([0.05, 0.05, 0.5, 0.3], 0.7));
    content.push(label(&[1, 1], [0.15, 0.2]));
    let mut scene = Scene::new(content);
    let (before, _) = scene.frame();
    assert_eq!(before, "XTXT");
    let programs = scene.state.program_creates.get();
    let written = scene.written();

    recover_context(&mut scene.renderer, &mut scene.host, scene.world, &font());
    let (recovered, stats) = scene.frame();
    assert_eq!(recovered, before, "{stats:?}");
    let rewritten = scene.written();
    assert!(rewritten.0 > written.0 && rewritten.1 > written.1);
    assert!(
        scene.state.program_creates.get() > programs,
        "the canvas and glyph programs are rebuilt"
    );

    let (warm, stats) = scene.frame();
    assert_eq!(warm, before);
    assert_eq!(stats.uploaded_bytes, 0);
}
