//! Custom canvas paints through the RenderService with a recording device: painted
//! and ordinary boxes stay one draw, property writes upload parameters without
//! geometry, and every unusable paint draws its box's colour with a diagnostic.
//! GL compilation and pixels are the maintained default-skin scenario's evidence.

mod support;

use ipp_core::components::{CanvasBox, CanvasPaint, CanvasStyle, GuiBehavior, GuiButton};
use ipp_core::services::asset_management::shader::{
    ShaderBackendSource, ShaderDefinition, ShaderParameterKind,
};
use ipp_core::{Command, ComponentValue, DynamicValue, EntityId, EntityRef, HostRuntime};
use ipp_render_gl::{CANVAS_PAINT_SLOTS, CanvasPaintFallbackReason, RenderService};
use std::collections::BTreeMap;
use std::rc::Rc;
use support::canvas::{CanvasSurface, add_content, apply};
use support::{DeviceState, FrameStats, TestDevice, render_frame, setup};

/// Fill type of a custom paint in the retained shape record lanes.
const GUI_FILL_PAINT: f32 = 5.0;

fn definition(body: &str, parameters: &[(&str, ShaderParameterKind)]) -> Vec<u8> {
    ShaderDefinition {
        parameters: parameters
            .iter()
            .map(|(name, kind)| (name.to_string(), *kind))
            .collect(),
        backends: BTreeMap::from([(
            "glsl-es-300".into(),
            ShaderBackendSource {
                paint: body.into(),
                ..Default::default()
            },
        )]),
        ..Default::default()
    }
    .encode()
    .unwrap()
}

fn scanlines() -> Vec<u8> {
    definition(
        "return vec4(color.rgb * step(0.5, fract(position.y / p_spacing)), color.a);",
        &[("spacing", ShaderParameterKind::F32)],
    )
}

struct PaintScene {
    host: HostRuntime,
    renderer: RenderService<TestDevice>,
    state: Rc<DeviceState>,
    world: ipp_core::WorldId,
    surface: CanvasSurface,
    /// Shader definition bytes by source.
    sources: BTreeMap<String, Vec<u8>>,
}

impl PaintScene {
    /// A presented canvas holding one ordinary box and an ordinary button.
    fn new() -> Self {
        let mut host = HostRuntime::new();
        host.data_sources_mut()
            .register_stream("fixture://")
            .unwrap();
        let (world, renderer, state) = setup(&mut host);
        let world_id = world.id();
        drop(world);
        let world = world_id;
        let surface = CanvasSurface::new(
            &mut host,
            world,
            0.0,
            vec![
                ComponentValue::CanvasStyle(CanvasStyle::default()),
                ComponentValue::CanvasBox(CanvasBox {
                    width: 0.5,
                    height: 0.25,
                    ..Default::default()
                }),
            ],
        );
        add_content(
            &mut host,
            surface.output,
            vec![
                ComponentValue::CanvasStyle(CanvasStyle {
                    y: 0.3,
                    ..Default::default()
                }),
                ComponentValue::GuiButton(GuiButton::default()),
                ComponentValue::GuiBehavior(GuiBehavior::default()),
            ],
        );
        Self {
            host,
            renderer,
            state,
            world,
            surface,
            sources: BTreeMap::new(),
        }
    }

    fn source(&mut self, name: &str, bytes: Vec<u8>) -> String {
        let source = format!("fixture:///{name}.ipph");
        self.sources.insert(source.clone(), bytes);
        source
    }

    /// A painted box at `x` with numeric `properties`.
    fn painted(&mut self, x: f32, source: &str, properties: &[(&str, f32)]) -> EntityId {
        let mut paint = CanvasPaint {
            source: source.into(),
            ..Default::default()
        };
        for (name, value) in properties {
            paint
                .properties
                .set(name, DynamicValue::F32(*value))
                .unwrap();
        }
        add_content(
            &mut self.host,
            self.surface.output,
            vec![
                ComponentValue::CanvasStyle(CanvasStyle {
                    x,
                    ..Default::default()
                }),
                ComponentValue::CanvasBox(CanvasBox {
                    width: 0.1,
                    height: 0.1,
                    ..Default::default()
                }),
                ComponentValue::CanvasPaint(paint),
            ],
        )
    }

    fn set(&mut self, entity: EntityId, name: &str, value: f32) {
        apply(
            &mut self.host,
            self.surface.output.world().id(),
            vec![Command::SetDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CANVAS_PAINT,
                name: name.into(),
                value: DynamicValue::F32(value),
            }],
        );
    }

    /// Serve every requested shader definition, then render a frame.
    fn render(&mut self) -> FrameStats {
        for _ in 0..8 {
            self.host.progress_assets();
            let requests = self.host.take_resource_requests();
            if requests.is_empty() {
                break;
            }
            for request in requests {
                let bytes = self.sources.get(&*request.source).cloned();
                self.host
                    .complete_resource(request.id, bytes.ok_or_else(|| "missing".into()))
                    .unwrap();
            }
            self.host.frame(0.0).unwrap();
        }
        render_frame(&mut self.renderer, &mut self.host, self.world, 100, 100).unwrap()
    }

    fn reason(&self, entity: EntityId) -> Option<CanvasPaintFallbackReason> {
        self.renderer
            .canvas_paint_diagnostics()
            .get(&(self.surface.output, entity))
            .map(|fallback| fallback.reason.clone())
    }

    fn draws(&self) -> u32 {
        self.state.gui_batch_draws.get()
    }

    fn last_blocks(&self) -> Vec<[f32; 4]> {
        self.state
            .paint_block_uploads
            .borrow()
            .last()
            .cloned()
            .unwrap_or_default()
    }

    /// Painted shape records written since `from`, by their slot lane.
    fn painted_lanes(&self, from: usize) -> Vec<f32> {
        let mut lanes: Vec<f32> = self.state.gui_shapes_written.borrow()[from..]
            .iter()
            .filter(|record| record.material_params[0] % 8.0 == GUI_FILL_PAINT)
            .map(|record| record.color1[0])
            .collect();
        lanes.dedup();
        lanes
    }
}

#[test]
fn ordinary_controls_and_two_painted_boxes_draw_in_one_batch() {
    let mut scene = PaintScene::new();
    let source = scene.source("scanlines", scanlines());
    let first = scene.painted(0.2, &source, &[("spacing", 4.0)]);
    let second = scene.painted(0.4, &source, &[("spacing", 8.0)]);
    let before = scene.draws();
    scene.render();

    // The box, the button's parts and both painted boxes are one GUI draw.
    assert_eq!(scene.draws() - before, 1);
    assert!(scene.renderer.canvas_paint_diagnostics().is_empty());
    // The canvas program holds the paint's function once and dispatches to it.
    let fragments = scene.state.program_fragments.borrow();
    let canvas = fragments
        .iter()
        .rev()
        .find(|fragment| fragment.contains("#define IPP_CANVAS_PAINTS"))
        .expect("a canvas program with paints");
    assert_eq!(canvas.matches("vec4 ipp_paint_1(").count(), 1);
    assert!(!canvas.contains("ipp_paint_2("));
    drop(fragments);
    // Each instance has its own block of the one parameter array.
    assert_eq!(
        scene.last_blocks(),
        [[4.0, 0.0, 0.0, 0.0], [8.0, 0.0, 0.0, 0.0]]
    );
    assert_eq!(scene.painted_lanes(0), [1.0, 17.0]);
    let _ = (first, second);
}

#[test]
fn a_property_write_uploads_parameters_without_geometry() {
    let mut scene = PaintScene::new();
    let source = scene.source("scanlines", scanlines());
    let painted = scene.painted(0.2, &source, &[("spacing", 4.0)]);
    scene.render();
    let builds = scene.renderer.canvas_program_builds();
    let writes = scene.state.gui_batch_writes.get();

    scene.set(painted, "spacing", 6.0);
    let stats = scene.render();
    assert_eq!(scene.last_blocks(), [[6.0, 0.0, 0.0, 0.0]]);
    assert_eq!(scene.state.gui_batch_writes.get(), writes);
    assert_eq!(stats.gui_rebuilds, 0);
    assert_eq!(stats.uploaded_bytes, 0);
    assert_eq!(scene.renderer.canvas_program_builds(), builds);

    // An unchanged frame uploads nothing at all.
    let uploads = scene.state.paint_block_uploads.borrow().len();
    scene.render();
    assert_eq!(scene.state.paint_block_uploads.borrow().len(), uploads);
}

#[test]
fn unusable_paints_draw_their_colour_and_say_why() {
    let mut scene = PaintScene::new();
    *scene.state.fail_program_containing.borrow_mut() = Some("BROKEN".into());
    let scanlines = scene.source("scanlines", scanlines());
    let broken = scene.source("broken", definition("return BROKEN;", &[]));
    let material = scene.source(
        "material",
        ShaderDefinition {
            backends: BTreeMap::from([(
                "glsl-es-300".into(),
                ShaderBackendSource {
                    fragment: "vec4 materialFragment() { return vec4(1); }".into(),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        }
        .encode()
        .unwrap(),
    );
    let working = scene.painted(0.1, &scanlines, &[("spacing", 4.0)]);
    let failed = scene.painted(0.2, &broken, &[]);
    let missing = scene.painted(0.3, &scanlines, &[]);
    let not_paint = scene.painted(0.4, &material, &[]);
    let unset = scene.painted(0.5, "", &[]);
    let before = scene.draws();
    let written = scene.state.gui_shapes_written.borrow().len();
    scene.render();

    // A body that does not compile alone fails its asset and never reaches the
    // canvas program; the others are refused for their own reasons.
    assert_eq!(scene.reason(working), None);
    assert_eq!(
        scene.reason(failed),
        Some(CanvasPaintFallbackReason::Unavailable)
    );
    assert_eq!(
        scene.reason(missing),
        Some(CanvasPaintFallbackReason::Parameter("spacing".into()))
    );
    assert_eq!(
        scene.reason(not_paint),
        Some(CanvasPaintFallbackReason::NotAPaint)
    );
    assert_eq!(
        scene.reason(unset),
        Some(CanvasPaintFallbackReason::Unavailable)
    );
    assert!(
        scene
            .state
            .program_fragments
            .borrow()
            .iter()
            .filter(|fragment| fragment.contains("IPP_CANVAS_PAINTS"))
            .all(|fragment| !fragment.contains("BROKEN"))
    );
    // Only the working paint carries paint lanes; the rest draw solidly, and the
    // GUI keeps drawing in one batch.
    assert_eq!(scene.painted_lanes(written), [1.0]);
    assert_eq!(scene.draws() - before, 1);
}

#[test]
fn a_ninth_paint_in_one_frame_finds_no_slot() {
    let mut scene = PaintScene::new();
    let mut boxes = Vec::new();
    for index in 0..=CANVAS_PAINT_SLOTS {
        let source = scene.source(
            &format!("paint-{index}"),
            definition(&format!("return color * {}.0;", index + 1), &[]),
        );
        boxes.push(scene.painted(0.05 * index as f32, &source, &[]));
    }
    let before = scene.draws();
    scene.render();

    let reasons: Vec<_> = boxes.iter().map(|entity| scene.reason(*entity)).collect();
    assert_eq!(
        reasons
            .iter()
            .filter(|reason| **reason == Some(CanvasPaintFallbackReason::SlotLimit))
            .count(),
        1,
        "{reasons:?}"
    );
    assert_eq!(reasons.iter().filter(|reason| reason.is_none()).count(), 8);
    assert_eq!(scene.draws() - before, 1);
}

#[test]
fn an_instance_beyond_the_parameter_array_falls_back() {
    let mut scene = PaintScene::new();
    // Each wide instance takes most of the array; the second has no room.
    let names: Vec<String> = (0..100).map(|index| format!("p{index:03}")).collect();
    let parameters: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), ShaderParameterKind::F32))
        .collect();
    let values: Vec<_> = names.iter().map(|name| (name.as_str(), 1.0)).collect();
    let wide = scene.source("wide", definition("return color;", &parameters));
    let first = scene.painted(0.6, &wide, &values);
    let second = scene.painted(0.7, &wide, &values);
    scene.render();

    assert_eq!(scene.reason(first), None);
    assert_eq!(
        scene.reason(second),
        Some(CanvasPaintFallbackReason::UniformBudget)
    );
    assert_eq!(scene.last_blocks().len(), names.len());
}

#[test]
fn a_paint_the_canvas_program_rejects_falls_back_and_the_program_rebuilds_without_it() {
    let mut scene = PaintScene::new();
    let source = scene.source("scanlines", scanlines());
    let conflict = scene.source("conflict", definition("return color * 0.5;", &[]));
    let working = scene.painted(0.1, &source, &[("spacing", 4.0)]);
    scene.render();
    assert_eq!(scene.reason(working), None);

    // The second paint compiles alone, as its asset requires, but the canvas
    // program with it in the second slot fails to build.
    *scene.state.fail_program_containing.borrow_mut() = Some("vec4 ipp_paint_2(".into());
    let rejected = scene.painted(0.2, &conflict, &[]);
    let before = scene.draws();
    scene.render();
    scene.render();
    assert!(matches!(
        scene.reason(rejected),
        Some(CanvasPaintFallbackReason::Program(_))
    ));
    assert_eq!(scene.reason(working), None);
    assert_eq!(
        scene.draws() - before,
        2,
        "the GUI keeps drawing every frame"
    );
    let fragments = scene.state.program_fragments.borrow();
    assert!(fragments.last().unwrap().contains("vec4 ipp_paint_1("));
    assert!(!fragments.last().unwrap().contains("vec4 ipp_paint_2("));
}

#[test]
fn context_recovery_rebuilds_the_canvas_program_and_paints_again() {
    let mut scene = PaintScene::new();
    let source = scene.source("scanlines", scanlines());
    let painted = scene.painted(0.2, &source, &[("spacing", 4.0)]);
    scene.render();
    let builds = scene.renderer.canvas_program_builds();

    scene.renderer.set_asset_context_active(false);
    scene.renderer.unload_host(&mut scene.host).unwrap();
    scene.host.flush_resource_lifecycle();
    scene.renderer.set_asset_context_active(true);
    let written = scene.state.gui_shapes_written.borrow().len();
    for _ in 0..3 {
        scene.render();
    }
    assert_eq!(scene.reason(painted), None);
    assert!(scene.renderer.canvas_program_builds() > builds);
    assert_eq!(scene.painted_lanes(written), [1.0]);
    assert_eq!(scene.last_blocks(), [[4.0, 0.0, 0.0, 0.0]]);
}

#[test]
fn a_cached_canvas_repaints_when_a_paint_property_changes() {
    let mut scene = PaintScene::new();
    scene.state.cache_limit.set(4096);
    let source = scene.source("scanlines", scanlines());
    let painted = scene.painted(0.2, &source, &[("spacing", 4.0)]);
    apply(
        &mut scene.host,
        scene.surface.parent,
        vec![Command::insert_value(
            EntityRef::Handle(scene.surface.anchor),
            ComponentValue::SurfaceCache(ipp_core::SurfaceCache {
                direct_distance: 0.0,
                texels_per_metre: 64.0,
                max_refresh_hz: 10.0,
            }),
        )],
    );
    let mut repaints = 0;
    for _ in 0..4 {
        scene.host.frame(0.2).unwrap();
        repaints += scene.render().surface_cache_repaints;
    }
    assert!(repaints >= 1);
    scene.host.frame(0.2).unwrap();
    let warm = scene.render();
    assert_eq!(
        (warm.surface_cache_repaints, warm.surface_cache_reuses),
        (0, 1)
    );

    scene.set(painted, "spacing", 6.0);
    scene.host.frame(0.2).unwrap();
    let changed = scene.render();
    assert_eq!(changed.surface_cache_repaints, 1);
    assert_eq!(scene.last_blocks(), [[6.0, 0.0, 0.0, 0.0]]);
}

#[test]
fn rebuilt_canvas_batches_upload_their_parameters_again() {
    let mut scene = PaintScene::new();
    let source = scene.source("scanlines", scanlines());
    let painted = scene.painted(0.2, &source, &[("spacing", 4.0)]);
    scene.render();
    assert_eq!(scene.last_blocks(), [[4.0, 0.0, 0.0, 0.0]]);

    // Deselecting every output releases the canvas's retained batches and
    // blocks; the rebuilt blocks with a new value must reach the program.
    scene.renderer.prepare(&mut scene.host, None).unwrap();
    scene.set(painted, "spacing", 6.0);
    scene.render();
    assert_eq!(scene.last_blocks(), [[6.0, 0.0, 0.0, 0.0]]);
}
