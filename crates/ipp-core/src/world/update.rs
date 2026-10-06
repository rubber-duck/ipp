//! The fixed frame loop: preflight, ordered ingress admission and evaluation
//! of every selected System phase, published as one [`WorldUpdateReport`].

use crate::world::WorldContext;
use crate::world::context::SystemInstanceAccess;
use crate::world::ingress::Ingress;
use crate::world::systems::{self, SystemRuntimeAccess};
use crate::{BatchOutcome, ErrorReason};

/// Stage 8 publication after mutation and evaluation complete.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldUpdateReport {
    /// Ordered playback transitions committed during this frame.
    pub playback_events: Vec<crate::systems::animation::AnimationPlaybackEvent>,
    /// Correlated outcomes for ordered controller mutations.
    pub animation_controller_outcomes: Vec<crate::systems::animation::AnimationControllerOutcome>,
    /// Monotonic completed-frame number.
    pub tick: u64,
    /// Accumulated host-supplied simulation time.
    pub time: f64,
    /// Correlated ordered system-command results.
    pub system_command_outcomes: Vec<systems::SystemCommandOutcome>,
    /// One outcome per queued batch, in submission order.
    pub outcomes: Vec<BatchOutcome>,
    /// Changed camera-system settings in committed transition order.
    pub camera_state_changes: Vec<crate::CameraStateChange>,
    /// Changed global render settings in committed transition order.
    pub render_state_changes: Vec<crate::RenderStateChange>,
    /// Read-only geometry results observing the final camera and effective frame.
    pub geometry_picks: Vec<crate::GeometryPickOutcome>,
    /// Stateless plane projections observing the final effective camera.
    pub camera_projections: Vec<crate::CameraProjectOutcome>,
    /// Mesh completions processed before ordinary batches, in enqueue order.
    pub assets: Vec<crate::services::asset_management::AssetUploadOutcome>,
    /// Terminal provider outcomes committed this frame, in completion order.
    pub resource_changes: Vec<crate::AssetResourceSnapshot>,
}

impl WorldContext<'_> {
    /// Preflight/accept subsystem ingress before shared service progression.
    /// This is not the data visibility cut: producers and Host time may still progress.
    /// Data bindings consume final notifications in `System::prepare_evaluation`.
    pub fn prepare_update(&mut self, dt: f64) -> Result<(), ErrorReason> {
        #[cfg(feature = "instrumentation")]
        let _context = crate::profiling::ContextScope::world(self.world.profile_context);

        if self.world.updating || !dt.is_finite() || dt < 0.0 || !(self.world.time + dt).is_finite()
        {
            return Err(ErrorReason::InvalidValue);
        }
        self.world
            .tick
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        if self.world.prepared_frame {
            return Ok(());
        }
        let mut report = WorldUpdateReport::default();
        self.dispatch_phase(dt, SystemFramePhase::Check, &mut report)?;
        self.dispatch_phase(dt, SystemFramePhase::Accept, &mut report)?;
        self.world.prepared_frame = true;
        Ok(())
    }

    /// Commit ordered ingress, evaluate the fixed schedule, then detach subsystem observations.
    pub fn step(&mut self, dt: f64) -> Result<WorldUpdateReport, ErrorReason> {
        let report = self.admit_frame(dt)?;
        self.evaluate_frame(dt, report)
    }

    pub(crate) fn admit_frame(&mut self, dt: f64) -> Result<WorldUpdateReport, ErrorReason> {
        #[cfg(feature = "instrumentation")]
        let _context = crate::profiling::ContextScope::world(self.world.profile_context);

        self.prepare_update(dt)?;
        self.owns_update = true;
        self.world.updating = true;
        let mut report = WorldUpdateReport {
            tick: self.world.tick + 1,
            time: self.world.time + dt,
            ..Default::default()
        };
        while let Some(front) = self.world.queue.front() {
            if let Ingress::System {
                system,
                command,
                ..
            } = front
                && self
                    .instances
                    .before
                    .iter()
                    .find(|instance| instance.id == *system)
                    .is_some_and(|instance| !instance.system.command_ready(command.as_ref()))
            {
                break;
            }
            let ingress = self.world.queue.pop_front().expect("observed ingress head");
            match ingress {
                Ingress::Batch {
                    batch,
                    mut effect_sink,
                } => {
                    let sink = effect_sink
                        .as_mut()
                        .map(|sink| &mut **sink as &mut dyn crate::OperationEffectSink);
                    report
                        .outcomes
                        .push(self.apply_batch(batch, report.tick, sink));
                }
                Ingress::System {
                    system,
                    session,
                    request_id,
                    command,
                } => {
                    let result = self.apply_system_command(system, session, command.as_ref());
                    if request_id != 0 {
                        report
                            .system_command_outcomes
                            .push(systems::SystemCommandOutcome {
                                session,
                                request_id,
                                applied: usize::from(result.is_ok()),
                                result,
                            });
                    }
                }
                Ingress::SystemBatch {
                    system,
                    session,
                    request_id,
                    commands,
                } => {
                    let mut applied = 0;
                    let mut result = Ok(());
                    for command in commands {
                        result = self.apply_system_command(system, session, command.as_ref());
                        if result.is_err() {
                            break;
                        }
                        applied += 1;
                    }
                    if request_id != 0 {
                        report
                            .system_command_outcomes
                            .push(systems::SystemCommandOutcome {
                                session,
                                request_id,
                                applied,
                                result,
                            });
                    }
                }
            }
        }
        self.world.updating = false;
        self.owns_update = false;
        Ok(report)
    }

    pub(crate) fn evaluate_frame(
        &mut self,
        dt: f64,
        mut report: WorldUpdateReport,
    ) -> Result<WorldUpdateReport, ErrorReason> {
        #[cfg(feature = "instrumentation")]
        let _context = crate::profiling::ContextScope::world(self.world.profile_context);
        #[cfg(feature = "instrumentation")]
        let _trace = crate::profiling::WorldTraceScope::new();

        self.owns_update = true;
        self.world.updating = true;
        self.data.begin_evaluation();
        if self.world.fault.is_none() {
            self.dispatch_phase(dt, SystemFramePhase::Prepare, &mut report)?;
            self.world.accepting_removals = true;
            self.dispatch_phase(dt, SystemFramePhase::Evaluate, &mut report)?;
            self.world.accepting_removals = false;
            self.drain_deferred_removals();
        } else {
            report.time = self.world.time;
        }
        self.world.tick = report.tick;
        self.world.time = report.time;
        if self.world.fault.is_none() {
            self.dispatch_phase(dt, SystemFramePhase::Finish, &mut report)?;
            self.dispatch_phase(dt, SystemFramePhase::Observe, &mut report)?;
        }
        self.data.end_evaluation();
        self.world.prepared_frame = false;
        self.world.updating = false;
        self.owns_update = false;
        Ok(report)
    }

    fn dispatch_phase(
        &mut self,
        dt: f64,
        phase: SystemFramePhase,
        report: &mut WorldUpdateReport,
    ) -> Result<(), ErrorReason> {
        for index in 0..self.instances.before.len() {
            let (systems_before_current, tail) = self.instances.before.split_at_mut(index);
            let (current_system, systems_after_current) =
                tail.split_first_mut().expect("schedule index");
            let mut context = systems::SystemUpdateContext {
                world: SystemRuntimeAccess {
                    world: self.world,
                    instances: SystemInstanceAccess {
                        before: systems_before_current,
                        current: Some(current_system.id),
                        after: systems_after_current,
                    },
                    asset_acquisition: self.asset_acquisition,
                    io: self.io,
                    data: self.data,
                    topology: self.topology,
                    frame_context: self.frame_context,
                    reference_worlds: self.reference_worlds.as_ref(),
                },
                dt: &dt,
                dependent: index,
            };
            #[cfg(feature = "instrumentation")]
            let _measurement = crate::profiling::Stage::system(
                current_system.profile_slot,
                phase as usize,
                current_system.id.0,
            );
            match phase {
                SystemFramePhase::Check => current_system.system.prepare_frame(&mut context)?,
                SystemFramePhase::Accept => current_system.system.accept_ingress(&mut context),
                SystemFramePhase::Prepare => current_system.system.prepare_evaluation(&mut context),
                SystemFramePhase::Evaluate => current_system.system.update(&mut context),
                SystemFramePhase::Finish => {
                    current_system.system.finish_update(&mut context, report)
                }
                SystemFramePhase::Observe => current_system
                    .system
                    .observe_frame(context.world.view(), report.tick),
            }
        }
        Ok(())
    }
}

/// Frame phases in dispatch order; profiling numbers them in this order.
#[derive(Clone, Copy)]
enum SystemFramePhase {
    Check,
    Accept,
    Prepare,
    Evaluate,
    Finish,
    /// Read-only comparison of final stored values, after every System's Finish.
    Observe,
}
