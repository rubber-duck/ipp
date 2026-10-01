//! Explicit System selections for direct-core fixtures.
//!
//! Every World creation names its Systems. Each part below names one capability
//! together with its required predecessors, so a fixture selects what it uses by
//! combining parts with [`select`] rather than selecting every factory.

use ipp_core::systems::{
    SystemId, animation::AnimationSystem, asset_dependencies::AssetDependencySystem,
    camera::CameraSystem, constraints::ConstraintSystem, geometry::GeometrySystem,
    hierarchy::FinalPropagationSystem, hierarchy::HierarchySystem,
    lifecycle_publisher::LifecyclePublisherSystem, look_at::LookAtSystem, render::RenderSystem,
    world_attachment::WorldAttachmentSystem,
};

/// Child World attachments.
pub const ATTACHMENTS: &[SystemId] = &[WorldAttachmentSystem::ID];

/// Lifecycle watches and observations.
pub const LIFECYCLE: &[SystemId] = &[LifecyclePublisherSystem::ID];

/// Property and structural animation.
pub const ANIMATION: &[SystemId] = &[AnimationSystem::ID];

/// Scalars and linear drivers.
pub const CONSTRAINTS: &[SystemId] = &[ConstraintSystem::ID];

/// Asset dependency tracking, which follows animation-held sources.
pub const ASSETS: &[SystemId] = &[AnimationSystem::ID, AssetDependencySystem::ID];

/// Transforms, terminal aiming and final World-space propagation.
pub const SPATIAL: &[SystemId] = &[
    HierarchySystem::ID,
    LookAtSystem::ID,
    FinalPropagationSystem::ID,
];

/// Spatial propagation with bounds and picking geometry.
pub const GEOMETRY: &[SystemId] = &[
    AnimationSystem::ID,
    AssetDependencySystem::ID,
    HierarchySystem::ID,
    LookAtSystem::ID,
    FinalPropagationSystem::ID,
    GeometrySystem::ID,
];

/// Camera outputs over evaluated geometry.
pub const CAMERA: &[SystemId] = &[
    AnimationSystem::ID,
    AssetDependencySystem::ID,
    HierarchySystem::ID,
    LookAtSystem::ID,
    FinalPropagationSystem::ID,
    GeometrySystem::ID,
    CameraSystem::ID,
];

/// Meshes, materials and lights prepared for rendering.
pub const RENDER: &[SystemId] = &[
    AnimationSystem::ID,
    AssetDependencySystem::ID,
    HierarchySystem::ID,
    LookAtSystem::ID,
    FinalPropagationSystem::ID,
    GeometrySystem::ID,
    RenderSystem::ID,
];

/// Surface anchors, which require evaluated transforms and bounds.
#[cfg(feature = "surfaces")]
pub const SURFACE: &[SystemId] = &[
    AnimationSystem::ID,
    AssetDependencySystem::ID,
    HierarchySystem::ID,
    LookAtSystem::ID,
    FinalPropagationSystem::ID,
    GeometrySystem::ID,
    ipp_core::systems::surface::SurfaceSystem::ID,
];

/// Canvas outputs with styles and boxes.
#[cfg(feature = "surfaces")]
pub const CANVAS: &[SystemId] = &[ipp_core::systems::canvas::CanvasSystem::ID];

/// Canvas outputs with asset-backed text, glyph runs, drawings and bitmaps.
#[cfg(feature = "surfaces")]
pub const CANVAS_CONTENT: &[SystemId] = &[
    AnimationSystem::ID,
    AssetDependencySystem::ID,
    ipp_core::systems::canvas::CanvasSystem::ID,
];

/// GUI controls painted on a Canvas.
#[cfg(feature = "gui")]
pub const GUI: &[SystemId] = &[
    ipp_core::systems::canvas::CanvasSystem::ID,
    ipp_core::systems::gui::GuiSystem::ID,
];

/// GUI controls with entity layout.
#[cfg(feature = "gui")]
pub const GUI_LAYOUT: &[SystemId] = &[
    ipp_core::systems::canvas::CanvasSystem::ID,
    ipp_core::systems::gui::GuiSystem::ID,
    ipp_core::systems::gui::GuiLayoutSystem::ID,
];

/// Skeleton poses and joint parenting.
#[cfg(feature = "skeletal-animation")]
pub const SKELETON: &[SystemId] = &[
    AnimationSystem::ID,
    AssetDependencySystem::ID,
    ipp_core::systems::skeleton::SkeletonSystem::ID,
];

/// Skinned deformation over skeleton poses and final transforms.
#[cfg(feature = "skeletal-animation")]
pub const SKINNING: &[SystemId] = &[
    AnimationSystem::ID,
    AssetDependencySystem::ID,
    ipp_core::systems::skeleton::SkeletonSystem::ID,
    ipp_core::systems::skinning::SkinningSystem::ID,
    HierarchySystem::ID,
    LookAtSystem::ID,
    FinalPropagationSystem::ID,
];

/// Particle producers, which require evaluated transforms and bounds.
#[cfg(feature = "particles")]
pub const PARTICLES: &[SystemId] = &[
    AnimationSystem::ID,
    AssetDependencySystem::ID,
    HierarchySystem::ID,
    LookAtSystem::ID,
    FinalPropagationSystem::ID,
    GeometrySystem::ID,
    ipp_core::systems::particles::ParticleSystem::ID,
];

/// Union of the named parts in registration order, so tie-breaking in the
/// evaluation order matches the complete registered composition.
pub fn select(parts: &[&[SystemId]]) -> Vec<SystemId> {
    let selected: Vec<SystemId> = ipp_core::systems::compiled_system_factories()
        .iter()
        .map(|factory| factory.id())
        .filter(|id| parts.iter().any(|part| part.contains(id)))
        .collect();

    assert_eq!(
        selected.len(),
        {
            let mut named: Vec<_> = parts.iter().flat_map(|part| part.iter()).collect();
            named.sort_by_key(|id| id.0);
            named.dedup();
            named.len()
        },
        "every selected System is registered"
    );
    selected
}
