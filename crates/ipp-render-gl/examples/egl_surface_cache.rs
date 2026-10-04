//! Native GLES whole-Surface cache scenario.
//!
//! The device section runs the shared cache-target oracle: creation bounds,
//! resizing, nesting rules, premultiplied composition front and mirrored, and
//! atlas population nested inside a repaint. The service section drives a real
//! Host and `RenderService` with owned font, drawing and bitmap assets. A camera
//! World places an opted-in panel Surface over an opaque backdrop Surface; each
//! presents the Canvas output of its own attached child World, authored as
//! ordinary layout, Canvas leaf and control entities. An orthographic camera
//! keeps the projected panel size fixed while its distance selects the cache
//! band, so cached and direct captures compare pixel for pixel: 80 screen pixels
//! per metre, a 4 x 2 metre panel over rows 40..200. The panel anchor opts in
//! through its `SurfaceCache` component; the Worlds' own prepared policy,
//! revisions and interaction priority drive every decision.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

#[cfg(target_os = "linux")]
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(target_os = "linux")]
mod scenario {
    use super::{Result, smoke};
    use ipp_core::components::rows::Rows;
    use ipp_core::components::{
        Camera, CanvasBitmap, CanvasBox, CanvasDrawing, CanvasStyle, CanvasText, GuiButton,
        GuiFont, GuiLayout, GuiSkin, Transform,
    };
    use ipp_core::services::asset_management::{
        AssetSource, drawing::DRAWING_TYPE, font::FONT_TYPE,
    };
    use ipp_core::services::gui_input::router::{
        GuiInputRouter, GuiPhysicalInput, GuiRoutingContext, GuiRoutingDelivery,
    };
    use ipp_core::services::gui_input::{
        GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputError,
    };
    use ipp_core::systems::canvas::CanvasPublication;
    use ipp_core::systems::gui::GuiPartId;
    use ipp_core::systems::gui::GuiPrimitivePart;
    use ipp_core::systems::gui::local::{GuiEntityTarget, GuiLocalAction, GuiLocalEffect};
    use ipp_core::systems::gui::presentation::GuiPaintPart;
    use ipp_core::{
        Batch, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, FieldValue,
        FieldWrite, FlatSurface, HostRuntime, OutputKind, OutputRef, SurfaceCache, TEXTURE_TYPE,
        ViewQueryTarget, WorldAttachment, WorldId, WorldViewport,
    };
    use ipp_render_gl::{
        GlesRenderDevice, RenderService, SurfaceCacheDiagnostic, SurfaceCachePresentation,
    };
    use smoke::frame_stats::{FrameStats, RenderFrameStats};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::mem::offset_of;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::Arc;

    const WIDTH: u32 = smoke::world::WIDTH;
    const HEIGHT: u32 = smoke::world::HEIGHT;

    /// Queue owners of the physical router and of programmatic focus requests.
    const ROUTED_OWNER: u64 = 77;

    /// Host frame interval; the cache refresh clock is accumulated World time.
    const DT: f64 = 1.0 / 60.0;

    /// Authored policy: direct inside 4 m, band 1 in [4, 8), band 2 in [8, 16).
    /// Band 1 matches the 80 px/m screen density, so its texels align with pixels.
    const POLICY: SurfaceCache = SurfaceCache {
        direct_distance: 4.0,
        texels_per_metre: 80.0,
        max_refresh_hz: 10.0,
    };

    /// Camera distances selecting each presentation.
    const NEAR: f32 = 2.0;
    const BAND1: f32 = 6.0;
    const BAND1_MOVED: f32 = 7.0;
    const BAND2: f32 = 12.0;

    /// Panel content size in metres and its expected band-1 and band-2 images.
    const PANEL: [f32; 2] = [4.0, 2.0];
    const BAND1_SIZE: [u32; 2] = [320, 160];
    const BAND2_SIZE: [u32; 2] = [160, 80];

    /// Direct-versus-cached tolerance per region at matched density (plan §5):
    /// bilinear sampling at texel centres reproduces direct coverage except for
    /// SRGB8 rounding of premultiplied low-alpha edges.
    const MATCHED_MEAN: f64 = 2.0;
    const MATCHED_MAX: u8 = 24;

    /// Pixels per region allowed beyond `MATCHED_MAX`: clip corners in the cache
    /// image round coverage differently from the screen by up to 36 levels on a
    /// few pixels (observed three), which bilinear sampling keeps.
    const MATCHED_OUTLIERS: u32 = 8;

    /// Band 2 halves the density: a loose bound on resampled edges only.
    const REDUCED_MEAN: f64 = 16.0;

    /// Recovered glyph atlas slots may round coverage by one level at glyph edges.
    const RECOVERY_MAX: u8 = 2;

    /// Adapter delivery for routed and programmatic GUI input: every reserved
    /// request records its single terminal.
    struct Permit(Rc<RefCell<Vec<GuiDeliveryTerminal>>>);

    impl GuiDeliveryPermit for Permit {
        fn prepare(
            &mut self,
            _: Option<&GuiLocalEffect>,
        ) -> std::result::Result<(), GuiDeliveryError> {
            Ok(())
        }

        fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
            self.0.borrow_mut().push(terminal);
        }
    }

    struct Delivery(Rc<RefCell<Vec<GuiDeliveryTerminal>>>);

    impl GuiRoutingDelivery for Delivery {
        fn command(&mut self) -> std::result::Result<Box<dyn GuiDeliveryPermit>, GuiInputError> {
            Ok(Box::new(Permit(self.0.clone())))
        }
    }

    pub(super) struct Scene<'a> {
        renderer: &'a mut RenderService<GlesRenderDevice>,
        context: &'a smoke::egl::Context,
        host: HostRuntime,
        tasks: Rc<smoke::task_scheduler::SmokeAssetTasks>,
        world: WorldId,
        root: OutputRef,
        camera: EntityId,
        panel: EntityId,
        canvas: OutputRef,
        label: EntityId,
        drawing: EntityId,
        button: EntityId,
        router: GuiInputRouter,
        routing: Option<GuiRoutingContext>,
        /// Batch identity of the latest programmatic `GuiAction` command.
        actions: u64,
        terminals: Rc<RefCell<Vec<GuiDeliveryTerminal>>>,
        evidence: &'a Path,
        payloads: BTreeMap<Arc<str>, Vec<u8>>,
        report: String,
    }

    fn source(kind: ipp_core::services::asset_management::AssetTypeId, name: &str) -> AssetSource {
        AssetSource {
            kind,
            uri: format!("fixture:///{name}").into(),
            variant: 0,
        }
    }

    fn field(entity: EntityId, component: u16, offset: usize, value: f32) -> Command {
        Command::SetField {
            entity: EntityRef::Handle(entity),
            component,
            field: FieldWrite {
                offset: offset as u32,
                value: FieldValue::F32(value),
            },
        }
    }

    /// Queue one ordered authoring batch and apply it in a Host frame.
    fn apply(
        host: &mut HostRuntime,
        world: WorldId,
        operations: Vec<Command>,
    ) -> Result<Vec<(u32, EntityId)>> {
        host.world_mut(world)
            .ok_or_else(|| format!("unknown World {world:?}"))?
            .enqueue(Batch {
                id: 1,
                operations,
            })?;

        let update = host
            .frame(0.0)?
            .worlds
            .remove(&world)
            .ok_or_else(|| format!("World {world:?} was not updated"))?
            .map_err(|reason| format!("World {world:?} update: {reason:?}"))?;
        let outcome = update
            .outcomes
            .into_iter()
            .next()
            .ok_or_else(|| format!("World {world:?} reported no batch outcome"))?;

        Ok(outcome
            .result
            .map_err(|error| format!("cache scene batch failed: {error:?}"))?)
    }

    /// Create one entity with `values`, optionally as the last child of `parent`.
    fn create(
        host: &mut HostRuntime,
        world: WorldId,
        parent: Option<EntityId>,
        values: Vec<ComponentValue>,
    ) -> Result<EntityId> {
        let mut operations = vec![Command::Create {
            alias: 0,
            metadata: Default::default(),
            adopt: false,
        }];
        operations.extend(
            values
                .into_iter()
                .map(|value| Command::insert_value(EntityRef::Alias(0), value)),
        );
        if let Some(parent) = parent {
            operations.push(Command::PlaceEntity {
                entity: EntityRef::Alias(0),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(parent)),
                    before: None,
                },
            });
        }

        Ok(apply(host, world, operations)?[0].1)
    }

    /// An attached child World's canvas with one logical unit per metre.
    fn canvas_output(host: &mut HostRuntime, extent: [f32; 2]) -> Result<OutputRef> {
        let mut options = ipp_core::WorldCreateOptions::new(smoke::selection::panel());
        options.canvas = Some(ipp_core::CanvasState {
            extent,
            units_per_metre: 1.0,
        });
        let world = host.create_world_with_options(Default::default(), options)?;
        Ok(OutputRef::canvas(host.world_ref(world).unwrap()))
    }

    /// A Surface anchor at depth `z` presenting `canvas` at its extent.
    fn anchor(
        host: &mut HostRuntime,
        world: WorldId,
        z: f32,
        canvas: OutputRef,
        extent: [f32; 2],
    ) -> Result<EntityId> {
        let surface = FlatSurface {
            width: extent[0],
            height: extent[1],
            ..Default::default()
        };
        create(
            host,
            world,
            None,
            vec![
                ComponentValue::Transform(Transform {
                    z,
                    ..Default::default()
                }),
                ComponentValue::FlatSurface(surface),
                ComponentValue::WorldAttachment(WorldAttachment::surface(canvas)),
            ],
        )
    }

    /// Layout placement inside the panel stack: `[top, left]` margins and an
    /// explicit size, or -1 for intrinsic sizing.
    fn placed(kind: u32, size: [f32; 2], margin: [f32; 2]) -> ComponentValue {
        ComponentValue::GuiLayout(GuiLayout {
            kind,
            width: size[0],
            height: size[1],
            margin_top: margin[0],
            margin_left: margin[1],
            ..Default::default()
        })
    }

    fn tint(color: [f32; 4]) -> CanvasStyle {
        CanvasStyle {
            red: color[0],
            green: color[1],
            blue: color[2],
            alpha: color[3],
            ..Default::default()
        }
    }

    /// Per-region mean and maximum channel difference over the panel rows in a
    /// 4 x 2 grid, with the count of pixels differing by more than `MATCHED_MAX`.
    fn regions(expected: &[u8], actual: &[u8]) -> Vec<(f64, u8, u32)> {
        let mut out = Vec::new();
        for row in 0..2 {
            for column in 0..4 {
                let (mut total, mut max, mut count, mut outliers) = (0u64, 0u8, 0u64, 0u32);
                for y in 40 + row * 80..40 + (row + 1) * 80 {
                    for x in column * 80..(column + 1) * 80 {
                        let offset = ((y * WIDTH + x) * 4) as usize;
                        let mut pixel = 0;
                        for channel in 0..3 {
                            let difference =
                                expected[offset + channel].abs_diff(actual[offset + channel]);
                            total += u64::from(difference);
                            pixel = pixel.max(difference);
                            count += 1;
                        }
                        max = max.max(pixel);
                        outliers += u32::from(pixel > MATCHED_MAX);
                    }
                }
                out.push((total as f64 / count as f64, max, outliers));
            }
        }
        out
    }

    fn max_difference(expected: &[u8], actual: &[u8]) -> u8 {
        expected
            .iter()
            .zip(actual)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0)
    }

    impl<'a> Scene<'a> {
        pub(super) fn new(
            renderer: &'a mut RenderService<GlesRenderDevice>,
            context: &'a smoke::egl::Context,
            assets: &Path,
            fonts: &Path,
            evidence: &'a Path,
        ) -> Result<Self> {
            let mut host = HostRuntime::new();
            let tasks = smoke::task_scheduler::SmokeAssetTasks::install(&mut host);
            renderer.install(&mut host)?;
            host.io_mut().register_stream("fixture://")?;
            let world = host.create_world(Default::default(), &smoke::selection::scene())?;
            let canvas = canvas_output(&mut host, PANEL)?;
            let backdrop = canvas_output(&mut host, [4.0, 3.0])?;
            let camera = create(
                &mut host,
                world,
                None,
                vec![
                    ComponentValue::Transform(Transform {
                        z: NEAR,
                        ..Default::default()
                    }),
                    ComponentValue::Camera(Camera {
                        projection: 1,
                        ortho_height: 3.0,
                        ..Default::default()
                    }),
                ],
            )?;
            let panel = anchor(&mut host, world, 0.0, canvas, PANEL)?;
            anchor(&mut host, world, -1.0, backdrop, [4.0, 3.0])?;
            host.world_mut(world)
                .unwrap()
                .enqueue_camera_activate(camera)?;
            host.frame(0.0)?;
            let root =
                host.bind_output(host.world_ref(world).unwrap(), camera, OutputKind::Camera)?;
            host.set_root_output(
                root,
                WorldViewport {
                    width: WIDTH,
                    height: HEIGHT,
                    device_pixel_ratio: 1.0,
                },
            )?;

            let font = source(FONT_TYPE, "shure-tech-mono.ippf");
            let drawing_source = source(DRAWING_TYPE, "panel.ippd");
            let bitmap = source(TEXTURE_TYPE, "badge.ippt");
            let payloads = BTreeMap::from([
                (
                    font.uri.clone(),
                    std::fs::read(fonts.join("shure-tech-mono.ippf"))?,
                ),
                (
                    drawing_source.uri.clone(),
                    std::fs::read(assets.join("panel.ippd"))?,
                ),
                // The same drawing under a second identity for resource replacement.
                (
                    source(DRAWING_TYPE, "panel-replacement.ippd").uri,
                    std::fs::read(assets.join("panel.ippd"))?,
                ),
                (
                    bitmap.uri.clone(),
                    std::fs::read(assets.join("badge.ippt"))?,
                ),
            ]);

            // Opaque backdrop behind the panel, never opted in.
            let box_leaf = |host: &mut HostRuntime,
                            output: OutputRef,
                            position: [f32; 2],
                            size: [f32; 2],
                            color: [f32; 4]|
             -> Result<EntityId> {
                create(
                    host,
                    output.world().id(),
                    None,
                    vec![
                        ComponentValue::CanvasBox(CanvasBox {
                            width: size[0],
                            height: size[1],
                            ..Default::default()
                        }),
                        ComponentValue::CanvasStyle(CanvasStyle {
                            x: position[0],
                            y: position[1],
                            ..tint(color)
                        }),
                    ],
                )
            };
            box_leaf(
                &mut host,
                backdrop,
                [0.0, 0.0],
                [4.0, 3.0],
                [0.05, 0.12, 0.3, 1.0],
            )?;
            box_leaf(
                &mut host,
                backdrop,
                [1.5, 0.0],
                [1.0, 3.0],
                [0.9, 0.9, 0.9, 1.0],
            )?;

            // The panel stack is transparent: the backdrop shows through its gaps.
            let gui = canvas.world().id();
            let stack = create(&mut host, gui, None, vec![placed(3, PANEL, [0.0, 0.0])])?;
            let append = |host: &mut HostRuntime, values| create(host, gui, Some(stack), values);

            // Gradient, border and glow: the control's Background part override,
            // a rounded box instead of the default button look's cut corners.
            let mut parts = Rows::new();
            parts
                .push(GuiPaintPart {
                    color: Some([1.0, 1.0, 1.0, 1.0]),
                    fill_mode: Some(1.0),
                    gradient_start: Some([0.0, 0.0]),
                    gradient_end: Some([0.0, 1.0]),
                    gradient_color0: Some([1.0, 0.08, 0.04, 1.0]),
                    gradient_color1: Some([1.0, 0.8, 0.08, 1.0]),
                    corner_radius: Some([0.12, 0.12]),
                    corner_cut: Some([0.0; 4]),
                    border_width: Some(0.05),
                    border_color: Some([1.0, 1.0, 1.0, 1.0]),
                    glow_color: Some([1.0, 0.35, 0.05, 1.0]),
                    glow_intensity: Some(0.8),
                    glow_radius: Some(0.15),
                    glow_falloff: Some(2.0),
                    ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background))?
                })
                .unwrap();
            append(
                &mut host,
                vec![
                    ComponentValue::GuiButton(GuiButton::default()),
                    placed(0, [1.4, 1.0], [0.3, 0.25]),
                    ComponentValue::GuiSkin(GuiSkin {
                        parts,
                        ..Default::default()
                    }),
                ],
            )?;

            // Overlapping translucent boxes over the backdrop.
            for (size, margin, color) in [
                ([0.8, 0.8], [0.2, 1.9], [1.0, 0.1, 0.1, 0.5]),
                ([0.8, 0.8], [0.6, 2.3], [0.1, 1.0, 0.2, 0.5]),
            ] {
                append(
                    &mut host,
                    vec![
                        ComponentValue::CanvasBox(CanvasBox::default()),
                        placed(6, size, margin),
                        ComponentValue::CanvasStyle(tint(color)),
                    ],
                )?;
            }

            // Clipped content: a viewport clip smaller than its laid-out child.
            let viewport = append(
                &mut host,
                vec![
                    placed(3, [-1.0, -1.0], [1.45, 3.2]),
                    ComponentValue::CanvasStyle(CanvasStyle {
                        clipped: true,
                        clip_max_x: 0.6,
                        clip_max_y: 0.4,
                        ..Default::default()
                    }),
                ],
            )?;
            create(
                &mut host,
                gui,
                Some(viewport),
                vec![
                    ComponentValue::CanvasBox(CanvasBox::default()),
                    placed(6, [1.5, 1.5], [0.0, 0.0]),
                    ComponentValue::CanvasStyle(tint([1.0, 0.0, 1.0, 1.0])),
                ],
            )?;

            let label = append(
                &mut host,
                vec![
                    ComponentValue::CanvasText(CanvasText {
                        text: "Cache".into(),
                        source: font.uri.clone(),
                        variant: font.variant,
                        font_size: 0.3,
                    }),
                    placed(0, [-1.0, -1.0], [1.45, 0.3]),
                    ComponentValue::CanvasStyle(tint([1.0, 0.85, 0.2, 1.0])),
                ],
            )?;

            // The unit-square drawing is centred in and scaled to its layout box.
            let drawing = append(
                &mut host,
                vec![
                    ComponentValue::CanvasDrawing(CanvasDrawing {
                        source: drawing_source.uri.clone(),
                        variant: drawing_source.variant,
                    }),
                    placed(0, [0.6, 0.45], [0.3, 3.2]),
                    ComponentValue::CanvasStyle(CanvasStyle {
                        x: 0.3,
                        y: 0.225,
                        scale_x: 0.6,
                        scale_y: 0.45,
                        ..tint([0.2, 0.6, 1.0, 1.0])
                    }),
                ],
            )?;
            append(
                &mut host,
                vec![
                    ComponentValue::CanvasBitmap(CanvasBitmap {
                        source: bitmap.uri.clone(),
                        variant: bitmap.variant,
                        width: 0.35,
                        height: 0.35,
                    }),
                    placed(0, [-1.0, -1.0], [0.95, 3.3]),
                    ComponentValue::CanvasStyle(CanvasStyle::default()),
                ],
            )?;
            let button = append(
                &mut host,
                vec![
                    ComponentValue::GuiButton(GuiButton {
                        label: "Go".into(),
                        ..Default::default()
                    }),
                    ComponentValue::GuiFont(GuiFont {
                        source: font.uri.clone(),
                        variant: font.variant,
                        font_size: 0.2,
                    }),
                    placed(0, [0.7, 0.35], [1.5, 2.1]),
                ],
            )?;

            let mut scene = Self {
                renderer,
                context,
                host,
                tasks,
                world,
                root,
                camera,
                panel,
                canvas,
                label,
                drawing,
                button,
                router: GuiInputRouter::default(),
                routing: None,
                actions: 0,
                terminals: Rc::default(),
                evidence,
                payloads,
                report: String::new(),
            };
            scene.settle(&[font, drawing_source, bitmap])?;
            Ok(scene)
        }

        fn batch(&mut self, operations: Vec<Command>) -> Result<()> {
            self.enqueue(self.world, operations)
        }

        /// Queue ordered writes for `world`'s next Host frame boundary.
        fn enqueue(&mut self, world: WorldId, operations: Vec<Command>) -> Result<()> {
            let mut world = self.host.world_mut(world).unwrap();
            world.enqueue(Batch {
                id: world.tick() + 1,
                operations,
            })?;
            Ok(())
        }

        /// Recolour the text leaf through its ordinary CanvasStyle tint fields.
        fn text_color(&mut self, color: [f32; 4]) -> Result<()> {
            let label = self.label;
            let operations = [
                (offset_of!(CanvasStyle, red), color[0]),
                (offset_of!(CanvasStyle, green), color[1]),
                (offset_of!(CanvasStyle, blue), color[2]),
                (offset_of!(CanvasStyle, alpha), color[3]),
            ]
            .map(|(offset, value)| field(label, ComponentValue::CANVAS_STYLE, offset, value))
            .to_vec();
            self.enqueue(self.canvas.world().id(), operations)
        }

        fn camera_distance(&mut self, distance: f32) -> Result<()> {
            let camera = self.camera;
            self.batch(vec![field(
                camera,
                ComponentValue::TRANSFORM,
                offset_of!(Transform, z),
                distance,
            )])
        }

        fn turn_panel(&mut self, rear: bool) -> Result<()> {
            let panel = self.panel;
            let (qy, qw) = if rear {
                (1.0, 0.0)
            } else {
                (0.0, 1.0)
            };
            // Write the growing component first so no intermediate quaternion is zero.
            let mut writes = vec![
                field(
                    panel,
                    ComponentValue::TRANSFORM,
                    offset_of!(Transform, qy),
                    qy,
                ),
                field(
                    panel,
                    ComponentValue::TRANSFORM,
                    offset_of!(Transform, qw),
                    qw,
                ),
            ];
            if !rear {
                writes.reverse();
            }

            self.batch(writes)
        }

        fn opt_in(&mut self, enabled: bool) -> Result<()> {
            let panel = self.panel;
            self.batch(vec![if enabled {
                Command::insert_value(
                    EntityRef::Handle(panel),
                    ComponentValue::SurfaceCache(POLICY),
                )
            } else {
                Command::RemoveComponent {
                    entity: EntityRef::Handle(panel),
                    component: ComponentValue::SURFACE_CACHE,
                }
            }])
        }

        /// Route one physical event against the current completed root view.
        fn route(&mut self, input: GuiPhysicalInput) -> Result<()> {
            if self.routing.is_none() {
                let (context, _) =
                    self.router
                        .bind(&self.host, self.root.world(), ROUTED_OWNER, Vec::new())?;
                self.routing = Some(context);
            }
            let view = self.host.resolve_view(ViewQueryTarget::RootView {
                output: self.root,
                expected_viewport: WorldViewport {
                    width: WIDTH,
                    height: HEIGHT,
                    device_pixel_ratio: 1.0,
                },
            })?;
            self.router.route(
                &mut self.host,
                self.routing.as_mut().unwrap(),
                view,
                input,
                &mut Delivery(self.terminals.clone()),
            )?;
            Ok(())
        }

        /// Programmatic focus or blur of the "Go" button.
        fn focus(&mut self, action: GuiLocalAction) -> Result<()> {
            let gui = self.canvas.world().id();
            let hit = self
                .host
                .output(
                    self.host.latest_publication(gui).ok_or("no publication")?,
                    self.canvas,
                )
                .and_then(|output| output.data::<CanvasPublication>())
                .and_then(|publication| {
                    publication
                        .hits
                        .iter()
                        .find(|hit| hit.target.entity == self.button)
                        .cloned()
                })
                .ok_or("the button has no Canvas hit")?;
            let target = GuiEntityTarget::from_canvas(self.canvas.world(), hit.target);
            self.actions += 1;
            self.host.world_mut(gui).unwrap().enqueue(Batch {
                id: 5000 + self.actions,
                operations: vec![Command::GuiAction {
                    target: ipp_core::GuiActionTarget {
                        entity: EntityRef::Handle(target.entity),
                        component: target.component,
                        incarnation: target.incarnation,
                    },
                    action,
                }],
            })?;
            Ok(())
        }

        /// Every routed request settled with an applied effect.
        fn release_input(&mut self) -> Result<()> {
            if let Some(context) = self.routing.take() {
                self.router.release(&mut self.host, context);
            }
            let terminals = self.terminals.borrow();
            if terminals.is_empty()
                || !terminals
                    .iter()
                    .all(|terminal| matches!(terminal, GuiDeliveryTerminal::Applied(_)))
            {
                return Err(format!("GUI input did not apply: {terminals:?}").into());
            }
            Ok(())
        }

        /// One Host frame: deliver requested bytes, advance World time by `dt`
        /// and present once, so per-frame cache counters describe this frame.
        fn frame(&mut self, dt: f64) -> Result<FrameStats> {
            let stats = self.present(dt)?;
            if stats.failed_draw_calls != 0 {
                return Err(format!("failed draws: {stats:?}").into());
            }
            Ok(stats)
        }

        /// One frame that may still skip draws while resources are re-uploaded.
        fn present(&mut self, dt: f64) -> Result<FrameStats> {
            self.tasks.poll_ready();
            self.host.progress_assets();
            self.tasks.poll_ready();
            for request in self.host.take_resource_requests() {
                let bytes = self
                    .payloads
                    .get(&request.source)
                    .ok_or_else(|| format!("unexpected cache request {}", request.source))?;
                self.host.complete_resource(request.id, Ok(bytes.clone()))?;
            }
            self.tasks.poll_ready();
            let frame = self.host.frame(dt)?;
            if !frame.publication_errors.is_empty() {
                return Err(format!("publication failures: {:?}", frame.publication_errors).into());
            }
            for report in frame.worlds.values() {
                for outcome in &report.as_ref().map_err(|error| error.to_string())?.outcomes {
                    outcome
                        .result
                        .as_ref()
                        .map_err(|error| format!("cache scene batch failed: {error:?}"))?;
                }
            }
            let selected = self
                .host
                .root_output(self.world)
                .map(|(output, _, publication)| (output, publication));
            self.renderer.prepare(&mut self.host, selected)?;
            self.host.progress_assets();
            self.tasks.poll_ready();
            Ok(self
                .renderer
                .draw_stats(&self.host, self.world, WIDTH, HEIGHT)?)
        }

        /// Present until the panel's presentation satisfies `done`: routed GUI
        /// input reaches interaction priority at a following update boundary.
        fn frame_until(
            &mut self,
            label: &str,
            done: impl Fn(SurfaceCachePresentation) -> bool,
        ) -> Result<FrameStats> {
            for _ in 0..4 {
                let stats = self.frame(DT)?;
                if done(self.record()?.presentation) {
                    return Ok(stats);
                }
                if stats.surface_cache_repaints != 0 {
                    return Err(format!("{label}: repainted while input was routed").into());
                }
            }
            Err(format!("{label}: presentation never changed: {:?}", self.record()?).into())
        }

        /// The panel Canvas output's completed paint and resource revisions.
        fn revisions(&self) -> Option<(u64, u64)> {
            let publication = self
                .host
                .output(
                    self.host.latest_publication(self.canvas.world().id())?,
                    self.canvas,
                )?
                .data::<CanvasPublication>()?;
            Some((publication.paint_revision, publication.resource_revision))
        }

        /// Whether every source has its data and GPU data resident.
        fn resident(&self, sources: &[AssetSource]) -> bool {
            let resources = self.host.asset_resources();
            sources.iter().all(|source| {
                resources
                    .find(source)
                    .and_then(|key| resources.get(key))
                    .is_some_and(|resource| {
                        resource.data().is_some() && resource.graphics_ready() == Some(true)
                    })
            })
        }

        /// Present until the panel's paint has stopped changing for two frames and
        /// the panel shows it directly under interaction or from a current image,
        /// so a transition the last input started has ended and the cache has
        /// caught up with it.
        fn settle_paint(&mut self, label: &str) -> Result<()> {
            let mut steady = 0;
            for _ in 0..120 {
                let before = self.revisions();
                self.frame(DT)?;
                let presentation = self.record()?.presentation;
                let current = presentation == SurfaceCachePresentation::Interaction
                    || presentation == SurfaceCachePresentation::Reused;
                steady = if self.revisions() == before && current {
                    steady + 1
                } else {
                    0
                };
                if steady >= 2 {
                    return Ok(());
                }
            }
            Err(format!("{label}: paint never settled: {:?}", self.record()?).into())
        }

        /// Present until every asset is ready and a frame uploads nothing.
        fn settle(&mut self, sources: &[AssetSource]) -> Result<FrameStats> {
            let mut last = FrameStats::default();
            for _ in 0..256 {
                last = self.present(DT)?;
                if self.resident(sources)
                    && last.uploaded_bytes == 0
                    && last.failed_draw_calls == 0
                    && last.draw_calls > 0
                {
                    return Ok(last);
                }
            }
            Err(format!("cache scene did not settle: {last:?}").into())
        }

        fn capture(&mut self, label: &str) -> Result<Vec<u8>> {
            let pixels = self.context.capture()?;
            smoke::world::save(self.evidence, label, &pixels)?;
            Ok(pixels)
        }

        fn diagnostics(&self) -> Vec<SurfaceCacheDiagnostic> {
            let mut out = Vec::new();
            self.renderer
                .surface_cache_diagnostics(self.world, &mut out);
            out
        }

        fn record(&self) -> Result<SurfaceCacheDiagnostic> {
            let records = self.diagnostics();
            records
                .iter()
                .find(|record| record.entity == self.panel)
                .copied()
                .ok_or_else(|| format!("no cache record for the panel: {records:?}").into())
        }

        fn world_time(&mut self) -> f64 {
            self.host.world_mut(self.world).unwrap().time()
        }

        fn note(&mut self, line: String) {
            println!("{line}");
            self.report.push_str(&line);
            self.report.push('\n');
        }

        /// Current content presented directly: move near, capture, restore.
        fn direct_reference(&mut self, label: &str, restore: f32) -> Result<Vec<u8>> {
            self.camera_distance(NEAR)?;
            let stats = self.frame(DT)?;
            if stats.surface_cache_repaints != 0 || stats.surface_cache_reuses != 0 {
                return Err(format!("{label}: near reference used the cache: {stats:?}").into());
            }
            let pixels = self.capture(label)?;
            self.camera_distance(restore)?;
            Ok(pixels)
        }

        fn compare(
            &mut self,
            label: &str,
            expected: &[u8],
            actual: &[u8],
            mean: f64,
            outlier_bound: bool,
        ) -> Result<()> {
            let regions = regions(expected, actual);
            let outside = (0..WIDTH * HEIGHT)
                .filter(|index| !(40..200).contains(&(index / WIDTH)))
                .map(|index| {
                    let offset = (index * 4) as usize;
                    (0..3)
                        .map(|channel| {
                            expected[offset + channel].abs_diff(actual[offset + channel])
                        })
                        .max()
                        .unwrap()
                })
                .max()
                .unwrap_or(0);
            self.note(format!(
                "{label}: regions={regions:?} outside_max={outside}"
            ));
            let failed = regions.iter().any(|(region_mean, _, outliers)| {
                *region_mean > mean || (outlier_bound && *outliers > MATCHED_OUTLIERS)
            }) || outside > 2;
            if failed {
                let diff: Vec<u8> = expected
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(actual.as_chunks::<4>().0)
                    .flat_map(|(a, b)| {
                        let d = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
                        [
                            d.saturating_mul(8),
                            d.saturating_mul(8),
                            d.saturating_mul(8),
                            255,
                        ]
                    })
                    .collect();
                smoke::world::save(self.evidence, &format!("{label}-diff"), &diff)?;
                return Err(format!("{label}: cached and direct differ beyond tolerance").into());
            }
            Ok(())
        }

        fn expect(
            &mut self,
            label: &str,
            stats: &FrameStats,
            presentation: SurfaceCachePresentation,
            repaints: u32,
            allocations: u32,
        ) -> Result<SurfaceCacheDiagnostic> {
            let record = self.record()?;
            self.note(format!(
                "{label}: mode={:?} band={} size={:?} repaints={}/{} reuses={} allocations={} direct={} fallbacks={} entries={} bytes={} uploaded={}",
                record.presentation,
                record.band,
                record.size,
                stats.surface_cache_repaints,
                record.repaints,
                stats.surface_cache_reuses,
                stats.surface_cache_allocations,
                stats.surface_cache_direct,
                stats.surface_cache_fallbacks,
                stats.surface_cache_entries,
                stats.surface_cache_resident_bytes,
                stats.uploaded_bytes,
            ));
            if record.presentation != presentation
                || stats.surface_cache_repaints != repaints
                || stats.surface_cache_allocations != allocations
            {
                return Err(format!(
                    "{label}: expected {presentation:?} with {repaints} repaints and {allocations} allocations, got {record:?} {stats:?}"
                )
                .into());
            }
            if presentation.is_direct() != (stats.surface_cache_direct == 1) {
                return Err(format!("{label}: direct count mismatch {stats:?}").into());
            }
            Ok(record)
        }

        fn finish(&self) -> Result<()> {
            std::fs::write(
                self.evidence.join("surface-cache-service.txt"),
                &self.report,
            )?;
            Ok(())
        }

        /// Direct baselines with an absent policy, then the full cache lifecycle.
        pub(super) fn run(mut self, rebuild: impl Fn() -> Result<GlesRenderDevice>) -> Result<()> {
            // Absent policy: no records and zero cache work, at any distance.
            let mut direct = BTreeMap::new();
            for (label, distance) in [("near", NEAR), ("band1", BAND1), ("band2", BAND2)] {
                self.camera_distance(distance)?;
                let stats = self.frame(DT)?;
                let counters = [
                    stats.surface_cache_repaints,
                    stats.surface_cache_reuses,
                    stats.surface_cache_direct,
                    stats.surface_cache_fallbacks,
                    stats.surface_cache_allocations,
                    stats.surface_cache_entries,
                    stats.surface_cache_resident_bytes,
                ];
                if counters != [0; 7] || !self.diagnostics().is_empty() {
                    return Err(format!("default policy did cache work: {stats:?}").into());
                }
                direct.insert(label, self.capture(&format!("direct-{label}"))?);
            }

            // The orthographic view keeps the panel's pixels fixed across distances,
            // which every cached-versus-direct comparison relies on.
            for label in ["band1", "band2"] {
                let difference = max_difference(&direct["near"], &direct[label]);
                if difference != 0 {
                    return Err(format!("direct {label} differs from near by {difference}").into());
                }
            }
            let covered = direct["near"]
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] > 200 && pixel[1] < 120 && pixel[2] < 90)
                .count();
            if covered < 500 {
                return Err(format!("gradient panel coverage too small: {covered}").into());
            }

            // Opt in while near: direct presentation, identical pixels.
            self.opt_in(true)?;
            self.camera_distance(NEAR)?;
            let stats = self.frame(DT)?;
            self.expect("near", &stats, SurfaceCachePresentation::Near, 0, 0)?;
            let near = self.capture("cache-near")?;
            if max_difference(&direct["near"], &near) != 0 {
                return Err("near presentation differs from direct".into());
            }

            // Band 1 cold: one allocation and one repaint at matched density.
            self.camera_distance(BAND1)?;
            let stats = self.frame(DT)?;
            let record = self.expect(
                "band1-cold",
                &stats,
                SurfaceCachePresentation::Repainted,
                1,
                1,
            )?;
            let bytes = BAND1_SIZE[0] * BAND1_SIZE[1] * 4;
            if record.band != 1
                || record.size != BAND1_SIZE
                || record.resident_bytes != bytes
                || stats.surface_cache_entries != 1
                || stats.surface_cache_resident_bytes != bytes
            {
                return Err(format!("band-1 image {record:?} {stats:?}").into());
            }
            let cold = self.capture("cache-band1")?;
            self.compare(
                "band1-vs-direct",
                &direct["near"],
                &cold,
                MATCHED_MEAN,
                true,
            )?;

            // Warm: the unchanged image is reused with no repaint or upload.
            let stats = self.frame(DT)?;
            self.expect("band1-warm", &stats, SurfaceCachePresentation::Reused, 0, 0)?;
            if stats.uploaded_bytes != 0 {
                return Err(format!("warm cached frame uploaded {stats:?}").into());
            }
            if max_difference(&cold, &self.capture("cache-band1-warm")?) != 0 {
                return Err("warm reuse changed pixels".into());
            }

            // Camera motion inside the band: composite only.
            self.camera_distance(BAND1_MOVED)?;
            let stats = self.frame(DT)?;
            self.expect(
                "band1-moved",
                &stats,
                SurfaceCachePresentation::Reused,
                0,
                0,
            )?;
            if max_difference(&cold, &self.capture("cache-band1-moved")?) != 0 {
                return Err("camera motion inside a band changed pixels".into());
            }

            // Coalesced edits: two edits before the deadline keep the presented
            // image, then one repaint shows the latest.
            let painted = self.record()?.painted_at;
            let interval = 1.0 / f64::from(POLICY.max_refresh_hz);
            self.text_color([0.2, 1.0, 0.3, 1.0])?;
            let stats = self.frame(DT)?;
            self.expect("edit-a", &stats, SurfaceCachePresentation::Reused, 0, 0)?;
            self.text_color([0.3, 0.6, 1.0, 1.0])?;
            let stats = self.frame(DT)?;
            self.expect("edit-b", &stats, SurfaceCachePresentation::Reused, 0, 0)?;
            let mut repainted_at = None;
            for _ in 0..12 {
                let stats = self.frame(DT)?;
                let time = self.world_time();
                if stats.surface_cache_repaints > 0 {
                    repainted_at = Some(time);
                    break;
                }
                if time - painted >= interval + DT {
                    return Err(format!("coalesced edit missed its deadline at {time}").into());
                }
            }
            let repainted_at = repainted_at.ok_or("coalesced edits never repainted")?;
            if repainted_at - painted < interval - 1e-6 {
                return Err(format!("repaint before the refresh interval: {repainted_at}").into());
            }
            self.note(format!(
                "coalesced repaint after {:.4}s",
                repainted_at - painted
            ));
            let latest = self.capture("cache-latest-edit")?;
            let reference = self.direct_reference("direct-latest-edit", BAND1_MOVED)?;
            self.compare(
                "latest-edit-vs-direct",
                &reference,
                &latest,
                MATCHED_MEAN,
                true,
            )?;
            self.frame(DT)?;

            // Continuous change repaints at the cap and never starves.
            let (start, mut repaints) = (self.world_time(), 0);
            for step in 0..30 {
                let shade = if step % 2 == 0 {
                    0.4
                } else {
                    0.9
                };
                self.text_color([shade, 0.9, 0.3, 1.0])?;
                repaints += self.frame(DT)?.surface_cache_repaints;
            }
            let elapsed = self.world_time() - start;
            let cap = (elapsed * f64::from(POLICY.max_refresh_hz)).floor() as u32;
            self.note(format!(
                "continuous: {repaints} repaints over {elapsed:.3}s (cap {cap})"
            ));
            if repaints > cap + 1 || repaints + 1 < cap {
                return Err(format!("continuous repaints {repaints} outside cap {cap}").into());
            }

            // A frozen clock never reaches the deadline.
            self.text_color([1.0, 1.0, 1.0, 1.0])?;
            for _ in 0..5 {
                let stats = self.frame(0.0)?;
                if stats.surface_cache_repaints != 0 {
                    return Err("repainted while World time was frozen".into());
                }
            }
            for _ in 0..8 {
                self.frame(DT)?;
            }

            // Resource replacement bypasses the cadence: the frame that sees the
            // new resource identity repaints or presents directly.
            let replacement = source(DRAWING_TYPE, "panel-replacement.ippd");
            let drawing = self.drawing;
            self.enqueue(
                self.canvas.world().id(),
                vec![Command::SetField {
                    entity: EntityRef::Handle(drawing),
                    component: ComponentValue::CANVAS_DRAWING,
                    field: FieldWrite {
                        offset: offset_of!(CanvasDrawing, source) as u32,
                        value: FieldValue::String(replacement.uri.clone()),
                    },
                }],
            )?;
            let revision = |scene: &Self| scene.revisions().map(|(_, resource)| resource);
            let before = revision(&self);
            let mut observed = false;
            for _ in 0..64 {
                let stats = self.frame(DT)?;
                if revision(&self) != before {
                    let record = self.record()?;
                    if record.presentation == SurfaceCachePresentation::Reused {
                        return Err(format!(
                            "resource replacement reused a stale image: {record:?} {stats:?}"
                        )
                        .into());
                    }
                    observed = true;
                    break;
                }
            }
            if !observed {
                return Err("resource replacement never changed the resource revision".into());
            }
            self.settle(&[replacement])?;

            // Band crossing: halved density at band 2, then hysteresis.
            self.camera_distance(BAND2)?;
            let stats = self.frame(DT)?;
            let record = self.expect("band2", &stats, SurfaceCachePresentation::Repainted, 1, 1)?;
            if record.band != 2 || record.size != BAND2_SIZE {
                return Err(format!("band-2 image {record:?}").into());
            }
            let reduced = self.capture("cache-band2")?;
            let reference = self.direct_reference("direct-band2", BAND2)?;
            self.compare("band2-vs-direct", &reference, &reduced, REDUCED_MEAN, false)?;
            self.frame(DT)?;
            for distance in [7.5, 8.5, 7.5, 8.5] {
                self.camera_distance(distance)?;
                let stats = self.frame(DT)?;
                let record = self.expect(
                    &format!("hysteresis-{distance}"),
                    &stats,
                    SurfaceCachePresentation::Reused,
                    0,
                    0,
                )?;
                if record.band != 2 {
                    return Err(format!("band changed inside hysteresis: {record:?}").into());
                }
            }
            self.camera_distance(BAND1)?;
            let stats = self.frame(DT)?;
            let record = self.expect(
                "band1-return",
                &stats,
                SurfaceCachePresentation::Repainted,
                1,
                1,
            )?;
            if record.band != 1 || record.size != BAND1_SIZE {
                return Err(format!("band-1 return {record:?}").into());
            }
            self.frame(DT)?;

            // Interaction: hover and focus present current content directly and
            // never leave a stale image behind on release. The default look's
            // hover fades in over 80 ms and out over 120 ms on the Host clock, so
            // each comparison waits for its transition to end.
            for label in ["hover", "focus"] {
                if label == "hover" {
                    // The "Go" button spans panel metres x 2.1..2.8, y 1.5..1.85;
                    // its centre in normalized viewport coordinates.
                    self.route(GuiPhysicalInput::PointerMove {
                        pointer: 1,
                        point: [2.45 / 4.0, (0.5 + 1.675) / 3.0],
                    })?;
                } else {
                    self.focus(GuiLocalAction::Focus(0))?;
                }
                let stats = self.frame_until(label, |presentation| {
                    presentation == SurfaceCachePresentation::Interaction
                })?;
                self.expect(label, &stats, SurfaceCachePresentation::Interaction, 0, 0)?;
                self.settle_paint(label)?;
                if self.record()?.presentation != SurfaceCachePresentation::Interaction {
                    return Err(
                        format!("{label} lost direct priority: {:?}", self.record()?).into(),
                    );
                }
                let promoted = self.capture(&format!("cache-{label}"))?;
                let reference = self.direct_reference(&format!("direct-{label}"), BAND1)?;
                if max_difference(&reference, &promoted) != 0 {
                    return Err(format!("{label} promotion differs from direct").into());
                }
                if label == "hover" {
                    // Leaving every panel ends hover.
                    self.route(GuiPhysicalInput::PointerMove {
                        pointer: 1,
                        point: [0.02, 0.02],
                    })?;
                } else {
                    self.focus(GuiLocalAction::Blur)?;
                }
                let stats = self.frame_until(label, |presentation| {
                    presentation != SurfaceCachePresentation::Interaction
                })?;
                let record = self.record()?;
                if record.presentation.is_direct()
                    && record.presentation != SurfaceCachePresentation::Near
                {
                    return Err(format!("{label} release kept direct priority: {record:?}").into());
                }
                self.settle_paint(&format!("{label} release"))?;
                let released = self.capture(&format!("cache-{label}-released"))?;
                let reference =
                    self.direct_reference(&format!("direct-{label}-released"), BAND1)?;
                self.note(format!(
                    "{label} release: {:?} repaints={}",
                    record.presentation, stats.surface_cache_repaints
                ));
                self.compare(
                    &format!("{label}-released-vs-direct"),
                    &reference,
                    &released,
                    MATCHED_MEAN,
                    true,
                )?;
            }
            self.release_input()?;

            // Mirrored rear view through the cache equals the direct rear view.
            self.turn_panel(true)?;
            let stats = self.frame(DT)?;
            if stats.surface_cache_repaints != 0 {
                return Err(format!("placement-only rear turn repainted: {stats:?}").into());
            }
            let rear = self.capture("cache-rear")?;
            let reference = self.direct_reference("direct-rear", BAND1)?;
            self.compare("rear-vs-direct", &reference, &rear, MATCHED_MEAN, true)?;
            self.turn_panel(false)?;
            self.frame(DT)?;
            self.frame(DT)?;

            // Budget exhaustion falls back to direct and evicts; restoring repaints.
            let budget = self.renderer.surface_cache_budget();
            self.renderer.set_surface_cache_budget(1024);
            let stats = self.frame(DT)?;
            self.expect("budget", &stats, SurfaceCachePresentation::Fallback, 0, 0)?;
            if stats.surface_cache_fallbacks != 1
                || stats.surface_cache_entries != 0
                || stats.surface_cache_resident_bytes != 0
            {
                return Err(format!("budget fallback kept an image: {stats:?}").into());
            }
            let fallback = self.capture("cache-fallback")?;
            let reference = self.direct_reference("direct-fallback", BAND1)?;
            if max_difference(&reference, &fallback) != 0 {
                return Err("budget fallback differs from direct".into());
            }
            self.renderer.set_surface_cache_budget(budget);
            let stats = self.frame(DT)?;
            self.expect(
                "budget-restored",
                &stats,
                SurfaceCachePresentation::Repainted,
                1,
                1,
            )?;
            self.frame(DT)?;
            let before_recovery = self.capture("cache-before-recovery")?;

            // Device replacement drops every image and every GPU resource. The
            // first repaint runs before the Host has re-uploaded the panel's
            // resources and skips what is missing; the frame on which they are
            // resident again repaints regardless of the refresh interval, with
            // unchanged paint and resource revisions.
            let revisions = self.revisions();
            self.renderer.replace_device(&mut self.host, rebuild()?)?;
            let sources = [
                source(FONT_TYPE, "shure-tech-mono.ippf"),
                source(DRAWING_TYPE, "panel-replacement.ippd"),
                source(TEXTURE_TYPE, "badge.ippt"),
            ];
            let mut timeline = Vec::new();
            let mut settled = None;
            for index in 0..256 {
                let stats = self.present(DT)?;
                let resident = self.resident(&sources);
                timeline.push((index, resident, stats.surface_cache_repaints));
                if resident
                    && stats.uploaded_bytes == 0
                    && stats.failed_draw_calls == 0
                    && stats.draw_calls > 0
                {
                    settled = Some(stats);
                    break;
                }
            }
            self.note(format!(
                "recovery timeline (frame, resident, repaints): {timeline:?}"
            ));
            let stats = settled.ok_or("recovery did not settle")?;
            let first_resident = timeline
                .iter()
                .find(|(_, resident, _)| *resident)
                .map(|(index, ..)| *index)
                .ok_or("resources never became resident")?;
            let repainted_first = timeline[0].2 == 1;
            let repainted_on_arrival = timeline[first_resident].2 == 1;
            if !repainted_first || (first_resident > 0 && !repainted_on_arrival) {
                return Err(format!(
                    "recovery must repaint at once and again when resources are resident: {timeline:?}"
                )
                .into());
            }
            if self.revisions() != revisions {
                return Err("device replacement changed the published revisions".into());
            }
            if stats.surface_cache_entries != 1 {
                return Err(
                    format!("cache did not recover after device replacement: {stats:?}").into(),
                );
            }
            let record = self.record()?;
            if record.presentation != SurfaceCachePresentation::Reused {
                return Err(format!("recovered image not reused: {record:?}").into());
            }
            let after_recovery = self.capture("cache-recovered")?;
            let difference = max_difference(&before_recovery, &after_recovery);
            self.note(format!(
                "recovery: first resident frame {first_resident}, max difference {difference}"
            ));
            if difference > RECOVERY_MAX {
                return Err(format!(
                    "recovery changed cached pixels by {difference} (record {record:?})"
                )
                .into());
            }
            let reference = self.direct_reference("direct-recovered", BAND1)?;
            self.compare(
                "recovered-vs-direct",
                &reference,
                &after_recovery,
                MATCHED_MEAN,
                true,
            )?;
            self.frame(DT)?;

            // Lifetimes: policy removal, re-add, entity deletion and forget_world.
            self.opt_in(false)?;
            let stats = self.frame(DT)?;
            if stats.surface_cache_entries != 0
                || stats.surface_cache_resident_bytes != 0
                || !self.diagnostics().is_empty()
            {
                return Err(format!("policy removal kept an image: {stats:?}").into());
            }
            self.opt_in(true)?;
            let stats = self.frame(DT)?;
            self.expect("readded", &stats, SurfaceCachePresentation::Repainted, 1, 1)?;
            let panel = self.panel;
            self.batch(vec![Command::Delete {
                entity: EntityRef::Handle(panel),
            }])?;
            let stats = self.frame(DT)?;
            if stats.surface_cache_entries != 0 || stats.surface_cache_resident_bytes != 0 {
                return Err(format!("deleted Surface kept an image: {stats:?}").into());
            }
            self.renderer.forget_world(self.world);
            if !self.diagnostics().is_empty() {
                return Err("forget_world kept records".into());
            }
            self.note(
                "cache active with core-prepared inputs: all service assertions passed".into(),
            );
            self.finish()
        }
    }
}

#[cfg(target_os = "linux")]
fn main() -> Result<()> {
    use std::path::PathBuf;

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: egl_surface_cache <EGL-GLES-library-directory> <surface-assets-directory> <font-assets-directory> <evidence-directory>".into());
    }
    let library_dir = PathBuf::from(&args[0]);
    let assets = PathBuf::from(&args[1]);
    let fonts = PathBuf::from(&args[2]);
    let evidence = PathBuf::from(&args[3]);
    std::fs::create_dir_all(&evidence)?;

    let context =
        smoke::egl::Context::new(&library_dir, smoke::world::WIDTH, smoke::world::HEIGHT)?;
    let mut device = context.device()?;
    smoke::surface_cache_target::run(&context, &mut device, &evidence)?;
    drop(device);
    smoke::error_checks::run(&context, &evidence)?;

    let mut renderer = ipp_render_gl::RenderService::new(context.device()?)?;
    let scene = scenario::Scene::new(&mut renderer, &context, &assets, &fonts, &evidence)?;
    scene.run(|| context.device())?;
    println!("PASS: Surface cache device targets and RenderService cache scenario");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL Surface cache runner supports Linux; no graphics test was run.");
    std::process::exit(1);
}
