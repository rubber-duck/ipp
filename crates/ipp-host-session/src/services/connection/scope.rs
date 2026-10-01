//! Producer asset names within shared Worlds.

use crate::RequestBody;
use ipp_core::ComponentValue;
use ipp_core::{Command, WorldContext};

pub(crate) fn scope_request(
    world: &WorldContext<'_>,
    body: &mut RequestBody,
) -> Result<(), String> {
    if let RequestBody::SubmitBatch(batch) = body {
        for command in &mut batch.operations {
            match command {
                Command::SetDynamicProperty {
                    value,
                    ..
                } => scope_dynamic_value(world.id(), value)?,
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
                }
                | Command::SetFieldIf {
                    component,
                    field,
                    ..
                } => scope_field(world.id(), *component, field)?,
                _ => {}
            }
        }
    }
    if let RequestBody::AnimationController(command) = body {
        scope_animation_command(world.id(), command)?;
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
                        entity_bindings: Vec::new(),
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
}
