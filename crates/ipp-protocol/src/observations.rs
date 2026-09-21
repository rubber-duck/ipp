//! Unsolicited GUI observation publication.
//!
//! Committed control effects with their conflicts and cancellations broadcast
//! as committed state; unhandled inputs go only to their supplying session so
//! raw input stays private. Messages chunk like Resources: at most 128
//! records within the transport budget, order preserved, each record in
//! exactly one message.
//!
//! Every observation text field keeps the standard 65536-byte wire string
//! bound. Legal values encode whole; oversize values fail instead of being
//! misrepresented as complete prefixes. Transient cursor effects (focus,
//! hover, scroll) never produce callbacks and stay off the wire.

use crate::codec::Writer;
use crate::wire::error_name;
use crate::{MAX_MESSAGE_BYTES, ProtocolError, ResponseBody};
use ipp_core::{
    EntityId, GuiControlValue, GuiInputCancellation, GuiInputCommand, GuiInputConflict,
    GuiInputConflictReason, GuiInputEffect, GuiInputEffectKind, GuiInputTarget, GuiNodeId,
    GuiTextFocusUpdate, GuiUnhandledInput, WorldUpdateReport,
};

/// Wire bound for every observation text field, matching all protocol strings.
pub const GUI_OBSERVATION_TEXT_BYTES: usize = 65536;

/// Maximum records in one observation message, mirroring Resources.
pub const GUI_OBSERVATIONS_PER_MESSAGE: usize = 128;

/// Inner payload budget per message, leaving envelope headroom under the
/// transport cap. Estimates always overstate, so chunks never overflow.
const INNER_BUDGET: usize = MAX_MESSAGE_BYTES - 65536;

/// Split one frame report into that session's unsolicited responses.
/// Committed button and control effects broadcast with conflicts and
/// cancellations; unhandled inputs filter to their supplying session and
/// transient cursor effects are skipped as unrepresentable.
pub fn gui_observation_bodies(report: &WorldUpdateReport, session: u64) -> Vec<ResponseBody> {
    let mut out = Vec::new();
    let effects: Vec<GuiInputEffect> = report
        .gui_input_effects
        .iter()
        .filter(|effect| {
            matches!(
                effect.kind,
                GuiInputEffectKind::ButtonPressed { .. }
                    | GuiInputEffectKind::ControlCommitted { .. }
            )
        })
        .cloned()
        .collect();
    for chunk in chunk_sized(effects, effect_size) {
        out.push(ResponseBody::GuiObservations {
            effects: chunk,
            conflicts: Vec::new(),
            cancellations: Vec::new(),
            text_focus_updates: Vec::new(),
        });
    }
    for chunk in chunk_sized(report.gui_input_conflicts.clone(), conflict_size) {
        out.push(ResponseBody::GuiObservations {
            effects: Vec::new(),
            conflicts: chunk,
            cancellations: Vec::new(),
            text_focus_updates: Vec::new(),
        });
    }
    for chunk in chunk_sized(report.gui_input_cancellations.clone(), cancellation_size) {
        out.push(ResponseBody::GuiObservations {
            effects: Vec::new(),
            conflicts: Vec::new(),
            cancellations: chunk,
            text_focus_updates: Vec::new(),
        });
    }
    for update in report
        .gui_text_focus_updates
        .iter()
        .filter(|update| match update {
            GuiTextFocusUpdate::Focused(state) => state.session == session,
            GuiTextFocusUpdate::Cleared {
                session: owner,
                ..
            } => *owner == session,
        })
    {
        out.push(ResponseBody::GuiObservations {
            effects: Vec::new(),
            conflicts: Vec::new(),
            cancellations: Vec::new(),
            text_focus_updates: vec![update.clone()],
        });
    }
    let unhandled: Vec<GuiUnhandledInput> = report
        .gui_unhandled_inputs
        .iter()
        .filter(|input| input.session == session)
        .cloned()
        .collect();
    for chunk in chunk_sized(unhandled, unhandled_size) {
        out.push(ResponseBody::GuiUnhandledInputs {
            inputs: chunk,
        });
    }
    out
}

/// Greedy chunking preserving order; every record lands in exactly one chunk.
/// A single record always fits: text caps at 64KB and paths are depth-bound.
fn chunk_sized<T>(records: Vec<T>, mut size_of: impl FnMut(&T) -> usize) -> Vec<Vec<T>> {
    let mut chunks: Vec<Vec<T>> = Vec::new();
    let mut bytes = 0usize;
    for record in records {
        let size = size_of(&record);
        let full = chunks
            .last()
            .is_none_or(|chunk: &Vec<T>| chunk.len() >= GUI_OBSERVATIONS_PER_MESSAGE);
        if full || bytes + size > INNER_BUDGET {
            chunks.push(Vec::new());
            bytes = 0;
        }
        chunks.last_mut().expect("observation chunk").push(record);
        bytes += size;
    }
    chunks
}

fn effect_size(effect: &GuiInputEffect) -> usize {
    let (path, value) = match &effect.kind {
        GuiInputEffectKind::ButtonPressed {
            path,
            ..
        } => (path, None),
        GuiInputEffectKind::ControlCommitted {
            path,
            value,
            ..
        } => (path, Some(value)),
        _ => return 0,
    };
    128 + 4 * path.len()
        + match value {
            Some(GuiControlValue::Text(text)) => text.len(),
            Some(_) => 8,
            None => 0,
        }
}

fn conflict_size(conflict: &GuiInputConflict) -> usize {
    128 + match &conflict.reason {
        GuiInputConflictReason::AdmissionFailed(reason) => error_name(*reason).len(),
        _ => 8,
    }
}

fn cancellation_size(_cancellation: &GuiInputCancellation) -> usize {
    128
}

fn unhandled_size(input: &GuiUnhandledInput) -> usize {
    256 + input_text_size(&input.input) + 32 * blocker_count(&input.input)
}

fn input_text_size(input: &GuiInputCommand) -> usize {
    match input {
        GuiInputCommand::Text {
            text,
        } => text.len(),
        GuiInputCommand::UpdateComposition {
            text,
            ..
        } => text.len(),
        _ => 0,
    }
}

fn blocker_count(input: &GuiInputCommand) -> usize {
    match input {
        GuiInputCommand::PointerDown {
            blockers,
            ..
        }
        | GuiInputCommand::PointerUp {
            blockers,
            ..
        }
        | GuiInputCommand::PointerMove {
            blockers,
            ..
        }
        | GuiInputCommand::Scroll {
            blockers,
            ..
        } => blockers.len(),
        _ => 0,
    }
}

/// Inner payload for one broadcast message: version, then effects, conflicts
/// and cancellations each with a bounded count.
pub(crate) fn encode_gui_observations_inner(
    effects: &[GuiInputEffect],
    conflicts: &[GuiInputConflict],
    cancellations: &[GuiInputCancellation],
    text_focus_updates: &[GuiTextFocusUpdate],
) -> Result<Vec<u8>, ProtocolError> {
    let total = effects.len() + conflicts.len() + cancellations.len() + text_focus_updates.len();
    if total == 0 {
        return Err(ProtocolError::Malformed("empty gui observations"));
    }
    if total > GUI_OBSERVATIONS_PER_MESSAGE {
        return Err(ProtocolError::Limit("gui observations"));
    }
    let mut w = Writer(Vec::new());
    w.u8(3)?;
    w.count(effects.len(), GUI_OBSERVATIONS_PER_MESSAGE)?;
    for effect in effects {
        write_effect(&mut w, effect)?;
    }
    w.count(conflicts.len(), GUI_OBSERVATIONS_PER_MESSAGE)?;
    for conflict in conflicts {
        write_conflict(&mut w, conflict)?;
    }
    w.count(cancellations.len(), GUI_OBSERVATIONS_PER_MESSAGE)?;
    for cancellation in cancellations {
        write_cancellation(&mut w, cancellation)?;
    }
    w.count(text_focus_updates.len(), 1)?;
    for update in text_focus_updates {
        write_text_focus_update(&mut w, update)?;
    }
    Ok(w.0)
}

fn write_text_focus_update(
    w: &mut Writer,
    update: &GuiTextFocusUpdate,
) -> Result<(), ProtocolError> {
    match update {
        GuiTextFocusUpdate::Cleared {
            session,
            context_generation,
            focus_generation,
        } => {
            w.u8(0)?;
            w.u64(*session)?;
            w.u64(*context_generation)?;
            w.u64(*focus_generation)?;
        }
        GuiTextFocusUpdate::Focused(state) => {
            w.u8(1)?;
            w.u64(state.session)?;
            w.u64(state.context_generation)?;
            w.u64(state.focus_generation)?;
            w.u64(state.target.entity.to_bits())?;
            w.u64(state.target.root_incarnation)?;
            w.u32(state.target.node.0)?;
            w.u32(state.target.lifetime)?;
            w.u32(state.revision)?;
            w.string(&state.text)?;
            w.u32(state.selection_start)?;
            w.u32(state.selection_end)?;
            match &state.composition {
                None => w.u8(0)?,
                Some(value) => {
                    w.u8(1)?;
                    w.string(&value.text)?;
                    w.u32(value.caret_start)?;
                    w.u32(value.caret_end)?;
                }
            }
        }
    }
    Ok(())
}

/// Inner payload for one supplier-private message: version, then inputs.
pub(crate) fn encode_gui_unhandled_inner(
    inputs: &[GuiUnhandledInput],
) -> Result<Vec<u8>, ProtocolError> {
    if inputs.is_empty() {
        return Err(ProtocolError::Malformed("empty gui unhandled inputs"));
    }
    if inputs.len() > GUI_OBSERVATIONS_PER_MESSAGE {
        return Err(ProtocolError::Limit("gui unhandled inputs"));
    }
    let mut w = Writer(Vec::new());
    w.u8(1)?;
    w.count(inputs.len(), GUI_OBSERVATIONS_PER_MESSAGE)?;
    for input in inputs {
        if input.session == 0 {
            return Err(ProtocolError::Malformed("gui unhandled session"));
        }
        w.u64(input.session)?;
        w.u64(input.tick)?;
        write_gui_input(&mut w, &input.input)?;
        match &input.reason {
            ipp_core::GuiUnhandledReason::NoPanelHit => w.u8(0)?,
            ipp_core::GuiUnhandledReason::Blocked {
                entity,
            } => {
                if entity.to_bits() == 0 {
                    return Err(ProtocolError::Malformed("gui unhandled blocker"));
                }
                w.u8(1)?;
                w.u64(entity.to_bits())?;
            }
            ipp_core::GuiUnhandledReason::StaleTarget => w.u8(2)?,
            ipp_core::GuiUnhandledReason::NoFocus => w.u8(3)?,
            ipp_core::GuiUnhandledReason::NoCapture => w.u8(4)?,
            ipp_core::GuiUnhandledReason::NotFocusable => w.u8(5)?,
            ipp_core::GuiUnhandledReason::NotOwner => w.u8(6)?,
        }
    }
    Ok(w.0)
}

fn write_effect(w: &mut Writer, effect: &GuiInputEffect) -> Result<(), ProtocolError> {
    if effect.session == 0 {
        return Err(ProtocolError::Malformed("gui observation session"));
    }
    match &effect.kind {
        GuiInputEffectKind::ButtonPressed {
            entity,
            root_incarnation,
            node,
            lifetime,
            path,
        } => {
            w.u8(0)?;
            write_effect_head(
                w,
                effect.session,
                effect.source_tick,
                effect.effect_tick,
                *entity,
                *root_incarnation,
                *node,
                *lifetime,
                path,
            )?;
        }
        GuiInputEffectKind::ControlCommitted {
            entity,
            root_incarnation,
            node,
            lifetime,
            value,
            revision,
            path,
        } => {
            w.u8(1)?;
            write_effect_head(
                w,
                effect.session,
                effect.source_tick,
                effect.effect_tick,
                *entity,
                *root_incarnation,
                *node,
                *lifetime,
                path,
            )?;
            w.u32(*revision)?;
            match value {
                GuiControlValue::Bool(value) => {
                    w.u8(1)?;
                    w.u8(u8::from(*value))?;
                }
                GuiControlValue::Scalar(value) => {
                    w.u8(2)?;
                    w.f32(*value)?;
                }
                GuiControlValue::Text(text) => {
                    w.u8(3)?;
                    write_bounded_text(w, text)?;
                }
                GuiControlValue::None => {
                    return Err(ProtocolError::Malformed("gui effect value"));
                }
            }
        }
        _ => return Err(ProtocolError::Malformed("transient gui effect")),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_effect_head(
    w: &mut Writer,
    session: u64,
    source_tick: u64,
    effect_tick: u64,
    entity: EntityId,
    root_incarnation: u64,
    node: GuiNodeId,
    lifetime: u32,
    path: &[GuiNodeId],
) -> Result<(), ProtocolError> {
    w.u64(session)?;
    w.u64(source_tick)?;
    w.u64(effect_tick)?;
    if entity.to_bits() == 0 {
        return Err(ProtocolError::Malformed("gui effect entity"));
    }
    w.u64(entity.to_bits())?;
    w.u64(root_incarnation)?;
    if node.0 == 0 {
        return Err(ProtocolError::Malformed("gui effect node"));
    }
    w.u32(node.0)?;
    w.u32(lifetime)?;
    w.count(path.len(), 65536)?;
    for id in path {
        if id.0 == 0 {
            return Err(ProtocolError::Malformed("gui effect path"));
        }
        w.u32(id.0)?;
    }
    Ok(())
}

fn write_target(w: &mut Writer, target: Option<&GuiInputTarget>) -> Result<(), ProtocolError> {
    match target {
        None => w.u8(0),
        Some(target) => {
            if target.entity.to_bits() == 0 || target.node.0 == 0 {
                return Err(ProtocolError::Malformed("gui observation target"));
            }
            w.u8(1)?;
            w.u64(target.entity.to_bits())?;
            w.u64(target.root_incarnation)?;
            w.u32(target.node.0)?;
            w.u32(target.lifetime)
        }
    }
}

fn write_conflict(w: &mut Writer, conflict: &GuiInputConflict) -> Result<(), ProtocolError> {
    if conflict.session == 0 {
        return Err(ProtocolError::Malformed("gui conflict session"));
    }
    w.u64(conflict.session)?;
    w.u64(conflict.source_tick)?;
    w.u64(conflict.effect_tick)?;
    write_target(w, conflict.target.as_ref())?;
    match &conflict.reason {
        GuiInputConflictReason::RevisionMismatch {
            expected,
            found,
        } => {
            w.u8(0)?;
            w.u32(*expected)?;
            w.u32(*found)?;
        }
        GuiInputConflictReason::AdmissionFailed(reason) => {
            w.u8(1)?;
            w.string(error_name(*reason))?;
        }
        GuiInputConflictReason::TouchArbitration {
            owner_pointer,
        } => {
            w.u8(2)?;
            w.u32(*owner_pointer)?;
        }
    }
    Ok(())
}

fn write_cancellation(
    w: &mut Writer,
    cancellation: &GuiInputCancellation,
) -> Result<(), ProtocolError> {
    if cancellation.session == 0 {
        return Err(ProtocolError::Malformed("gui cancellation session"));
    }
    w.u64(cancellation.session)?;
    w.u64(cancellation.source_tick)?;
    w.u64(cancellation.effect_tick)?;
    write_target(w, cancellation.target.as_ref())?;
    w.u8(match cancellation.reason {
        ipp_core::GuiInputCancelReason::TargetRemoved => 0,
        ipp_core::GuiInputCancelReason::TargetHidden => 1,
        ipp_core::GuiInputCancelReason::SessionReplaced => 2,
        ipp_core::GuiInputCancelReason::GestureCancelled => 3,
    })?;
    Ok(())
}

fn write_bounded_text(w: &mut Writer, text: &str) -> Result<(), ProtocolError> {
    w.string(text)
}

/// Input echo mirroring the admission encoding, versioned the same way.
fn write_gui_input(w: &mut Writer, input: &GuiInputCommand) -> Result<(), ProtocolError> {
    w.u8(1)?;
    match input {
        GuiInputCommand::PointerDown {
            pointer,
            panel,
            position,
            button,
            blockers,
            panel_distance,
        } => {
            w.u8(1)?;
            w.u32(*pointer)?;
            write_panel(w, panel)?;
            w.f32(position[0])?;
            w.f32(position[1])?;
            write_button(w, button)?;
            write_blockers(w, blockers)?;
            write_distance(w, panel_distance)?;
        }
        GuiInputCommand::PointerUp {
            pointer,
            panel,
            position,
            button,
            blockers,
            panel_distance,
        } => {
            w.u8(2)?;
            w.u32(*pointer)?;
            write_panel(w, panel)?;
            w.f32(position[0])?;
            w.f32(position[1])?;
            write_button(w, button)?;
            write_blockers(w, blockers)?;
            write_distance(w, panel_distance)?;
        }
        GuiInputCommand::PointerMove {
            pointer,
            panel,
            position,
            blockers,
            panel_distance,
        } => {
            w.u8(3)?;
            w.u32(*pointer)?;
            write_panel(w, panel)?;
            w.f32(position[0])?;
            w.f32(position[1])?;
            write_blockers(w, blockers)?;
            write_distance(w, panel_distance)?;
        }
        GuiInputCommand::PointerCancel {
            pointer,
        } => {
            w.u8(4)?;
            w.u32(*pointer)?;
        }
        GuiInputCommand::Scroll {
            panel,
            position,
            delta,
            blockers,
            panel_distance,
        } => {
            w.u8(5)?;
            write_panel(w, panel)?;
            w.f32(position[0])?;
            w.f32(position[1])?;
            w.f32(delta[0])?;
            w.f32(delta[1])?;
            write_blockers(w, blockers)?;
            write_distance(w, panel_distance)?;
        }
        GuiInputCommand::Key {
            key,
            pressed,
        } => {
            w.u8(6)?;
            w.u8(match key {
                ipp_core::GuiKey::Tab => 0,
                ipp_core::GuiKey::Enter => 1,
                ipp_core::GuiKey::Space => 2,
                ipp_core::GuiKey::Escape => 3,
                ipp_core::GuiKey::Backspace => 4,
                ipp_core::GuiKey::Delete => 5,
                ipp_core::GuiKey::Left => 6,
                ipp_core::GuiKey::Right => 7,
                ipp_core::GuiKey::Up => 8,
                ipp_core::GuiKey::Down => 9,
                ipp_core::GuiKey::Home => 10,
                ipp_core::GuiKey::End => 11,
            })?;
            w.u8(u8::from(*pressed))?;
        }
        GuiInputCommand::Text {
            text,
        } => {
            w.u8(7)?;
            write_bounded_text(w, text)?;
        }
        GuiInputCommand::Focus {
            handle,
        } => {
            w.u8(8)?;
            if handle.session == 0 || handle.entity.to_bits() == 0 || handle.node_id.0 == 0 {
                return Err(ProtocolError::Malformed("gui input focus"));
            }
            w.u64(handle.session)?;
            w.u64(handle.entity.to_bits())?;
            w.u64(handle.root_incarnation)?;
            w.u32(handle.node_id.0)?;
            w.u32(handle.node_lifetime)?;
        }
        GuiInputCommand::Blur => {
            w.u8(9)?;
        }
        GuiInputCommand::SetTextSelection {
            start,
            end,
        } => {
            w.u8(10)?;
            w.u32(*start)?;
            w.u32(*end)?;
        }
        GuiInputCommand::UpdateComposition {
            text,
            caret_start,
            caret_end,
        } => {
            w.u8(11)?;
            write_bounded_text(w, text)?;
            w.u32(*caret_start)?;
            w.u32(*caret_end)?;
        }
        GuiInputCommand::CommitComposition => {
            w.u8(12)?;
        }
        GuiInputCommand::CancelComposition => {
            w.u8(13)?;
        }
    }
    Ok(())
}

fn write_panel(w: &mut Writer, panel: &Option<EntityId>) -> Result<(), ProtocolError> {
    match panel {
        None => w.u8(0),
        Some(entity) => {
            if entity.to_bits() == 0 {
                return Err(ProtocolError::Malformed("gui input panel"));
            }
            w.u8(1)?;
            w.u64(entity.to_bits())
        }
    }
}

fn write_button(w: &mut Writer, button: &ipp_core::GuiPointerButton) -> Result<(), ProtocolError> {
    w.u8(match button {
        ipp_core::GuiPointerButton::Primary => 0,
        ipp_core::GuiPointerButton::Secondary => 1,
        ipp_core::GuiPointerButton::Auxiliary => 2,
    })
}

fn write_blockers(
    w: &mut Writer,
    blockers: &[ipp_core::GuiBlockerHit],
) -> Result<(), ProtocolError> {
    w.count(blockers.len(), 1024)?;
    for blocker in blockers {
        if blocker.entity.to_bits() == 0 {
            return Err(ProtocolError::Malformed("gui input blocker"));
        }
        w.u64(blocker.entity.to_bits())?;
        w.f32(blocker.distance)?;
    }
    Ok(())
}

fn write_distance(w: &mut Writer, distance: &Option<f32>) -> Result<(), ProtocolError> {
    match distance {
        None => w.u8(0),
        Some(distance) => {
            w.u8(1)?;
            w.f32(*distance)
        }
    }
}
