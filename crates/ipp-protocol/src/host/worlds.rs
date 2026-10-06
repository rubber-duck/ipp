//! World creation options, admission manifests, selectors and capacity hints carried
//! by Host requests and responses.

use crate::ProtocolError;
use crate::codec::{Reader, Writer};
use crate::contract::wire_manifest::*;
use ipp_core::{
    WorldCapacityHints, WorldDescriptor, WorldId, WorldMetadata, WorldPersistentId, WorldSelector,
    WorldSystemCapacityHints,
};
use std::collections::BTreeMap;

/// Owned wire-facing creation request; remote names resolve against Host factories.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldCreateOptions {
    /// Host-unique symbolic name, or empty for automatic naming.
    pub symbolic_id: String,
    /// Storage reservations independent of selected Systems.
    pub capacity_hints: WorldCapacityHints,
    /// Registered factory names the World instantiates, exactly. The wire keeps
    /// absence representable only so the Host can refuse it: there is no
    /// default selection.
    pub selected_systems: Option<Vec<String>>,
    /// Initial canvas extent and density, accepted only when the selection
    /// includes the Canvas System; absent selects the defaults.
    pub canvas: Option<ipp_core::CanvasState>,
}

impl WorldCreateOptions {
    /// Automatically named options with default reservations for this selection.
    pub fn new(selected_systems: Vec<String>) -> Self {
        Self {
            symbolic_id: String::new(),
            capacity_hints: WorldCapacityHints::default(),
            selected_systems: Some(selected_systems),
            canvas: None,
        }
    }
}

/// Selected World admission, independent of the compiled target schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldManifest {
    /// Resolved System identities in execution order.
    pub systems: Vec<String>,
    /// Admitted compiled component identities.
    pub components: Vec<u16>,
    /// Stable operation identities in the Host control contract.
    pub operations: Vec<u8>,
}

impl WorldManifest {
    /// Copy a published World's immutable manifest into an owned wire reply.
    pub fn from_core(manifest: &ipp_core::systems::WorldManifest) -> Self {
        use ipp_core::systems::WorldOperation;
        Self {
            systems: manifest
                .systems()
                .iter()
                .map(|system| system.0.to_owned())
                .collect(),
            components: manifest.components().collect(),
            operations: manifest
                .operations()
                .map(|operation| match operation {
                    WorldOperation::EntityLinks => 0,
                    // Operation 1 is retired; retired identifiers are never reused.
                    WorldOperation::Animation => 2,
                    WorldOperation::JointAnimation => 3,
                    WorldOperation::Constraints => 4,
                    WorldOperation::LookAt => 5,
                    WorldOperation::Geometry => 6,
                    WorldOperation::Rendering => 7,
                    WorldOperation::Camera => 8,
                    WorldOperation::Surface => 9,
                    WorldOperation::Gui => 10,
                    WorldOperation::Particles => 11,
                    WorldOperation::Canvas => 12,
                })
                .collect(),
        }
    }
}

pub(super) fn read_system_selection(
    reader: &mut Reader<'_>,
) -> Result<Option<Vec<String>>, ProtocolError> {
    if !reader.boolean()? {
        return Ok(None);
    }
    let count = reader.count(1024)?;
    let mut selected = Vec::with_capacity(count);
    for _ in 0..count {
        let name = reader.string()?;
        if name.is_empty() || selected.contains(&name) {
            return Err(ProtocolError::Malformed("selected system"));
        }
        selected.push(name);
    }
    Ok(Some(selected))
}

pub(super) fn write_system_selection(
    writer: &mut Writer,
    selected: &Option<Vec<String>>,
) -> Result<(), ProtocolError> {
    writer.u8(u8::from(selected.is_some()))?;
    if let Some(selected) = selected {
        writer.count(selected.len(), 1024)?;
        for name in selected {
            writer.string(name)?;
        }
    }
    Ok(())
}

pub(super) fn read_manifest(reader: &mut Reader<'_>) -> Result<WorldManifest, ProtocolError> {
    let count = reader.count(1024)?;
    let systems = (0..count)
        .map(|_| reader.string())
        .collect::<Result<Vec<_>, _>>()?;
    let count = reader.count(1024)?;
    let components = (0..count)
        .map(|_| reader.u16())
        .collect::<Result<Vec<_>, _>>()?;
    let count = reader.count(32)?;
    let operations = (0..count)
        .map(|_| reader.u8())
        .collect::<Result<Vec<_>, _>>()?;
    if operations.iter().any(|&operation| operation > 12) {
        return Err(ProtocolError::Malformed("World operation"));
    }
    Ok(WorldManifest {
        systems,
        components,
        operations,
    })
}

pub(super) fn write_manifest(
    writer: &mut Writer,
    manifest: &WorldManifest,
) -> Result<(), ProtocolError> {
    writer.count(manifest.systems.len(), 1024)?;
    for system in &manifest.systems {
        writer.string(system)?;
    }
    writer.count(manifest.components.len(), 1024)?;
    for component in &manifest.components {
        writer.u16(*component)?;
    }
    writer.count(manifest.operations.len(), 32)?;
    for operation in &manifest.operations {
        writer.u8(*operation)?;
    }
    Ok(())
}

/// Accept a standalone World session's hello and append that World's admission manifest.
pub fn accept_world_hello(
    bytes: &[u8],
    session: u64,
    manifest: &ipp_core::systems::WorldManifest,
) -> Result<Vec<u8>, ProtocolError> {
    let mut reply = crate::contract::accept_hello(bytes, session)?;
    let mut writer = Writer::new(Vec::new());
    write_manifest(&mut writer, &WorldManifest::from_core(manifest))?;
    reply.extend(writer.0);
    Ok(reply)
}

pub(super) fn read_selector(reader: &mut Reader<'_>) -> Result<WorldSelector, ProtocolError> {
    match reader.u8()? {
        WORLD_SELECTOR_ID => Ok(WorldSelector::Id(WorldId(reader.u64()?))),
        WORLD_SELECTOR_SYMBOL => Ok(WorldSelector::SymbolicId(reader.string()?)),
        tag => Err(ProtocolError::Unsupported(tag)),
    }
}

pub(super) fn write_selector(
    writer: &mut Writer,
    selector: &WorldSelector,
) -> Result<(), ProtocolError> {
    match selector {
        WorldSelector::Id(id) => {
            writer.u8(WORLD_SELECTOR_ID)?;
            writer.u64(id.0)
        }
        WorldSelector::SymbolicId(symbol) => {
            writer.u8(WORLD_SELECTOR_SYMBOL)?;
            writer.string(symbol)
        }
    }
}

fn read_hints(reader: &mut Reader<'_>) -> Result<WorldCapacityHints, ProtocolError> {
    let entities = reader.u32()? as usize;
    let count = reader.count(1024)?;
    let mut systems = BTreeMap::new();
    for _ in 0..count {
        let system = reader.string()?;
        let count = reader.count(1024)?;
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let key = reader.string()?;
            let value = reader.u32()? as usize;
            if values.insert(key, value).is_some() {
                return Err(ProtocolError::Malformed("duplicate capacity hint"));
            }
        }
        if systems
            .insert(system, WorldSystemCapacityHints(values))
            .is_some()
        {
            return Err(ProtocolError::Malformed("duplicate system hints"));
        }
    }
    Ok(WorldCapacityHints {
        entities,
        systems,
    })
}

fn write_hints(writer: &mut Writer, hints: &WorldCapacityHints) -> Result<(), ProtocolError> {
    writer.u32(
        u32::try_from(hints.entities).map_err(|_| ProtocolError::Limit("entity reservation"))?,
    )?;
    writer.count(hints.systems.len(), 1024)?;
    for (system, hints) in &hints.systems {
        writer.string(system)?;
        writer.count(hints.0.len(), 1024)?;
        for (key, &value) in &hints.0 {
            writer.string(key)?;
            writer.u32(
                u32::try_from(value).map_err(|_| ProtocolError::Limit("system reservation"))?,
            )?;
        }
    }
    Ok(())
}

pub(super) fn write_world(
    writer: &mut Writer,
    world: &WorldDescriptor,
) -> Result<(), ProtocolError> {
    writer.u64(world.id.0)?;
    writer.string(&world.metadata.symbolic_id)?;
    writer.raw(&world.metadata.persistent_id.0.to_le_bytes())?;
    write_hints(writer, &world.capacity_hints)
}

pub(super) fn read_world(reader: &mut Reader<'_>) -> Result<WorldDescriptor, ProtocolError> {
    let id = WorldId(reader.u64()?);
    let symbolic_id = reader.string()?;
    let persistent_id = WorldPersistentId(u128::from_le_bytes(
        reader.take(16)?.try_into().expect("checked length"),
    ));
    Ok(WorldDescriptor {
        id,
        metadata: WorldMetadata {
            symbolic_id,
            persistent_id,
        },
        capacity_hints: read_hints(reader)?,
    })
}

pub(super) fn read_hints_patch(
    reader: &mut Reader<'_>,
) -> Result<ipp_core::WorldCapacityHintsPatch, ProtocolError> {
    let entities = if reader.boolean()? {
        Some(reader.u32()? as usize)
    } else {
        None
    };
    let count = reader.count(1024)?;
    let mut systems = BTreeMap::new();
    for _ in 0..count {
        let system = reader.string()?;
        let count = reader.count(1024)?;
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let key = reader.string()?;
            let value = reader.u32()? as usize;
            if values.insert(key, value).is_some() {
                return Err(ProtocolError::Malformed("duplicate capacity hint"));
            }
        }
        if systems
            .insert(system, WorldSystemCapacityHints(values))
            .is_some()
        {
            return Err(ProtocolError::Malformed("duplicate system hints"));
        }
    }
    Ok(ipp_core::WorldCapacityHintsPatch {
        entities,
        systems,
    })
}

pub(super) fn write_hints_patch(
    writer: &mut Writer,
    hints: &ipp_core::WorldCapacityHintsPatch,
) -> Result<(), ProtocolError> {
    writer.u8(u8::from(hints.entities.is_some()))?;
    if let Some(entities) = hints.entities {
        writer.u32(
            u32::try_from(entities).map_err(|_| ProtocolError::Limit("entity reservation"))?,
        )?;
    }
    writer.count(hints.systems.len(), 1024)?;
    for (system, hints) in &hints.systems {
        writer.string(system)?;
        writer.count(hints.0.len(), 1024)?;
        for (key, &value) in &hints.0 {
            writer.string(key)?;
            writer.u32(
                u32::try_from(value).map_err(|_| ProtocolError::Limit("system reservation"))?,
            )?;
        }
    }
    Ok(())
}
