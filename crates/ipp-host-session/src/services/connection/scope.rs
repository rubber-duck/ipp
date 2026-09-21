//! Session-owned declaration scopes and producer asset names within shared Worlds.

use crate::{RequestBody, WorldSession};
use ipp_core::ComponentValue;
use ipp_core::{Command, WorldContext};

pub(crate) fn scope_request(
    session: &mut WorldSession,
    world: &WorldContext<'_>,
    body: &mut RequestBody,
) -> Result<(), String> {
    let chained = matches!(body, RequestBody::BatchChunk(_));
    let mut components = if chained {
        session
            .command_batch
            .as_mut()
            .map(|batch| std::mem::take(&mut batch.components))
            .unwrap_or_default()
    } else {
        std::collections::BTreeMap::new()
    };
    if let RequestBody::Batch(batch) | RequestBody::BatchChunk(batch) = body {
        for command in &mut batch.operations {
            {
                use ipp_core::StateOverlayRef;
                let owner = match command {
                    Command::ReleaseStateOverlayOwner {
                        owner,
                    }
                    | Command::AttachEntityOverlayBinding {
                        owner,
                        ..
                    }
                    | Command::ReleaseEntityOverlayBinding {
                        owner,
                        ..
                    }
                    | Command::AttachComponentStateOverlay {
                        owner,
                        ..
                    }
                    | Command::UpdateComponentStateOverlay {
                        owner,
                        ..
                    }
                    | Command::UpdateDynamicComponentStateOverlay {
                        owner,
                        ..
                    }
                    | Command::ReleaseComponentStateOverlay {
                        owner,
                        ..
                    } => Some(*owner),
                    _ => None,
                };
                if let Some(StateOverlayRef::Handle(owner)) = owner
                    && !session.owners.contains(&owner)
                {
                    return Err("StateOverlay owner belongs to another World session".into());
                }
            }
            match command {
                Command::SetDynamicProperty {
                    value,
                    ..
                } => scope_dynamic_value(world.id(), value)?,
                Command::UpdateDynamicComponentStateOverlay {
                    properties,
                    ..
                } => {
                    for (_, value) in properties {
                        scope_dynamic_value(world.id(), value)?;
                    }
                }
                Command::InsertComponent {
                    component,
                    fields,
                    ..
                } => {
                    for field in fields {
                        scope_field(world.id(), *component, field)?;
                    }
                }
                Command::SetField {
                    component,
                    field,
                    ..
                } => scope_field(world.id(), *component, field)?,
                Command::AttachComponentStateOverlay {
                    alias,
                    component,
                    fields,
                    ..
                } => {
                    components.insert(*alias, *component);
                    for field in fields {
                        scope_field(world.id(), *component, field)?;
                    }
                }
                Command::UpdateComponentStateOverlay {
                    overlay,
                    fields,
                    ..
                } => {
                    let component = match overlay {
                        ipp_core::StateOverlayRef::Handle(handle) => {
                            world.state_overlay_component(*handle)
                        }
                        ipp_core::StateOverlayRef::Alias(alias) => components.get(alias).copied(),
                    };
                    if let Some(component) = component {
                        for field in fields {
                            scope_field(world.id(), component, field)?;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if chained && let Some(batch) = &mut session.command_batch {
        batch.components = components;
    }
    if let RequestBody::AnimationController(command) = body {
        scope_animation_command(world.id(), command)?;
    }
    #[cfg(feature = "surfaces")]
    if let RequestBody::SurfaceCommand(command) = body {
        scope_surface_command(world.id(), command)?;
    }
    #[cfg(feature = "gui")]
    if let RequestBody::GuiCommands {
        commands,
        ..
    } = body
    {
        for command in commands {
            scope_gui_command(world.id(), command)?;
        }
    }
    Ok(())
}

fn scope_animation_command(
    world: ipp_core::WorldId,
    command: &mut ipp_core::systems::animation::AnimationControllerCommand,
) -> Result<(), String> {
    let description = match command {
        ipp_core::systems::animation::AnimationControllerCommand::Create(description)
        | ipp_core::systems::animation::AnimationControllerCommand::Update {
            description,
            ..
        } => description,
        ipp_core::systems::animation::AnimationControllerCommand::Transition {
            transition,
            ..
        } => &mut transition.description,
        _ => return Ok(()),
    };
    for driver in &mut description.drivers {
        scope_source(world, &driver.source)?;
    }
    Ok(())
}

#[cfg(feature = "surfaces")]
fn scope_surface_command(
    world: ipp_core::WorldId,
    command: &ipp_core::systems::surface::SurfaceCommand,
) -> Result<(), String> {
    use ipp_core::systems::surface::SurfaceCommand;

    let asset = match command {
        SurfaceCommand::Insert {
            style,
            ..
        } => style.asset.as_ref(),
        SurfaceCommand::Update {
            patch,
            ..
        } => patch.asset.as_ref().and_then(Option::as_ref),
        SurfaceCommand::Remove {
            ..
        }
        | SurfaceCommand::Move {
            ..
        } => None,
    };
    if let Some(asset) = asset {
        scope_source(world, &asset.uri)?;
    }
    Ok(())
}

#[cfg(feature = "gui")]
fn scope_gui_command(
    world: ipp_core::WorldId,
    command: &ipp_core::systems::gui::GuiCommand,
) -> Result<(), String> {
    use ipp_core::systems::gui::GuiCommand;

    let asset = match command {
        GuiCommand::InsertNode {
            style,
            ..
        } => style.asset.as_ref(),
        GuiCommand::UpdateNode {
            patch,
            ..
        } => patch.asset.as_ref().and_then(Option::as_ref),
        GuiCommand::MoveNode {
            ..
        }
        | GuiCommand::RemoveNode {
            ..
        }
        | GuiCommand::SetControlValue {
            ..
        } => None,
    };
    if let Some(asset) = asset {
        scope_source(world, &asset.uri)?;
    }
    Ok(())
}

fn scope_dynamic_value(
    world: ipp_core::WorldId,
    value: &mut ipp_core::DynamicValue,
) -> Result<(), String> {
    if let ipp_core::DynamicValue::Asset(asset) = value {
        scope_source(world, &asset.uri)?;
    }
    Ok(())
}

fn scope_field(
    world: ipp_core::WorldId,
    component: u16,
    field: &mut ipp_core::FieldWrite,
) -> Result<(), String> {
    if let ipp_core::FieldValue::String(source) = &field.value
        && source.starts_with("producer://")
        && !source.starts_with(&format!("producer://{}/", world.0))
    {
        return Err("Producer asset belongs to another World".into());
    }
    if let ipp_core::FieldValue::String(source) = &mut field.value
        && ComponentValue::asset_references(component)
            .iter()
            .any(|reference| reference.source_offset == field.offset)
    {
        scope_source(world, source)?;
    }
    Ok(())
}

fn scope_source(world: ipp_core::WorldId, source: &str) -> Result<(), String> {
    if source.starts_with("producer://") && !source.starts_with(&format!("producer://{}/", world.0))
    {
        return Err("Producer asset belongs to another World".into());
    }
    if source.starts_with("asset://") {
        return Err(
            "Numeric asset references are unavailable; use an immutable provider source".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_source_references_do_not_require_producer_authority() {
        assert!(scope_source(ipp_core::WorldId(1), "client://7/a#v").is_ok());
        assert!(scope_source(ipp_core::WorldId(1), "client://8/a#v").is_ok());
    }

    #[test]
    fn transition_destination_sources_are_scoped_to_the_world() {
        use ipp_core::systems::animation::*;

        let transition = |source: &str| AnimationControllerCommand::Transition {
            id: AnimationControllerId::from_bits(1),
            transition: AnimationControllerTransition {
                description: AnimationControllerDescription {
                    drivers: vec![AnimationDriverDescription {
                        source: source.into(),
                        variant: 0,
                        track: 0,
                        target: ipp_core::EntityId::from_bits(1),
                        property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                            component: 1,
                            offsets: vec![0],
                        }),
                        weight: 1.0,
                        additive: false,
                        reference_time: 0.0,
                        repeat: false,
                    }],
                    ..Default::default()
                },
                duration: 0.5,
                easing: AnimationTransitionEasing::Linear,
                start_time: AnimationTransitionStartTime::Restart,
            },
        };
        let mut valid = transition("producer://7/clip#v1");
        assert!(scope_animation_command(ipp_core::WorldId(7), &mut valid).is_ok());
        let mut foreign = transition("producer://8/clip#v1");
        assert!(scope_animation_command(ipp_core::WorldId(7), &mut foreign).is_err());
    }

    #[cfg(feature = "surfaces")]
    #[test]
    fn surface_asset_sources_are_scoped_to_the_world() {
        use ipp_core::services::asset_management::AssetSource;
        use ipp_core::systems::surface::*;

        let insert = |source: &str| SurfaceCommand::Insert {
            entity: ipp_core::EntityId::from_bits(1),
            id: SurfaceItemId(1),
            index: 0,
            content: SurfaceItemContent::Bitmap {
                size: [1.0; 2],
            },
            style: SurfaceItemStyle {
                asset: Some(AssetSource {
                    uri: source.into(),
                    variant: 0,
                    kind: ipp_core::TEXTURE_TYPE,
                }),
                ..Default::default()
            },
        };
        assert!(scope_surface_command(ipp_core::WorldId(7), &insert("producer://7/image")).is_ok());
        assert!(
            scope_surface_command(ipp_core::WorldId(7), &insert("producer://8/image")).is_err()
        );
        assert!(scope_surface_command(ipp_core::WorldId(7), &insert("asset://4")).is_err());

        let update = SurfaceCommand::Update {
            entity: ipp_core::EntityId::from_bits(1),
            id: SurfaceItemId(1),
            patch: SurfaceItemPatch {
                asset: Some(Some(AssetSource {
                    uri: "producer://8/image".into(),
                    variant: 0,
                    kind: ipp_core::TEXTURE_TYPE,
                })),
                ..Default::default()
            },
        };
        assert!(scope_surface_command(ipp_core::WorldId(7), &update).is_err());

        let clear = SurfaceCommand::Update {
            entity: ipp_core::EntityId::from_bits(1),
            id: SurfaceItemId(1),
            patch: SurfaceItemPatch {
                asset: Some(None),
                ..Default::default()
            },
        };
        assert!(scope_surface_command(ipp_core::WorldId(7), &clear).is_ok());
    }

    #[cfg(feature = "gui")]
    #[test]
    fn gui_asset_sources_are_scoped_to_the_world() {
        use ipp_core::services::asset_management::AssetSource;
        use ipp_core::systems::gui::*;

        let handle = GuiNodeHandle::new(1, ipp_core::EntityId::from_bits(1), 1, GuiNodeId(1), 1);
        let insert = |uri: &str| GuiCommand::InsertNode {
            entity: ipp_core::EntityId::from_bits(1),
            root_incarnation: 1,
            id: GuiNodeId(1),
            parent: None,
            index: 0,
            content: GuiNodeContent::Drawing,
            style: GuiNodeStyle {
                asset: Some(AssetSource {
                    uri: uri.into(),
                    variant: 0,
                    kind: ipp_core::TEXTURE_TYPE,
                }),
                ..Default::default()
            },
        };

        assert!(scope_gui_command(ipp_core::WorldId(7), &insert("producer://7/image")).is_ok());
        assert!(scope_gui_command(ipp_core::WorldId(7), &insert("producer://8/image")).is_err());
        assert!(scope_gui_command(ipp_core::WorldId(7), &insert("asset://4")).is_err());

        let update = GuiCommand::UpdateNode {
            handle,
            patch: GuiNodePatch {
                asset: Some(Some(AssetSource {
                    uri: "producer://8/image".into(),
                    variant: 0,
                    kind: ipp_core::TEXTURE_TYPE,
                })),
                ..Default::default()
            },
        };
        assert!(scope_gui_command(ipp_core::WorldId(7), &update).is_err());

        let clear = GuiCommand::UpdateNode {
            handle,
            patch: GuiNodePatch {
                asset: Some(None),
                ..Default::default()
            },
        };
        assert!(scope_gui_command(ipp_core::WorldId(7), &clear).is_ok());
    }
}
