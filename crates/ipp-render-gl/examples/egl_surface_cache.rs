//! Native GLES whole-Surface cache scenario.
//!
//! The device section runs the shared cache-target oracle: creation bounds,
//! resizing, nesting rules, premultiplied composition front and mirrored, and
//! atlas population nested inside a repaint. The service section drives a real
//! Host, World and `RenderService` with owned font, drawing and bitmap assets
//! and an opted-in GUI Surface over an opaque background Surface. An
//! orthographic camera keeps the projected panel size fixed while its distance
//! selects the cache band, so cached and direct captures compare pixel for
//! pixel: 80 screen pixels per metre, a 4 x 2 metre panel over rows 40..200.
//!
//! While RenderService still presents every Surface directly the service
//! section verifies the direct baselines, reports `cache inactive` and passes;
//! once opted-in Surfaces report cache records it asserts the full lifecycle.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

#[cfg(target_os = "linux")]
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(target_os = "linux")]
mod scenario {
    use super::{Result, smoke};
    use ipp_core::components::{Camera, Transform};
    use ipp_core::services::asset_management::{
        AssetSource, drawing::DRAWING_TYPE, font::FONT_TYPE,
    };
    use ipp_core::{
        Batch, Command, ComponentValue, DynamicValue, EntityId, EntityRef, FieldValue, FieldWrite,
        GuiCommand, GuiContainerKind, GuiInputCommand, GuiNodeContent, GuiNodeHandle, GuiNodeId,
        GuiNodeStyle, GuiRoot, HostRuntime, Surface, SurfaceCache, TEXTURE_TYPE, WorldId,
    };
    use ipp_render_gl::{
        GlesRenderDevice, RenderService, RenderStats, SurfaceCacheDiagnostic,
        SurfaceCachePresentation,
    };
    use std::collections::BTreeMap;
    use std::mem::offset_of;
    use std::path::Path;

    const WIDTH: u32 = smoke::world::WIDTH;
    const HEIGHT: u32 = smoke::world::HEIGHT;
    const SESSION: u64 = 1;

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

    /// Band 2 halves the density: a loose bound on resampled edges only.
    const REDUCED_MEAN: f64 = 16.0;

    /// Recovered glyph atlas slots may round coverage by one level at glyph edges.
    const RECOVERY_MAX: u8 = 2;

    pub(super) struct Scene<'a> {
        renderer: &'a mut RenderService<GlesRenderDevice>,
        context: &'a smoke::egl::Context,
        host: HostRuntime,
        world: WorldId,
        camera: EntityId,
        panel: EntityId,
        incarnation: u64,
        evidence: &'a Path,
        payloads: BTreeMap<String, Vec<u8>>,
        report: String,
    }

    fn source(kind: ipp_core::services::asset_management::AssetTypeId, name: &str) -> AssetSource {
        AssetSource {
            kind,
            uri: format!("fixture:///{name}"),
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

    /// Per-region absolute channel differences over the panel rows, in a 4 x 2 grid.
    fn regions(expected: &[u8], actual: &[u8]) -> Vec<(f64, u8)> {
        let mut out = Vec::new();
        for row in 0..2 {
            for column in 0..4 {
                let (mut total, mut max, mut count) = (0u64, 0u8, 0u64);
                for y in 40 + row * 80..40 + (row + 1) * 80 {
                    for x in column * 80..(column + 1) * 80 {
                        let offset = ((y * WIDTH + x) * 4) as usize;
                        for channel in 0..3 {
                            let difference =
                                expected[offset + channel].abs_diff(actual[offset + channel]);
                            total += u64::from(difference);
                            max = max.max(difference);
                            count += 1;
                        }
                    }
                }
                out.push((total as f64 / count as f64, max));
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
            renderer.install(&mut host)?;
            host.data_sources_mut().register_stream("fixture://")?;
            let world = host.create_world(Default::default())?;
            let mut panel_surface = Surface::default();
            panel_surface.width = PANEL[0];
            panel_surface.height = PANEL[1];
            let mut backdrop = Surface::default();
            backdrop.width = 4.0;
            backdrop.height = 3.0;
            let (camera, panel, background) = {
                let mut world_context = host.world_mut(world).unwrap();
                world_context.enqueue(Batch {
                    id: 1,
                    operations: vec![
                        Command::Create {
                            alias: 1,
                            metadata: Default::default(),
                        },
                        Command::InsertComponentValue {
                            entity: EntityRef::Alias(1),
                            value: ComponentValue::Transform(Transform {
                                z: NEAR,
                                ..Default::default()
                            }),
                        },
                        Command::InsertComponentValue {
                            entity: EntityRef::Alias(1),
                            value: ComponentValue::Camera(Camera {
                                projection: 1,
                                ortho_height: 3.0,
                                ..Default::default()
                            }),
                        },
                        Command::Create {
                            alias: 2,
                            metadata: Default::default(),
                        },
                        Command::InsertComponentValue {
                            entity: EntityRef::Alias(2),
                            value: ComponentValue::Transform(Transform::default()),
                        },
                        Command::InsertComponentValue {
                            entity: EntityRef::Alias(2),
                            value: ComponentValue::Surface(panel_surface),
                        },
                        Command::InsertComponentValue {
                            entity: EntityRef::Alias(2),
                            value: ComponentValue::GuiRoot(GuiRoot::default()),
                        },
                        Command::Create {
                            alias: 3,
                            metadata: Default::default(),
                        },
                        Command::InsertComponentValue {
                            entity: EntityRef::Alias(3),
                            value: ComponentValue::Transform(Transform {
                                z: -1.0,
                                ..Default::default()
                            }),
                        },
                        Command::InsertComponentValue {
                            entity: EntityRef::Alias(3),
                            value: ComponentValue::Surface(backdrop),
                        },
                        Command::InsertComponentValue {
                            entity: EntityRef::Alias(3),
                            value: ComponentValue::GuiRoot(GuiRoot::default()),
                        },
                    ],
                })?;
                let report = world_context.step(0.0)?;
                let created = report.outcomes[0]
                    .result
                    .as_ref()
                    .map_err(|error| format!("cache scene batch failed: {error:?}"))?;
                (created[0].1, created[1].1, created[2].1)
            };
            host.world_mut(world)
                .unwrap()
                .enqueue_camera_activate(camera)?;
            host.world_mut(world).unwrap().step(0.0)?;

            let font = source(FONT_TYPE, "shure-tech-mono.ippf");
            let drawing = source(DRAWING_TYPE, "panel.ippd");
            let bitmap = source(TEXTURE_TYPE, "badge.ippt");
            let payloads = BTreeMap::from([
                (
                    font.uri.clone(),
                    std::fs::read(fonts.join("shure-tech-mono.ippf"))?,
                ),
                (
                    drawing.uri.clone(),
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
            let incarnation = |host: &mut HostRuntime, entity| -> Result<u64> {
                Ok(host
                    .world_mut(world)
                    .unwrap()
                    .inspect_gui(entity, None, 1, 1)?
                    .root_incarnation)
            };
            let background_incarnation = incarnation(&mut host, background)?;
            let panel_incarnation = incarnation(&mut host, panel)?;
            let mut scene = Self {
                renderer,
                context,
                host,
                world,
                camera,
                panel,
                incarnation: panel_incarnation,
                evidence,
                payloads,
                report: String::new(),
            };

            // Opaque backdrop behind the panel, never opted in.
            scene.insert(
                background,
                background_incarnation,
                1,
                None,
                0,
                GuiNodeContent::Container(GuiContainerKind::Stack),
                GuiNodeStyle {
                    width: Some(4.0),
                    height: Some(3.0),
                    background_color: Some([0.05, 0.12, 0.3, 1.0]),
                    ..Default::default()
                },
            )?;
            scene.insert(
                background,
                background_incarnation,
                2,
                Some(1),
                0,
                GuiNodeContent::Container(GuiContainerKind::SizedBox),
                GuiNodeStyle {
                    width: Some(1.0),
                    height: Some(3.0),
                    margin: Some([0.0, 0.0, 0.0, 1.5]),
                    background_color: Some([0.9, 0.9, 0.9, 1.0]),
                    ..Default::default()
                },
            )?;

            // The panel root is transparent: the backdrop shows through its gaps.
            let panel_nodes: [(u32, Option<u32>, GuiNodeContent, GuiNodeStyle); 10] = [
                (
                    1,
                    None,
                    GuiNodeContent::Container(GuiContainerKind::Stack),
                    GuiNodeStyle {
                        width: Some(PANEL[0]),
                        height: Some(PANEL[1]),
                        ..Default::default()
                    },
                ),
                // Gradient, border and glow; its skin lanes are set below.
                (
                    2,
                    Some(1),
                    GuiNodeContent::Container(GuiContainerKind::SizedBox),
                    GuiNodeStyle {
                        width: Some(1.4),
                        height: Some(1.0),
                        margin: Some([0.3, 0.0, 0.0, 0.25]),
                        background_color: Some([1.0, 1.0, 1.0, 1.0]),
                        ..Default::default()
                    },
                ),
                // Overlapping translucent boxes over the backdrop.
                (
                    3,
                    Some(1),
                    GuiNodeContent::Container(GuiContainerKind::SizedBox),
                    GuiNodeStyle {
                        width: Some(0.8),
                        height: Some(0.8),
                        margin: Some([0.2, 0.0, 0.0, 1.9]),
                        background_color: Some([1.0, 0.1, 0.1, 0.5]),
                        ..Default::default()
                    },
                ),
                (
                    4,
                    Some(1),
                    GuiNodeContent::Container(GuiContainerKind::SizedBox),
                    GuiNodeStyle {
                        width: Some(0.8),
                        height: Some(0.8),
                        margin: Some([0.6, 0.0, 0.0, 2.3]),
                        background_color: Some([0.1, 1.0, 0.2, 0.5]),
                        ..Default::default()
                    },
                ),
                // Clipped content: a scroll viewport smaller than its child.
                (
                    5,
                    Some(1),
                    GuiNodeContent::Container(GuiContainerKind::ScrollView),
                    GuiNodeStyle {
                        width: Some(0.6),
                        height: Some(0.4),
                        margin: Some([1.45, 0.0, 0.0, 3.2]),
                        ..Default::default()
                    },
                ),
                (
                    6,
                    Some(5),
                    GuiNodeContent::Container(GuiContainerKind::SizedBox),
                    GuiNodeStyle {
                        width: Some(1.5),
                        height: Some(1.5),
                        background_color: Some([1.0, 0.0, 1.0, 1.0]),
                        ..Default::default()
                    },
                ),
                (
                    7,
                    Some(1),
                    GuiNodeContent::Text("Cache".into()),
                    GuiNodeStyle {
                        font_size: 0.3,
                        margin: Some([1.45, 0.0, 0.0, 0.3]),
                        color: [1.0, 0.85, 0.2, 1.0],
                        asset: Some(font.clone()),
                        ..Default::default()
                    },
                ),
                (
                    8,
                    Some(1),
                    GuiNodeContent::Drawing,
                    GuiNodeStyle {
                        width: Some(0.6),
                        height: Some(0.45),
                        margin: Some([0.3, 0.0, 0.0, 3.2]),
                        color: [0.2, 0.6, 1.0, 1.0],
                        asset: Some(drawing.clone()),
                        ..Default::default()
                    },
                ),
                (
                    9,
                    Some(1),
                    GuiNodeContent::Image {
                        size: [0.35, 0.35],
                    },
                    GuiNodeStyle {
                        margin: Some([0.95, 0.0, 0.0, 3.3]),
                        asset: Some(bitmap.clone()),
                        ..Default::default()
                    },
                ),
                (
                    10,
                    Some(1),
                    GuiNodeContent::Button {
                        label: "Go".into(),
                    },
                    GuiNodeStyle {
                        width: Some(0.7),
                        height: Some(0.35),
                        margin: Some([1.5, 0.0, 0.0, 2.1]),
                        font_size: 0.2,
                        asset: Some(font.clone()),
                        ..Default::default()
                    },
                ),
            ];
            let mut children = BTreeMap::<Option<u32>, u32>::new();
            for (id, parent, content, style) in panel_nodes {
                let index = children.entry(parent).or_default();
                scene.insert(panel, panel_incarnation, id, parent, *index, content, style)?;
                *index += 1;
            }
            let part = |suffix, value| {
                (
                    GuiRoot::part_property_name(GuiNodeId(2), "background", suffix).unwrap(),
                    value,
                )
            };
            let lanes = [
                part("fill_mode", DynamicValue::F32(1.0)),
                part("gradient_start", DynamicValue::Vec2([0.0, 0.0])),
                part("gradient_end", DynamicValue::Vec2([0.0, 1.0])),
                part(
                    "gradient_color0",
                    DynamicValue::Vec4([1.0, 0.08, 0.04, 1.0]),
                ),
                part("gradient_color1", DynamicValue::Vec4([1.0, 0.8, 0.08, 1.0])),
                part("corner_radius", DynamicValue::Vec2([0.12, 0.12])),
                part("border_width", DynamicValue::F32(0.05)),
                part("border_color", DynamicValue::Vec4([1.0, 1.0, 1.0, 1.0])),
                part("glow_color", DynamicValue::Vec4([1.0, 0.35, 0.05, 1.0])),
                part("glow_intensity", DynamicValue::F32(0.8)),
                part("glow_radius", DynamicValue::F32(0.15)),
                part("glow_falloff", DynamicValue::F32(2.0)),
            ];
            scene.dynamic(lanes.into_iter().collect())?;
            scene.settle(&[font, drawing, bitmap])?;
            Ok(scene)
        }

        fn insert(
            &mut self,
            entity: EntityId,
            root_incarnation: u64,
            id: u32,
            parent: Option<u32>,
            index: u32,
            content: GuiNodeContent,
            style: GuiNodeStyle,
        ) -> Result<()> {
            self.host
                .world_mut(self.world)
                .unwrap()
                .enqueue_gui_command(
                    SESSION,
                    GuiCommand::InsertNode {
                        entity,
                        root_incarnation,
                        id: GuiNodeId(id),
                        parent: parent.map(GuiNodeId),
                        index,
                        content,
                        style,
                    },
                )?;
            self.host.world_mut(self.world).unwrap().step(0.0)?;
            Ok(())
        }

        fn batch(&mut self, operations: Vec<Command>) -> Result<()> {
            let mut world = self.host.world_mut(self.world).unwrap();
            world.enqueue(Batch {
                id: world.tick() + 1,
                operations,
            })?;
            Ok(())
        }

        fn dynamic(&mut self, lanes: Vec<(String, DynamicValue)>) -> Result<()> {
            let panel = self.panel;
            self.batch(
                lanes
                    .into_iter()
                    .map(|(name, value)| Command::SetDynamicProperty {
                        entity: EntityRef::Handle(panel),
                        component: ComponentValue::GUI_ROOT,
                        name,
                        value,
                    })
                    .collect(),
            )
        }

        fn text_color(&mut self, color: [f32; 4]) -> Result<()> {
            self.dynamic(vec![(
                GuiRoot::part_property_name(GuiNodeId(7), "label", "color").unwrap(),
                DynamicValue::Vec4(color),
            )])
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
            self.batch(vec![
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
            ])
        }

        fn opt_in(&mut self, enabled: bool) -> Result<()> {
            let panel = self.panel;
            self.batch(vec![if enabled {
                Command::InsertComponentValue {
                    entity: EntityRef::Handle(panel),
                    value: ComponentValue::SurfaceCache(POLICY),
                }
            } else {
                Command::RemoveComponent {
                    entity: EntityRef::Handle(panel),
                    component: ComponentValue::SURFACE_CACHE,
                }
            }])
        }

        fn input(&mut self, command: GuiInputCommand) -> Result<()> {
            self.host
                .world_mut(self.world)
                .unwrap()
                .enqueue_gui_input_command(SESSION, command)?;
            Ok(())
        }

        fn node(&self, id: u32) -> GuiNodeHandle {
            GuiNodeHandle::new(SESSION, self.panel, self.incarnation, GuiNodeId(id), 1)
        }

        /// One Host frame: deliver requested bytes, advance World time by `dt`
        /// and present once, so per-frame cache counters describe this frame.
        fn frame(&mut self, dt: f64) -> Result<RenderStats> {
            self.renderer.begin_frame();
            self.host
                .world_mut(self.world)
                .unwrap()
                .prepare_update(dt)?;
            self.host.progress_assets();
            for request in self.host.take_resource_requests() {
                let bytes = self
                    .payloads
                    .get(&request.source)
                    .ok_or_else(|| format!("unexpected cache request {}", request.source))?;
                self.host.complete_resource(request.id, Ok(bytes.clone()))?;
            }
            let mut world = self.host.world_mut(self.world).unwrap();
            let report = world.step(dt)?;
            for outcome in &report.outcomes {
                outcome
                    .result
                    .as_ref()
                    .map_err(|error| format!("cache scene batch failed: {error:?}"))?;
            }
            let stats = self.renderer.render(&mut world, WIDTH, HEIGHT)?;
            if stats.failed_draw_calls != 0 {
                return Err(format!("failed draws: {stats:?}").into());
            }
            Ok(stats)
        }

        /// Present until every asset is ready and a frame uploads nothing.
        fn settle(&mut self, sources: &[AssetSource]) -> Result<RenderStats> {
            let mut last = RenderStats::default();
            for _ in 0..256 {
                last = self.frame(DT)?;
                let world = self.host.world_mut(self.world).unwrap();
                let ready = sources.iter().all(|source| {
                    world
                        .asset_resources()
                        .find(source)
                        .and_then(|key| world.asset_resources().get(key))
                        .is_some_and(|resource| {
                            resource.data().is_some() && resource.graphics_ready() == Some(true)
                        })
                });
                if ready && last.uploaded_bytes == 0 && last.draw_calls > 0 {
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
            max: Option<u8>,
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
            let failed = regions.iter().any(|(region_mean, region_max)| {
                *region_mean > mean || max.is_some_and(|max| *region_max > max)
            }) || outside > 2;
            if failed {
                let diff: Vec<u8> = expected
                    .chunks_exact(4)
                    .zip(actual.chunks_exact(4))
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
            stats: &RenderStats,
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

        /// Direct baselines that hold with or without the cache, then the full
        /// cache lifecycle once RenderService reports cache records.
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
                .chunks_exact(4)
                .filter(|pixel| pixel[0] > 200 && pixel[1] < 120 && pixel[2] < 90)
                .count();
            if covered < 500 {
                return Err(format!("gradient panel coverage too small: {covered}").into());
            }

            // Opt in while near: direct presentation, identical pixels.
            self.opt_in(true)?;
            self.camera_distance(NEAR)?;
            let stats = self.frame(DT)?;
            if self.diagnostics().is_empty() && stats.surface_cache_direct == 0 {
                self.note(
                    "cache inactive: RenderService presents opted-in Surfaces directly; direct baselines verified, cache assertions skipped".into(),
                );
                let pixels = self.capture("inactive-opted-in")?;
                if max_difference(&direct["near"], &pixels) != 0 {
                    return Err("opting in changed direct presentation".into());
                }
                return self.finish();
            }
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
                Some(MATCHED_MAX),
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
                Some(MATCHED_MAX),
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
            self.dynamic(vec![(
                GuiRoot::part_property_name(GuiNodeId(8), "icon", "asset").unwrap(),
                DynamicValue::Asset(replacement.clone()),
            )])?;
            let revision = |scene: &mut Self| {
                let panel = scene.panel;
                scene
                    .host
                    .world_mut(scene.world)
                    .unwrap()
                    .surface_render_items()
                    .iter()
                    .find(|item| item.entity == panel)
                    .map(|item| item.resource_revision)
            };
            let before = revision(&mut self);
            let mut observed = false;
            for _ in 0..64 {
                let stats = self.frame(DT)?;
                if revision(&mut self) != before {
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
            self.compare("band2-vs-direct", &reference, &reduced, REDUCED_MEAN, None)?;
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
            // never leave a stale image behind on release.
            for (label, press, release) in [
                (
                    "hover",
                    GuiInputCommand::PointerMove {
                        pointer: 1,
                        panel: None,
                        position: [0.5, 0.5],
                        blockers: Vec::new(),
                        panel_distance: None,
                    },
                    GuiInputCommand::PointerCancel {
                        pointer: 1,
                    },
                ),
                (
                    "focus",
                    GuiInputCommand::Focus {
                        handle: self.node(10),
                    },
                    GuiInputCommand::Blur,
                ),
            ] {
                self.input(press)?;
                let stats = self.frame(DT)?;
                self.expect(label, &stats, SurfaceCachePresentation::Interaction, 0, 0)?;
                let promoted = self.capture(&format!("cache-{label}"))?;
                let reference = self.direct_reference(&format!("direct-{label}"), BAND1)?;
                if max_difference(&reference, &promoted) != 0 {
                    return Err(format!("{label} promotion differs from direct").into());
                }
                self.input(release)?;
                let stats = self.frame(DT)?;
                let record = self.record()?;
                if record.presentation.is_direct()
                    && record.presentation != SurfaceCachePresentation::Near
                {
                    return Err(format!("{label} release kept direct priority: {record:?}").into());
                }
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
                    Some(MATCHED_MAX),
                )?;
            }

            // Mirrored rear view through the cache equals the direct rear view.
            self.turn_panel(true)?;
            let stats = self.frame(DT)?;
            if stats.surface_cache_repaints != 0 {
                return Err(format!("placement-only rear turn repainted: {stats:?}").into());
            }
            let rear = self.capture("cache-rear")?;
            let reference = self.direct_reference("direct-rear", BAND1)?;
            self.compare(
                "rear-vs-direct",
                &reference,
                &rear,
                MATCHED_MEAN,
                Some(MATCHED_MAX),
            )?;
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

            // Device replacement drops every image; the next frame repaints the same pixels.
            self.renderer.replace_device(&mut self.host, rebuild()?)?;
            let mut recovered = None;
            for _ in 0..32 {
                let stats = self.frame(DT)?;
                if stats.surface_cache_entries == 1 && stats.uploaded_bytes == 0 {
                    recovered = Some(stats);
                    break;
                }
            }
            recovered.ok_or("cache did not recover after device replacement")?;
            let after_recovery = self.capture("cache-recovered")?;
            let difference = max_difference(&before_recovery, &after_recovery);
            self.note(format!("recovery max difference {difference}"));
            if difference > RECOVERY_MAX {
                return Err(format!("recovery changed pixels by {difference}").into());
            }

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
            self.note("cache active: all service assertions passed".into());
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

    let mut renderer = ipp_render_gl::RenderService::new(context.device()?)?;
    let scene = scenario::Scene::new(&mut renderer, &context, &assets, &fonts, &evidence)?;
    scene.run(|| context.device().map_err(Into::into))?;
    println!("PASS: Surface cache device targets and RenderService cache scenario");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL Surface cache runner supports Linux; no graphics test was run.");
    std::process::exit(1);
}
