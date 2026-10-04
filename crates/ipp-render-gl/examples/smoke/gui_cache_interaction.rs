//! Real GUI delivery, cache eligibility and retained recovery on the EGL driver.

use super::canvas_publications::{canvas, place};
use super::publications::{assert_color, camera, create, save};
use ipp_core::components::rows::Rows;
use ipp_core::components::{FlatSurface, GuiButton, GuiLayout, Scalar, SurfaceCache, Transform};
use ipp_core::services::gui_input::{
    GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputContext, GuiInputLimits,
    GuiInputService, GuiInputSession, GuiPointerLease,
};
use ipp_core::systems::canvas::CanvasPublication;
use ipp_core::systems::gui::GuiPrimitivePart;
use ipp_core::systems::gui::local::{
    GuiEntityTarget, GuiInteractionFlags, GuiInteractionUpdate, GuiLocalAction, GuiLocalCommand,
    GuiLocalEffect,
};
use ipp_core::systems::gui::presentation::{GuiPaintPart, GuiSkin, GuiTheme};
use ipp_core::systems::gui::{GuiPartId, GuiSkinState, GuiSystem};
use ipp_core::systems::*;
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, ErrorReason, HostRuntime, OutputRef,
    WorldAttachment, WorldAttachmentToken, WorldId, WorldViewport,
};
use ipp_render_gl::{RenderDevice, RenderService, RenderStatistics, SurfaceCachePresentation};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const VIEW: WorldViewport = WorldViewport {
    width: 256,
    height: 256,
    device_pixel_ratio: 1.0,
};
const RED: [f32; 3] = [0.5, 0.0, 0.0];
const GREEN: [f32; 3] = [0.0, 0.5, 0.0];
const BLUE: [f32; 3] = [0.0, 0.0, 0.5];
const YELLOW: [f32; 3] = [0.5, 0.5, 0.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultLocation {
    Canvas,
    ContainingCamera,
    Spatial,
    NestedCamera,
}

struct FaultFactory(Arc<AtomicBool>);

struct CommitFault(Arc<AtomicBool>, usize);

impl SystemFactory for FaultFactory {
    fn id(&self) -> SystemId {
        SystemId("fixture.gles-cache-fault")
    }

    fn create(
        &self,
        _: &mut SystemInitContext<'_>,
    ) -> std::result::Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(CommitFault(self.0.clone(), 0)))
    }
}

impl System for CommitFault {
    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        if !self.0.load(Ordering::Relaxed) {
            return;
        }
        let targets: Vec<_> = context
            .changed_components()
            .filter(|(_, component)| *component == ComponentValue::SCALAR)
            .map(|(entity, _)| entity)
            .collect();
        for entity in targets {
            self.1 += 1;
            context.restore_evaluated_component(
                entity,
                ComponentValue::Scalar(Scalar {
                    value: self.1 as f32 + 100.0,
                }),
            );
        }
    }
}

struct Permit(Rc<RefCell<Vec<GuiDeliveryTerminal>>>);

impl GuiDeliveryPermit for Permit {
    fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> std::result::Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
        self.0.borrow_mut().push(terminal);
    }
}

struct Input {
    service: GuiInputService,
    session: GuiInputSession,
    context: GuiInputContext,
    terminals: Rc<RefCell<Vec<GuiDeliveryTerminal>>>,
    next: u64,
    next_batch: u64,
    pointer: Option<GuiPointerLease>,
}

impl Input {
    fn new(host: &HostRuntime, root: OutputRef) -> Result<Self> {
        let service = GuiInputService::new(GuiInputLimits {
            pointer_leases: 1,
            ..Default::default()
        });
        let session = service.open_session()?;
        let context = service.bind_context(host, &session, root.world())?.context;
        Ok(Self {
            service,
            session,
            context,
            terminals: Default::default(),
            next: 0,
            next_batch: 0,
            pointer: None,
        })
    }

    /// Submit one `GuiAction` command; the next frame applies it.
    fn action(
        &mut self,
        host: &mut HostRuntime,
        panel: Panel,
        action: GuiLocalAction,
    ) -> Result<()> {
        let target = panel.target(host);
        self.next_batch += 1;
        host.world_mut(panel.output.world().id())
            .unwrap()
            .enqueue(Batch {
                id: 1000 + self.next_batch,
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

    fn feedback(
        &mut self,
        host: &mut HostRuntime,
        panel: Panel,
        path: &[WorldAttachmentToken],
        update: GuiInteractionUpdate,
    ) -> Result<()> {
        let target = panel.target(host);
        self.next += 1;
        let input = self.service.reserve_routed(
            host,
            &self.context,
            target,
            self.next,
            path,
            Box::new(Permit(self.terminals.clone())),
        )?;
        if self.pointer.is_none() {
            self.pointer = Some(self.service.pointer_lease(&input, 1)?);
        }
        let command =
            GuiLocalCommand::interaction(input, self.pointer.as_ref().unwrap().clone(), update)?;
        host.world_mut(panel.output.world().id())
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 77, command)?;
        Ok(())
    }

    fn settled(&self) {
        assert_eq!(self.service.pending_count(), 0);
        assert_eq!(self.terminals.borrow().len(), self.next as usize);
        assert!(
            self.terminals
                .borrow()
                .iter()
                .all(|terminal| matches!(terminal, GuiDeliveryTerminal::Applied(_))),
            "{:?}",
            self.terminals.borrow()
        );
    }

    fn drop_cancelled_pointer(&mut self) {
        self.settled();
        let pointer = self.pointer.take().expect("cancelled pointer");
        assert!(!pointer.is_live());
        drop(pointer);
    }

    fn close(self) {
        self.settled();
        assert!(self.pointer.is_none());
        self.service.release_context(&self.context);
        self.service.close_session(&self.session);
    }
}

#[derive(Clone, Copy)]
struct Panel {
    output: OutputRef,
    control: EntityId,
    theme: EntityId,
    owner: WorldId,
    anchor: EntityId,
}

impl Panel {
    /// The control's exact lifetime, as its latest Canvas publication hit carries it.
    fn target(self, host: &HostRuntime) -> GuiEntityTarget {
        let hit = self
            .publication(host)
            .hits
            .iter()
            .find(|hit| hit.target.entity == self.control)
            .unwrap();

        GuiEntityTarget::from_canvas(self.output.world(), hit.target)
    }

    /// Whether the `GuiFocus` System query reports this control.
    fn focused(self, host: &mut HostRuntime) -> bool {
        host.world_mut(self.output.world().id())
            .unwrap()
            .gui_focus_page(0, self.control.to_bits(), 1)
            .iter()
            .any(|record| record.target.entity == self.control)
    }

    /// The control's feedback aggregated over the `GuiPointers` System query.
    fn interaction(self, host: &mut HostRuntime) -> GuiInteractionFlags {
        host.world_mut(self.output.world().id())
            .unwrap()
            .gui_pointer_page(0, self.control.to_bits(), usize::MAX)
            .iter()
            .fold(GuiInteractionFlags::default(), |flags, record| {
                GuiInteractionFlags {
                    hovered: flags.hovered || record.state.hovered,
                    pressed: flags.pressed || record.state.pressed,
                    captured: flags.captured || record.state.captured,
                }
            })
    }

    fn publication(self, host: &HostRuntime) -> &CanvasPublication {
        host.output(
            host.latest_publication(self.output.world().id()).unwrap(),
            self.output,
        )
        .unwrap()
        .data()
        .unwrap()
    }
}

fn theme(color: [f32; 3]) -> Result<GuiTheme> {
    let mut parts = Rows::new();
    for (state, color) in [
        (GuiSkinState::Idle, color),
        (GuiSkinState::Hovered, GREEN),
        (GuiSkinState::Pressed, BLUE),
    ] {
        parts
            .push(GuiPaintPart {
                color: Some([color[0], color[1], color[2], 1.0]),
                ..GuiPaintPart::keyed(GuiPartId::state(GuiPrimitivePart::Background, state))?
            })
            .unwrap();
    }
    Ok(GuiTheme {
        parts,
        ..Default::default()
    })
}

fn surface(
    host: &mut HostRuntime,
    parent: WorldId,
    child: OutputRef,
    x: f32,
    extent: f32,
    cached: bool,
) -> Result<EntityId> {
    let surface = FlatSurface {
        width: extent,
        height: extent,
        ..Default::default()
    };
    let mut values = vec![
        ComponentValue::Transform(Transform {
            x,
            ..Default::default()
        }),
        ComponentValue::FlatSurface(surface),
        ComponentValue::WorldAttachment(WorldAttachment::surface(child)),
    ];
    if cached {
        values.push(ComponentValue::SurfaceCache(SurfaceCache {
            direct_distance: 0.0,
            texels_per_metre: 128.0,
            max_refresh_hz: 10.0,
        }));
    }
    create(host, parent, values)
}

fn panel(host: &mut HostRuntime, owner: WorldId, x: f32, color: [f32; 3]) -> Result<Panel> {
    let world = host.create_world(
        Default::default(),
        &super::selection::with_fixtures(
            host,
            [
                super::selection::panel(),
                super::selection::CONSTRAINTS.to_vec(),
            ]
            .concat(),
        ),
    )?;
    let output = canvas(host, world, 100.0)?;
    let theme = create(host, world, vec![ComponentValue::GuiTheme(theme(color)?)])?;
    let control = place(
        host,
        output,
        vec![
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 80.0,
                height: 80.0,
                ..Default::default()
            }),
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                ..Default::default()
            }),
        ],
    )?;
    let anchor = surface(host, owner, output, x, 0.8, true)?;
    Ok(Panel {
        output,
        control,
        theme,
        owner,
        anchor,
    })
}

struct Scene {
    host: HostRuntime,
    root: OutputRef,
    left: Panel,
    right: Panel,
    fault_world: WorldId,
    oscillator: EntityId,
    enabled: Arc<AtomicBool>,
    time: f64,
}

impl Scene {
    fn new<D: RenderDevice>(
        renderer: &mut RenderService<D>,
        location: FaultLocation,
    ) -> Result<Self> {
        let enabled = Arc::new(AtomicBool::new(false));
        let mut factories = compiled_system_factories();
        factories.push(Arc::new(FaultFactory(enabled.clone())));
        let mut host = HostRuntime::with_system_factories(factories)?;
        renderer.install(&mut host)?;
        let parent = host.create_world(
            Default::default(),
            &super::selection::with_fixtures(
                &host,
                [
                    super::selection::scene(),
                    super::selection::CONSTRAINTS.to_vec(),
                ]
                .concat(),
            ),
        )?;
        let root = camera(&mut host, parent, 2.0)?;
        host.set_root_output(root, VIEW)?;
        let (owner, ancestor, x) = match location {
            FaultLocation::Canvas | FaultLocation::ContainingCamera => (parent, parent, -0.5),
            FaultLocation::Spatial => {
                let middle = host.create_world(
                    Default::default(),
                    &super::selection::with_fixtures(
                        &host,
                        super::selection::select(&[
                            super::selection::ATTACHMENTS,
                            super::selection::CONSTRAINTS,
                        ]),
                    ),
                )?;
                let owner = host.create_world(Default::default(), &super::selection::scene())?;
                for (parent, child) in [(parent, middle), (middle, owner)] {
                    let child = host.world_ref(child).unwrap();
                    create(
                        &mut host,
                        parent,
                        vec![ComponentValue::WorldAttachment(WorldAttachment::spatial(
                            child,
                        ))],
                    )?;
                }
                (owner, middle, -0.5)
            }
            FaultLocation::NestedCamera => {
                let owner = host.create_world(
                    Default::default(),
                    &super::selection::with_fixtures(
                        &host,
                        [
                            super::selection::scene(),
                            super::selection::CONSTRAINTS.to_vec(),
                        ]
                        .concat(),
                    ),
                )?;
                let output = camera(&mut host, owner, 0.8)?;
                surface(&mut host, parent, output, -0.5, 0.8, false)?;
                (owner, owner, 0.0)
            }
        };
        let left = panel(&mut host, owner, x, RED)?;
        let right = panel(&mut host, parent, 0.5, GREEN)?;
        let fault_world = if location == FaultLocation::Canvas {
            left.output.world().id()
        } else {
            ancestor
        };
        let oscillator = create(
            &mut host,
            fault_world,
            vec![ComponentValue::Scalar(Scalar::default())],
        )?;
        Ok(Self {
            host,
            root,
            left,
            right,
            fault_world,
            oscillator,
            enabled,
            time: 0.0,
        })
    }

    fn draw<D: RenderDevice>(
        &mut self,
        renderer: &mut RenderService<D>,
    ) -> Result<(RenderStatistics, ipp_core::HostFrameReport)> {
        self.time += 0.2;
        let report = self.host.frame(0.2)?;
        if self.enabled.load(Ordering::Relaxed) {
            assert_eq!(
                report.publication_errors,
                std::collections::BTreeMap::from([(
                    self.fault_world,
                    ErrorReason::NonConvergentCommit.to_string()
                ),])
            );
        } else {
            assert!(
                report.publication_errors.is_empty(),
                "{:?}",
                report.publication_errors
            );
        }
        assert!(report.worlds.values().all(std::result::Result::is_ok));
        let publication = self.host.root_output(self.root.world().id()).unwrap().2;
        renderer.prepare(&mut self.host, Some((self.root, publication)))?;
        self.host.progress_assets();
        let summary = renderer.draw(&self.host, self.root, publication, VIEW, self.time)?;
        assert_eq!(summary.failed_draw_calls, 0);
        assert!(!summary.invalid_camera);
        Ok((*renderer.statistics(), report))
    }

    fn image(&self, pixels: &[u8], left: [f32; 3]) {
        assert_color(pixels, 64, 128, left);
        assert_color(pixels, 192, 128, GREEN);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] > 120 || pixel[1] > 120 || pixel[2] > 120)
                .count()
                > 10_000
        );
    }

    fn presentations<D: RenderDevice>(&self, renderer: &RenderService<D>, interaction: [bool; 2]) {
        for (panel, interaction) in [self.left, self.right].into_iter().zip(interaction) {
            let mut diagnostics = Vec::new();
            renderer.surface_cache_diagnostics(panel.owner, &mut diagnostics);
            let record = diagnostics
                .iter()
                .find(|record| record.entity == panel.anchor)
                .unwrap();
            if interaction {
                assert_eq!(
                    record.presentation,
                    SurfaceCachePresentation::Interaction,
                    "{:?}: {record:?}",
                    panel.output
                );
            } else {
                assert!(
                    matches!(
                        record.presentation,
                        SurfaceCachePresentation::Reused | SurfaceCachePresentation::Repainted
                    ),
                    "{:?}: {record:?}",
                    panel.output
                );
            }
        }
    }

    fn path(&self) -> Vec<WorldAttachmentToken> {
        let publication = self
            .host
            .publication(self.host.latest_publication(self.left.owner).unwrap())
            .unwrap();
        vec![
            publication
                .attachments
                .iter()
                .find(|edge| edge.anchor == self.left.anchor)
                .unwrap()
                .token
                .clone(),
        ]
    }

    fn close<D: RenderDevice>(mut self, renderer: &mut RenderService<D>) -> Result<()> {
        renderer.prepare(&mut self.host, None)?;
        renderer.unload_host(&mut self.host)?;
        Ok(())
    }
}

fn focus_return<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    artifacts: &Path,
    evidence: &mut String,
) -> Result<()> {
    let mut scene = Scene::new(renderer, FaultLocation::Canvas)?;
    for _ in 0..3 {
        scene.draw(renderer)?;
    }
    let mut input = Input::new(&scene.host, scene.root)?;
    let initial = capture()?;
    scene.image(&initial, RED);
    input.action(&mut scene.host, scene.left, GuiLocalAction::Focus(0))?;
    let (focused, _) = scene.draw(renderer)?;
    assert_eq!(focused.surface_cache_direct, 1);
    assert_eq!(focused.surface_cache_entries, 2);
    scene.presentations(renderer, [true, false]);
    assert!(scene.left.focused(&mut scene.host));
    save(artifacts, "gui-cache-focus-initial", &capture()?)?;
    scene
        .host
        .world_mut(scene.left.output.world().id())
        .unwrap()
        .enqueue(Batch {
            id: 101,
            operations: vec![Command::insert_value(
                EntityRef::Handle(scene.left.theme),
                ComponentValue::GuiTheme(theme(YELLOW)?),
            )],
        })?;
    let (edited, _) = scene.draw(renderer)?;
    assert_eq!(edited.surface_cache_direct, 1);
    assert_eq!(edited.surface_cache_repaints, 0);
    scene.presentations(renderer, [true, false]);
    let direct = capture()?;
    scene.image(&direct, YELLOW);
    assert_ne!(direct, initial);
    save(artifacts, "gui-cache-focus-edited", &direct)?;
    input.action(&mut scene.host, scene.left, GuiLocalAction::Blur)?;
    let (blurred, _) = scene.draw(renderer)?;
    assert!(!scene.left.focused(&mut scene.host));
    assert_eq!(
        (
            blurred.surface_cache_direct,
            blurred.surface_cache_repaints,
            blurred.surface_cache_reuses
        ),
        (0, 1, 1)
    );
    scene.presentations(renderer, [false, false]);
    let cached = capture()?;
    scene.image(&cached, YELLOW);
    assert_ne!(cached, initial);
    save(artifacts, "gui-cache-blur-current", &cached)?;
    let (warm, _) = scene.draw(renderer)?;
    assert_eq!(warm.surface_cache_reuses, 2);
    assert_eq!((warm.uploaded_bytes, warm.gui_rebuilds), (0, 0));
    assert_eq!(capture()?, cached);
    evidence.push_str(&format!("focus: {focused:?}\nfocus edit: {edited:?}\nblur current: {blurred:?}\nblur warm: {warm:?}\n"));
    // Override rows win in every state: a plain box without the default
    // look's border or glow, so hover leaves the paint unchanged.
    let mut parts = Rows::new();
    parts
        .push(GuiPaintPart {
            color: Some([YELLOW[0], YELLOW[1], YELLOW[2], 1.0]),
            border_width: Some(0.0),
            border_color: Some([0.0; 4]),
            glow_intensity: Some(0.0),
            ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background))?
        })
        .unwrap();
    scene
        .host
        .world_mut(scene.left.output.world().id())
        .unwrap()
        .enqueue(Batch {
            id: 102,
            operations: vec![Command::insert_value(
                EntityRef::Handle(scene.left.control),
                ComponentValue::GuiSkin(GuiSkin {
                    theme: scene.left.theme,
                    parts,
                }),
            )],
        })?;
    scene.draw(renderer)?;
    scene.draw(renderer)?;
    let paint = scene.left.publication(&scene.host).paint_revision;
    let unchanged = capture()?;
    let path = scene.path();
    for hovered in [true, false] {
        input.feedback(
            &mut scene.host,
            scene.left,
            &path,
            GuiInteractionUpdate::Hover(hovered),
        )?;
        let (work, _) = scene.draw(renderer)?;
        input.settled();
        assert_eq!(scene.left.interaction(&mut scene.host).hovered, hovered);
        assert_eq!(scene.left.publication(&scene.host).paint_revision, paint);
        assert_eq!(work.surface_cache_direct, u32::from(hovered));
        assert_eq!(work.surface_cache_reuses, 2 - u32::from(hovered));
        assert_eq!(
            (
                work.surface_cache_repaints,
                work.uploaded_bytes,
                work.gui_rebuilds
            ),
            (0, 0, 0)
        );
        scene.presentations(renderer, [hovered, false]);
        let pixels = capture()?;
        scene.image(&pixels, YELLOW);
        if !hovered {
            assert_eq!(pixels, unchanged);
        }
        save(
            artifacts,
            &format!("gui-cache-unchanged-hover-{hovered}"),
            &pixels,
        )?;
        evidence.push_str(&format!("unchanged hover {hovered}: {work:?}\n"));
    }
    input.feedback(
        &mut scene.host,
        scene.left,
        &path,
        GuiInteractionUpdate::Cancel,
    )?;
    scene.draw(renderer)?;
    input.drop_cancelled_pointer();
    input.close();
    scene.close(renderer)
}

fn feedback<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    artifacts: &Path,
    evidence: &mut String,
) -> Result<()> {
    let mut scene = Scene::new(renderer, FaultLocation::Canvas)?;
    for _ in 0..3 {
        scene.draw(renderer)?;
    }
    let mut input = Input::new(&scene.host, scene.root)?;
    let path = scene.path();
    let pixels = capture()?;
    scene.image(&pixels, RED);
    save(artifacts, "gui-cache-feedback-idle", &pixels)?;
    let mut previous_lease = None;
    for cycle in 0..3 {
        for (index, (update, expected, direct)) in [
            (GuiInteractionUpdate::Hover(true), GREEN, true),
            (GuiInteractionUpdate::Press, BLUE, true),
            (GuiInteractionUpdate::Capture, BLUE, true),
            (GuiInteractionUpdate::Hover(false), BLUE, true),
            (GuiInteractionUpdate::Release, YELLOW, false),
            (GuiInteractionUpdate::Hover(true), GREEN, true),
            (GuiInteractionUpdate::Cancel, RED, false),
        ]
        .into_iter()
        .enumerate()
        {
            if matches!(
                update,
                GuiInteractionUpdate::Release | GuiInteractionUpdate::Cancel
            ) {
                scene
                    .host
                    .world_mut(scene.left.output.world().id())
                    .unwrap()
                    .enqueue(Batch {
                        id: 200 + index as u64,
                        operations: vec![Command::insert_value(
                            EntityRef::Handle(scene.left.theme),
                            ComponentValue::GuiTheme(theme(expected)?),
                        )],
                    })?;
            }
            input.feedback(&mut scene.host, scene.left, &path, update)?;
            if index == 0 {
                let identity = input.pointer.as_ref().unwrap().id();
                assert_ne!(previous_lease, Some(identity));
                previous_lease = Some(identity);
            }
            let (work, _) = scene.draw(renderer)?;
            input.settled();
            assert_eq!(work.surface_cache_direct, u32::from(direct), "{work:?}");
            if !direct {
                assert_eq!(work.surface_cache_repaints, 1, "{work:?}");
            }
            let interaction = scene.left.interaction(&mut scene.host);
            assert!(!scene.left.focused(&mut scene.host));
            assert_eq!(
                [
                    interaction.hovered,
                    interaction.pressed,
                    interaction.captured
                ],
                [
                    [true, false, false],
                    [true, true, false],
                    [true, true, true],
                    [false, true, true],
                    [false; 3],
                    [true, false, false],
                    [false; 3],
                ][index]
            );
            let pixels = capture()?;
            scene.image(&pixels, expected);
            save(
                artifacts,
                &format!("gui-cache-feedback-{cycle}-{index}"),
                &pixels,
            )?;
            evidence.push_str(&format!("feedback {cycle} {update:?}: {work:?}\n"));
        }
        input.drop_cancelled_pointer();
        let (warm, _) = scene.draw(renderer)?;
        assert_eq!(warm.surface_cache_reuses, 2);
        assert_eq!((warm.uploaded_bytes, warm.gui_rebuilds), (0, 0));
    }
    input.close();
    scene.close(renderer)
}

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    mut replacement: impl FnMut() -> Result<D>,
    artifacts: &Path,
) -> Result<()> {
    let mut evidence = String::new();
    focus_return(renderer, &mut capture, artifacts, &mut evidence)?;
    feedback(renderer, &mut capture, artifacts, &mut evidence)?;
    for location in [
        FaultLocation::Canvas,
        FaultLocation::ContainingCamera,
        FaultLocation::Spatial,
        FaultLocation::NestedCamera,
    ] {
        let mut scene = Scene::new(renderer, location)?;
        for _ in 0..3 {
            scene.draw(renderer)?;
        }
        let mut input = Input::new(&scene.host, scene.root)?;
        input.action(&mut scene.host, scene.left, GuiLocalAction::Focus(0))?;
        input.action(&mut scene.host, scene.right, GuiLocalAction::Focus(0))?;
        let (focused, _) = scene.draw(renderer)?;
        input.settled();
        assert_eq!(focused.surface_cache_direct, 2);
        scene.presentations(renderer, [true, true]);
        let image = capture()?;
        scene.image(&image, RED);
        save(
            artifacts,
            &format!("gui-cache-{location:?}-focused"),
            &image,
        )?;
        renderer.replace_device(&mut scene.host, replacement()?)?;
        for _ in 0..3 {
            scene.draw(renderer)?;
        }
        let (recovered, _) = scene.draw(renderer)?;
        assert_eq!(recovered.surface_cache_direct, 2);
        scene.presentations(renderer, [true, true]);
        assert_eq!((recovered.uploaded_bytes, recovered.gui_rebuilds), (0, 0));
        let pixels = capture()?;
        scene.image(&pixels, RED);
        save(
            artifacts,
            &format!("gui-cache-{location:?}-recovered"),
            &pixels,
        )?;
        evidence.push_str(&format!("{location:?} recovered: {recovered:?}\n"));

        scene.enabled.store(true, Ordering::Relaxed);
        scene
            .host
            .world_mut(scene.fault_world)
            .unwrap()
            .enqueue(Batch {
                id: 900,
                operations: vec![Command::insert_value(
                    EntityRef::Handle(scene.oscillator),
                    ComponentValue::Scalar(Scalar {
                        value: 1.0,
                    }),
                )],
            })?;
        let (faulted, report) = scene.draw(renderer)?;
        assert_eq!(
            report.worlds[&scene.fault_world].as_ref().unwrap().outcomes[0]
                .result
                .as_ref()
                .unwrap_err()
                .reason,
            ErrorReason::NonConvergentCommit
        );
        assert!(!report.evaluation_order.contains(&scene.fault_world));
        assert_eq!(
            scene
                .host
                .world_fault(scene.host.world_ref(scene.fault_world).unwrap())?,
            Some(ErrorReason::NonConvergentCommit)
        );
        assert!(scene.left.publication(&scene.host).interaction.focused);
        let sibling_direct = u32::from(location != FaultLocation::ContainingCamera);
        assert_eq!(
            faulted.surface_cache_direct, sibling_direct,
            "{location:?}: {faulted:?}"
        );
        scene.presentations(renderer, [false, sibling_direct != 0]);
        assert_eq!(
            faulted.surface_cache_repaints + faulted.surface_cache_reuses,
            2 - sibling_direct
        );
        let pixels = capture()?;
        scene.image(&pixels, RED);
        save(
            artifacts,
            &format!("gui-cache-{location:?}-faulted"),
            &pixels,
        )?;
        evidence.push_str(&format!("{location:?} faulted: {faulted:?}\n"));
        input.close();
        scene.close(renderer)?;
    }
    std::fs::write(artifacts.join("gui-cache-interaction-work.txt"), evidence)?;
    Ok(())
}
