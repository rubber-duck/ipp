//! GUI presentation bookkeeping for [`RenderSystem`](super::RenderSystem).
//!
//! RenderSystem owns this state and drives it at its existing schedule
//! points with unchanged timing. This file holds the retained revision
//! tracking, skin resource/presentation/override maps, pending skin
//! animation commands, skin paint resolution and skin transition planning
//! so `system.rs` keeps the schedule and the scene passes.

use super::RenderSystem;

#[cfg(feature = "gui")]
#[derive(Clone, Debug)]
pub(super) struct GuiSkinPresentation {
    desired: crate::systems::gui::GuiSkinnedAppearance,
    motion: Option<crate::systems::gui::GuiPartMotion>,
    pending: Option<GuiSkinPendingTransition>,
    acknowledged: Option<u64>,
    refused: Option<(u64, crate::ErrorReason)>,
    last_painted: crate::systems::gui::GuiSkinnedAppearance,
}

/// Every input skin reconciliation reads, compared before each pass so an
/// unchanged frame skips it. Roots carry their layout content revision
/// (evaluated records, paint lanes and control values, not translated
/// geometry) and committed-change count (authored lanes that animation may
/// hide); cursors select interaction states; controller states carry
/// acknowledgements, rejections, failures and crossfade completion.
#[cfg(feature = "gui")]
#[derive(Clone, Debug, PartialEq)]
pub(super) struct GuiSkinReconcileKey {
    roots: Vec<(crate::EntityId, u64, u64)>,
    cursors: crate::systems::gui::GuiSkinCursors,
    controllers: Vec<crate::systems::animation::GuiSkinControllerState>,
}

#[cfg(feature = "gui")]
#[derive(Clone, Debug)]
pub(super) struct GuiSkinPendingTransition {
    request: u64,
    source: crate::systems::gui::GuiPartMotion,
    source_sample: crate::systems::animation::GuiSkinAnimationSample,
}

#[cfg(feature = "gui")]
impl RenderSystem {
    /// Publish the full-fenced last-ready selections as this System's derived
    /// consumer lane. The shared service preserves aggregate World demand when
    /// authored selection moves to a pending replacement.
    pub(super) fn update_gui_skin_resource_demand(
        &self,
        world: crate::WorldId,
        assets: &mut crate::services::asset_management::AssetManagementService,
    ) {
        let demand = self
            .gui_skin_resources
            .values()
            .map(|resource| {
                crate::services::asset_management::service::AssetDemandSelection::new(
                    resource.source.kind,
                    &resource.source.uri,
                    resource.source.variant,
                )
            })
            .collect();
        assets.update_system_users(world, Self::ID.0, demand);
    }
}

// ---------------------------------------------------------------------------
// Skin paint and transition-plan ownership.
// ---------------------------------------------------------------------------

/// Runtime resource catalog behind skin asset lanes in prepared paint.
///
/// Mirrors the layout pass resolver without measuring text: glyph fonts
/// always stay on their measured resources and only drawing and bitmap
/// lanes resolve here.
#[cfg(all(feature = "surfaces", feature = "gui"))]
struct GuiSkinRenderResolver<'a> {
    /// Identity scoping immutable source lookups.
    world: crate::WorldId,
    /// Shared Host resource catalog.
    assets: &'a crate::services::asset_management::AssetManagementService,
}

#[cfg(all(feature = "surfaces", feature = "gui"))]
impl crate::systems::gui::GuiResourceResolver for GuiSkinRenderResolver<'_> {
    fn text_font(
        &self,
        _source: &crate::services::asset_management::AssetSource,
    ) -> crate::systems::gui::GuiFontResolution<'_> {
        crate::systems::gui::GuiFontResolution::Missing
    }

    fn surface_resource(
        &self,
        source: &crate::services::asset_management::AssetSource,
    ) -> Option<crate::systems::surface::SurfaceRenderResource> {
        let key = self
            .assets
            .find_source(self.world, source.kind, &source.uri, source.variant)?;
        self.assets.get(key)?.data()?;
        Some(crate::systems::surface::SurfaceRenderResource {
            key,
            source: source.clone(),
        })
    }

    fn drawing_view_box(
        &self,
        source: &crate::services::asset_management::AssetSource,
    ) -> Option<[f32; 4]> {
        let key = self
            .assets
            .find_source(self.world, source.kind, &source.uri, source.variant)?;
        self.assets
            .get(key)?
            .data()?
            .decoded()
            .downcast_ref::<crate::services::asset_management::drawing::DrawingAsset>()
            .map(|drawing| drawing.view_box())
    }
}

/// Resolve skin styling for one retained view before scroll translation.
///
/// Reads the effective root, the consumed input cursors and the runtime
/// resource catalog. Unavailable views, missing roots and idle cursors
/// keep base paint; skins never move geometry or reorder primitives.
#[cfg(all(feature = "surfaces", feature = "gui"))]
#[allow(clippy::too_many_arguments)] // Boundary data stays explicitly borrowed from its owners.
pub(super) fn skin_paint_for_view(
    world_id: crate::WorldId,
    world: &crate::world::WorldSimulationState,
    assets: &crate::services::asset_management::AssetManagementService,
    cursors: &crate::systems::gui::GuiSkinCursors,
    view: &crate::systems::gui::GuiEvaluatedView,
    entity: crate::EntityId,
    retained_resources: &mut std::collections::BTreeMap<
        (crate::EntityId, crate::systems::surface::GuiPrimitiveId),
        crate::systems::surface::SurfaceRenderResource,
    >,
    overrides: &std::collections::BTreeMap<
        (crate::EntityId, crate::systems::surface::GuiPrimitiveId),
        crate::systems::gui::GuiSkinnedAppearance,
    >,
) -> Vec<crate::systems::surface::SurfaceRenderPrimitive> {
    let base = view.surface_primitives();
    let index = entity.index() as usize;
    let Some(root) = world.components.gui_root(index) else {
        return base;
    };
    let resolver = GuiSkinRenderResolver {
        world: world_id,
        assets,
    };
    let view_overrides = overrides
        .iter()
        .filter_map(|(&(owner, id), appearance)| {
            (owner == entity).then_some((id, appearance.clone()))
        })
        .collect();
    crate::systems::gui::skinned_primitives_for_view_with_overrides(
        view,
        root,
        cursors,
        &resolver,
        retained_resources,
        &view_overrides,
    )
}

/// Reconcile authored skin destinations with ordinary AnimationSystem controllers.
#[cfg(all(feature = "surfaces", feature = "gui"))]
impl RenderSystem {
    pub(super) fn update_gui_skin_presentations(
        &mut self,
        world: &crate::world::WorldSimulationState,
        _assets: &crate::services::asset_management::AssetManagementService,
        animation: &crate::systems::animation::AnimationSystem,
        layout: &crate::systems::gui::GuiLayoutSystem,
    ) -> bool {
        use crate::systems::animation::{
            AnimationControllerTransition, AnimationInternalCommand, AnimationTransitionStartTime,
            GuiSkinAnimationOwner,
        };

        // Unchanged inputs reproduce the retained presentations. A pending
        // transition reissues its request every frame until acknowledged.
        let key = GuiSkinReconcileKey {
            roots: layout.content_states(),
            cursors: self.gui_skin_cursors.clone(),
            controllers: animation.skin_controller_states(),
        };
        let pending = self
            .gui_skin_presentations
            .values()
            .any(|presentation| presentation.pending.is_some());
        if !pending && self.gui_skin_key.as_ref() == Some(&key) {
            return false;
        }
        self.gui_skin_key = Some(key);

        #[cfg(test)]
        super::system::PREPARATION_COUNTS.with(|counts| {
            let (passes, reconciliations) = counts.get();
            counts.set((passes, reconciliations + 1));
        });

        let mut live = std::collections::BTreeSet::new();
        let mut overrides = std::collections::BTreeMap::new();
        let mut next_request = self.next_skin_animation_request;

        for (entity, _) in layout.paint_states() {
            let Some(view) = layout.view(entity) else {
                continue;
            };
            let Some(effective_root) = world.components.gui_root(entity.index() as usize) else {
                continue;
            };
            // Borrow the effective root unless animation holds authored
            // values underneath it.
            let restored;
            let authored_root = if animation.state.has_underlying(
                entity,
                view.root_incarnation,
                crate::ComponentValue::GUI_ROOT,
            ) {
                let mut underlying = crate::ComponentValue::GuiRoot(effective_root.clone());
                animation
                    .state
                    .restore_underlying(entity, view.root_incarnation, &mut underlying);
                let crate::ComponentValue::GuiRoot(root) = underlying else {
                    unreachable!("restored GUI root remains a GUI root")
                };
                restored = root;
                &restored
            } else {
                effective_root
            };

            let parts = crate::systems::gui::skinned_parts_for_view(
                view,
                authored_root,
                &self.gui_skin_cursors,
            );
            for part in parts {
                let id = part.id;
                let node = part.node;
                let interaction = self.gui_skin_cursors.interaction_for(
                    crate::systems::gui::GuiInputTarget {
                        entity,
                        root_incarnation: id.root_incarnation,
                        node: id.node,
                        lifetime: id.lifetime,
                    },
                    node.enabled,
                );
                let Some(desired) = crate::systems::gui::resolve_paint_appearance(
                    authored_root,
                    node,
                    &interaction,
                    id.part,
                ) else {
                    continue;
                };
                let motion = crate::systems::gui::resolve_state_part_motion(
                    authored_root,
                    id.node,
                    id.part.as_str(),
                    desired.state,
                    desired.variant,
                );
                let key = (entity, id);
                live.insert(key);

                let Some(presentation) = self.gui_skin_presentations.get_mut(&key) else {
                    overrides.insert(key, desired.clone());
                    self.gui_skin_presentations.insert(
                        key,
                        GuiSkinPresentation {
                            desired: desired.clone(),
                            motion,
                            pending: None,
                            acknowledged: None,
                            refused: None,
                            last_painted: desired,
                        },
                    );
                    continue;
                };

                let owner = GuiSkinAnimationOwner {
                    entity,
                    primitive: id,
                };
                let owned = animation.skin_controller(owner);
                let rejection = animation.skin_controller_rejection(owner);
                let had_derived_state = presentation.pending.is_some()
                    || presentation.acknowledged.is_some()
                    || presentation.refused.is_some()
                    || owned.is_some()
                    || rejection.is_some();
                let mut terminal_failure = rejection;
                if let Some((request, controller, failure)) = owned {
                    if let Some(reason) = failure {
                        terminal_failure = Some((request, reason));
                    } else {
                        let mut sampled = crate::systems::gui::appearance_with_effective_numeric(
                            presentation.desired.clone(),
                            effective_root,
                            id.node,
                            id.part,
                        );
                        if controller.transition.is_some() {
                            sampled.asset = presentation.last_painted.asset.clone();
                        }
                        presentation.last_painted = sampled;
                        presentation.acknowledged = Some(request);
                        presentation.refused = None;
                    }
                } else if rejection.is_none() && presentation.acknowledged.take().is_some() {
                    // A private association can disappear during restoration or
                    // lifecycle repair. Never retain its last sampled override:
                    // return to the current authored destination and clear any
                    // stale owner bookkeeping before accepting a later intent.
                    presentation.last_painted = desired.clone();
                    self.pending_skin_animation_commands
                        .push(AnimationInternalCommand::DeleteSkin(owner));
                }
                if let Some(failure) = terminal_failure {
                    if presentation.refused != Some(failure) {
                        crate::diagnostic!(
                            Warn,
                            "[IPP core] gui_skin_animation.reject entity={entity:?} request={} reason={}",
                            failure.0,
                            failure.1
                        );
                    }
                    if presentation
                        .pending
                        .as_ref()
                        .is_some_and(|pending| pending.request == failure.0)
                    {
                        presentation.pending = None;
                    }
                    presentation.acknowledged = None;
                    presentation.refused = Some(failure);
                    presentation.last_painted = desired.clone();
                    self.pending_skin_animation_commands
                        .push(AnimationInternalCommand::DeleteSkin(owner));
                }

                let appearance_changed = presentation.desired != desired;
                let motion_changed = presentation.motion != motion;
                if presentation.refused.is_some() && (appearance_changed || motion_changed) {
                    presentation.pending = None;
                    presentation.acknowledged = None;
                    presentation.last_painted = desired.clone();
                    if let Some(to) = motion.as_ref()
                        && skin_numeric_base_complete(authored_root, id.node, id.part)
                        && let Some(sample) = skin_animation_sample(&desired)
                        && next_request != 0
                    {
                        presentation.pending = Some(GuiSkinPendingTransition {
                            request: next_request,
                            source: to.clone(),
                            source_sample: sample,
                        });
                        next_request = next_request.checked_add(1).unwrap_or(0);
                    } else {
                        self.pending_skin_animation_commands
                            .push(AnimationInternalCommand::DeleteSkin(owner));
                    }
                    presentation.refused = None;
                } else if appearance_changed {
                    if let Some((from, to)) = presentation.motion.as_ref().zip(motion.as_ref())
                        && skin_numeric_base_complete(authored_root, id.node, id.part)
                        && let Some(source_sample) = skin_animation_sample(&presentation.desired)
                        && skin_animation_sample(&desired).is_some()
                    {
                        if next_request != 0 {
                            presentation.pending = Some(GuiSkinPendingTransition {
                                request: next_request,
                                source: from.clone(),
                                source_sample,
                            });
                            next_request = next_request.checked_add(1).unwrap_or(0);
                        }
                        let _ = to;
                    } else {
                        presentation.pending = None;
                        presentation.acknowledged = None;
                        if had_derived_state {
                            self.pending_skin_animation_commands
                                .push(AnimationInternalCommand::DeleteSkin(owner));
                        }
                        presentation.last_painted = desired.clone();
                    }
                } else if motion_changed && (presentation.pending.is_some() || owned.is_some()) {
                    if let Some(source) = presentation.motion.as_ref()
                        && motion.is_some()
                        && let Some(source_sample) = skin_animation_sample(&presentation.desired)
                        && next_request != 0
                    {
                        presentation.pending = Some(GuiSkinPendingTransition {
                            request: next_request,
                            source: source.clone(),
                            source_sample,
                        });
                        next_request = next_request.checked_add(1).unwrap_or(0);
                    } else if motion.is_none() {
                        presentation.pending = None;
                        presentation.acknowledged = None;
                        self.pending_skin_animation_commands
                            .push(AnimationInternalCommand::DeleteSkin(owner));
                        presentation.last_painted = desired.clone();
                    }
                }
                presentation.desired = desired;
                presentation.motion = motion;

                if let Some(pending) = presentation.pending.clone() {
                    if owned.is_some_and(|(request, _, failure)| {
                        request == pending.request && failure.is_none()
                    }) {
                        presentation.pending = None;
                    } else if let Some(to) = presentation.motion.as_ref()
                        && let Some(destination_sample) =
                            skin_animation_sample(&presentation.desired)
                    {
                        self.pending_skin_animation_commands.push(
                            AnimationInternalCommand::EnsureSkinTransition {
                                owner,
                                request: pending.request,
                                source: skin_controller_description(entity, id, &pending.source),
                                source_time: pending.source.sample_time,
                                source_sample: pending.source_sample,
                                transition: AnimationControllerTransition {
                                    description: skin_controller_description(entity, id, to),
                                    duration: to.duration_secs,
                                    easing: to.easing,
                                    start_time: AnimationTransitionStartTime::Seek(to.sample_time),
                                },
                                destination_sample,
                            },
                        );
                    } else {
                        presentation.pending = None;
                        presentation.last_painted = presentation.desired.clone();
                        self.pending_skin_animation_commands
                            .push(AnimationInternalCommand::DeleteSkin(owner));
                    }
                }
                overrides.insert(key, presentation.last_painted.clone());
            }
        }

        let departed: Vec<_> = self
            .gui_skin_presentations
            .keys()
            .filter(|key| !live.contains(key))
            .copied()
            .collect();
        for key in departed {
            let owner = GuiSkinAnimationOwner {
                entity: key.0,
                primitive: key.1,
            };
            if self
                .gui_skin_presentations
                .remove(&key)
                .is_some_and(|presentation| {
                    presentation.pending.is_some()
                        || presentation.acknowledged.is_some()
                        || presentation.refused.is_some()
                        || animation.skin_controller(owner).is_some()
                        || animation.skin_controller_rejection(owner).is_some()
                })
            {
                self.pending_skin_animation_commands
                    .push(AnimationInternalCommand::DeleteSkin(owner));
            }
        }

        self.next_skin_animation_request = next_request;
        let changed = self.gui_skin_overrides != overrides;
        self.gui_skin_overrides = overrides;
        changed
    }
}

#[cfg(all(feature = "surfaces", feature = "gui"))]
pub(super) fn skin_numeric_base_complete(
    root: &crate::systems::gui::GuiRoot,
    node: crate::systems::gui::GuiNodeId,
    part: crate::systems::surface::GuiPrimitivePart,
) -> bool {
    let style = crate::systems::gui::part_style(root, node, part.as_str());
    style.color.is_some() && style.opacity.is_some() && style.scale.is_some()
}

#[cfg(all(feature = "surfaces", feature = "gui"))]
pub(super) fn skin_animation_sample(
    appearance: &crate::systems::gui::GuiSkinnedAppearance,
) -> Option<crate::systems::animation::GuiSkinAnimationSample> {
    Some(crate::systems::animation::GuiSkinAnimationSample {
        color: appearance.color?,
        opacity: appearance.opacity?,
        scale: appearance.scale?,
    })
}

#[cfg(all(feature = "surfaces", feature = "gui"))]
pub(super) fn skin_controller_description(
    entity: crate::EntityId,
    primitive: crate::systems::surface::GuiPrimitiveId,
    motion: &crate::systems::gui::GuiPartMotion,
) -> crate::systems::animation::AnimationControllerDescription {
    use crate::systems::animation::{
        AnimationControllerDescription, AnimationDriverDescription, AnimationTrackTarget,
    };

    let tracks = [
        motion.base_track,
        motion
            .base_track
            .checked_add(1)
            .expect("resolved GUI skin motion has three tracks"),
        motion
            .base_track
            .checked_add(2)
            .expect("resolved GUI skin motion has three tracks"),
    ];
    let drivers = ["color", "opacity", "scale"]
        .into_iter()
        .zip(tracks)
        .map(|(lane, track)| AnimationDriverDescription {
            source: motion.source.uri.clone(),
            variant: motion.source.variant,
            track,
            target: entity,
            property: AnimationTrackTarget::DynamicProperty {
                component: crate::ComponentValue::GUI_ROOT,
                name: crate::systems::gui::GuiRoot::part_property_name(
                    primitive.node,
                    primitive.part.as_str(),
                    lane,
                )
                .expect("built-in GUI skin lane"),
            },
            weight: 1.0,
            additive: false,
            reference_time: 0.0,
            repeat: false,
        })
        .collect();
    AnimationControllerDescription {
        drivers,
        speed: 0.0,
        looping: false,
    }
}
