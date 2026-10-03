use super::*;

impl Writer {
    pub(crate) fn raw(&mut self, v: &[u8]) -> Result<(), ProtocolError> {
        let length = self
            .len()
            .checked_add(v.len())
            .filter(|length| *length <= MAX_MESSAGE_BYTES)
            .ok_or(ProtocolError::Limit("message"))?;
        if let Some(count) = &mut self.1 {
            *count = length;
        } else {
            self.0.extend_from_slice(v);
        }
        Ok(())
    }

    fn asset_error(&mut self, error: &str) -> Result<(), ProtocolError> {
        if error.len() > ipp_core::services::asset_management::MAX_ASSET_ERROR_BYTES {
            return Err(ProtocolError::Limit("asset error"));
        }
        self.string(error)
    }

    pub(crate) fn u8(&mut self, v: u8) -> Result<(), ProtocolError> {
        self.raw(&[v])
    }

    pub(crate) fn u16(&mut self, v: u16) -> Result<(), ProtocolError> {
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn u32(&mut self, v: u32) -> Result<(), ProtocolError> {
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn u64(&mut self, v: u64) -> Result<(), ProtocolError> {
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn f32(&mut self, v: f32) -> Result<(), ProtocolError> {
        if !v.is_finite() {
            return Err(ProtocolError::Malformed("nonfinite f32"));
        }
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn f64(&mut self, v: f64) -> Result<(), ProtocolError> {
        if !v.is_finite() || v < 0.0 {
            return Err(ProtocolError::Malformed("invalid simulation time"));
        }
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn count(&mut self, n: usize, max: usize) -> Result<(), ProtocolError> {
        if n > max {
            return Err(ProtocolError::Limit("count"));
        }
        self.u32(n as u32)
    }

    pub(crate) fn string(&mut self, s: &str) -> Result<(), ProtocolError> {
        self.count(s.len(), crate::MAX_FIELD_BYTES)?;
        self.raw(s.as_bytes())
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> Result<(), ProtocolError> {
        self.count(bytes.len(), crate::MAX_FIELD_BYTES)?;
        self.raw(bytes)
    }

    pub(super) fn metadata(&mut self, m: &EntityMetadata) -> Result<(), ProtocolError> {
        match &m.symbolic_id {
            None => self.u8(OPTION_NONE)?,
            Some(s) => {
                self.u8(OPTION_SOME)?;
                self.string(s)?;
            }
        }
        self.count(m.classes.len(), crate::MAX_METADATA_CLASSES)?;
        for class in &m.classes {
            self.string(class)?;
        }
        Ok(())
    }

    pub(super) fn outcome(&mut self, outcome: &BatchOutcome) -> Result<(), ProtocolError> {
        self.u64(outcome.batch_id)?;
        self.u64(outcome.tick)?;
        let aliases = match &outcome.result {
            Ok(aliases) => {
                self.u8(OUTCOME_SUCCESS)?;
                aliases
            }
            Err(error) => {
                self.u8(OUTCOME_FAILURE)?;
                self.u8(match error.scope {
                    ipp_core::BatchErrorScope::Operation => BATCH_ERROR_OPERATION,
                    ipp_core::BatchErrorScope::Commit => BATCH_ERROR_COMMIT,
                })?;
                self.u8(if error.operation.is_some() {
                    OPTION_SOME
                } else {
                    OPTION_NONE
                })?;
                if let Some(operation) = error.operation {
                    self.u32(
                        u32::try_from(operation)
                            .map_err(|_| ProtocolError::Limit("operation index"))?,
                    )?;
                }
                self.string(error_name(error.reason))?;
                &error.aliases
            }
        };
        self.count(aliases.len(), crate::BATCH_OUTCOME_ALIASES)?;
        for (alias, id) in aliases {
            self.u32(*alias)?;
            self.u64(id.to_bits())?;
        }
        self.count(outcome.symbols.len(), crate::BATCH_OUTCOME_ALIASES)?;
        for (symbol, id) in &outcome.symbols {
            self.string(symbol)?;
            self.u64(id.to_bits())?;
        }
        Ok(())
    }

    pub(super) fn resource(
        &mut self,
        resource: &ipp_core::AssetResourceSnapshot,
    ) -> Result<(), ProtocolError> {
        self.u64(resource.id)?;
        self.u16(resource.kind.0)?;
        self.string(&resource.source)?;
        self.u32(resource.variant)?;
        match &resource.status {
            ipp_core::AssetResourceStatus::Unloaded => self.u8(RESOURCE_UNLOADED)?,
            ipp_core::AssetResourceStatus::Start => self.u8(RESOURCE_START)?,
            ipp_core::AssetResourceStatus::Progress {
                completed,
                total,
            } => {
                self.u8(RESOURCE_PROGRESS)?;
                self.u64(*completed)?;
                self.u8(if total.is_some() {
                    OPTION_SOME
                } else {
                    OPTION_NONE
                })?;
                if let Some(total) = total {
                    self.u64(*total)?;
                }
            }
            ipp_core::AssetResourceStatus::Loaded => self.u8(RESOURCE_LOADED)?,
            ipp_core::AssetResourceStatus::Failed(error) => {
                self.u8(RESOURCE_FAILED)?;
                self.asset_error(error)?;
            }
        }
        let representation = resource.representation;
        self.u8(u8::from(representation.decoded))?;
        self.u8(if representation.graphics_ready.is_some() {
            OPTION_SOME
        } else {
            OPTION_NONE
        })?;
        if let Some(ready) = representation.graphics_ready {
            self.u8(u8::from(ready))?;
        }
        self.u64(representation.source_bytes)?;
        self.u64(representation.resident_bytes)?;
        self.u8(if representation.graphics_bytes.is_some() {
            OPTION_SOME
        } else {
            OPTION_NONE
        })?;
        if let Some(bytes) = representation.graphics_bytes {
            self.u64(bytes)?;
        }
        Ok(())
    }

    /// Encode one inspected component with its own named-property descriptor table.
    pub(super) fn component(&mut self, component: &ComponentValue) -> Result<(), ProtocolError> {
        self.u16(component.type_id())?;
        let fields = component.fields();
        self.count(fields.len(), crate::MAX_INSPECTED_FIELDS)?;
        for (offset, value) in fields {
            self.resolved_field(offset, value)?;
        }
        Ok(())
    }

    pub(crate) fn canvas_state(
        &mut self,
        state: &ipp_core::CanvasState,
    ) -> Result<(), ProtocolError> {
        self.f32(state.extent[0])?;
        self.f32(state.extent[1])?;
        self.f32(state.units_per_metre)
    }

    pub(super) fn render_state_patch(
        &mut self,
        patch: &ipp_core::RenderStatePatch,
    ) -> Result<(), ProtocolError> {
        self.u16(
            u16::from(patch.show_all_debug_geometries.is_some())
                | (u16::from(patch.debug_geometry_color.is_some()) << 1)
                | (u16::from(patch.ambient_light.is_some()) << 2),
        )?;
        if let Some(value) = patch.show_all_debug_geometries {
            self.u8(u8::from(value))?;
        }
        if let Some(color) = patch.debug_geometry_color {
            for value in color {
                if !(0.0..=1.0).contains(&value) {
                    return Err(ProtocolError::Malformed("render state color"));
                }
                self.f32(value)?;
            }
        }
        if let Some(color) = patch.ambient_light {
            for value in color {
                if value < 0.0 {
                    return Err(ProtocolError::Malformed("render state ambient light"));
                }
                self.f32(value)?;
            }
        }
        Ok(())
    }

    /// Encode one snapshot field: offset, value kind and value.
    pub(crate) fn resolved_field(
        &mut self,
        offset: u32,
        value: ResolvedValue,
    ) -> Result<(), ProtocolError> {
        self.u32(offset)?;
        self.u8(value.kind() as u8)?;
        match value {
            ResolvedValue::World(value) => {
                self.u8(u8::from(value.is_some()))?;
                if let Some(value) = value {
                    self.world_reference(value.into())?;
                }
            }
            ResolvedValue::Output(value) => {
                self.u8(u8::from(value.is_some()))?;
                if let Some(value) = value {
                    self.output_reference(value.into())?;
                }
            }
            ResolvedValue::Dynamic(v) => self.bytes(&v.encode())?,
            ResolvedValue::Bool(v) => self.u8(u8::from(v))?,
            ResolvedValue::F32(v) => self.f32(v)?,
            ResolvedValue::U32(v) => self.u32(v)?,
            ResolvedValue::U64(v) => self.u64(v)?,
            ResolvedValue::String(v) => self.string(&v)?,
            ResolvedValue::Bytes(v) => {
                // Dense GUI descriptor tables may exceed an authored byte value.
                // Inspection remains bounded by the complete response budget.
                self.count(v.len(), MAX_MESSAGE_BYTES)?;
                self.raw(&v)?;
            }
            ResolvedValue::Entity(id) => {
                self.u8(REF_HANDLE)?;
                self.u64(id.to_bits())?;
            }
            ResolvedValue::Rows(table) => {
                // One typed table value; the response budget bounds it.
                self.count(table.len(), MAX_MESSAGE_BYTES)?;
                self.raw(&table)?;
            }
            ResolvedValue::Unset => {}
        }
        Ok(())
    }
}

/// Encode owned results, rejecting oversized or unsupported response data.
pub fn encode_response(response: &Response) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = Vec::with_capacity(encoded_response_size(response)?);
    encode_response_into(response, &mut bytes)?;
    Ok(bytes)
}

/// Validate and measure the exact wire allocation without allocating a response.
pub fn encoded_response_size(response: &Response) -> Result<usize, ProtocolError> {
    let mut writer = Writer::measuring();
    write_response(response, &mut writer)?;
    Ok(writer.len())
}

/// Encode into exclusive caller-owned storage, retaining capacity even on failure.
/// Failed encodes leave an empty buffer; no partial message can be published.
pub fn encode_response_into(response: &Response, bytes: &mut Vec<u8>) -> Result<(), ProtocolError> {
    bytes.clear();
    let mut writer = Writer::new(std::mem::take(bytes));
    let result = write_response(response, &mut writer);
    *bytes = writer.0;
    if result.is_err() {
        bytes.clear();
    }
    result
}

fn write_response(response: &Response, w: &mut Writer) -> Result<(), ProtocolError> {
    if response.session == 0 {
        return Err(ProtocolError::SessionMismatch);
    }
    let unsolicited = matches!(
        response.body,
        ResponseBody::BatchAborted { .. }
            | ResponseBody::RuntimeFailure { .. }
            | ResponseBody::LifecycleEvents(_)
            | ResponseBody::PlaybackEvents(_)
            | ResponseBody::RenderStateUpdatedEvent(_)
            | ResponseBody::Frame { .. }
            | ResponseBody::Resources { .. }
    );
    let unsolicited = unsolicited
        || matches!(
            response.body,
            ResponseBody::GuiObservation(
                ipp_core::systems::gui::observations::GuiObservationRecord::Effect { .. }
            )
        );
    if unsolicited != (response.request_id == 0) {
        return Err(ProtocolError::Malformed("reserved response identity"));
    }

    w.u64(response.session)?;
    w.u64(response.request_id)?;
    w.u64(response.tick)?;
    match &response.body {
        ResponseBody::LifecycleDiagnostics(sample) => {
            if response.tick != 0
                || sample.endpoint.world.id == 0
                || sample.endpoint.world.incarnation == 0
                || sample.endpoint.output == 0
            {
                return Err(ProtocolError::Malformed("lifecycle diagnostic envelope"));
            }
            w.u8(RESPONSE_LIFECYCLE_DIAGNOSTICS)?;
            w.u64(sample.endpoint.world.id)?;
            w.u64(sample.endpoint.world.incarnation)?;
            w.u64(sample.endpoint.output)?;
            w.u64(sample.work.lookups)?;
            w.u64(sample.work.recipient_visits)?;
            w.u8(u8::from(sample.work.saturated))?;
            w.u64(sample.traffic.queued_events)?;
            w.u64(sample.traffic.queued_bytes)?;
            w.u8(u8::from(sample.traffic.saturated))?;
        }
        ResponseBody::GuiObservation(record) => {
            if response.tick != 0 {
                return Err(ProtocolError::Malformed("GUI observation outer tick"));
            }
            if let ipp_core::systems::gui::observations::GuiObservationRecord::Control {
                request,
                ..
            } = record
                && *request != response.request_id
            {
                return Err(ProtocolError::Malformed("GUI observation correlation"));
            }
            w.u8(RESPONSE_GUI_OBSERVATION)?;
            w.gui_observation(record)?;
        }
        ResponseBody::RuntimeFailure {
            scope,
            faulted,
            message,
        } => {
            if message.len() > crate::MAX_FAILURE_MESSAGE_BYTES {
                return Err(ProtocolError::Limit("runtime diagnostic"));
            }
            w.u8(RESPONSE_RUNTIME_FAILURE)?;
            w.u8(*scope as u8)?;
            w.u8(u8::from(*faulted))?;
            w.string(message)?;
        }

        ResponseBody::LifecycleSubscription => w.u8(RESPONSE_LIFECYCLE_SUBSCRIPTION)?,
        ResponseBody::LifecycleEvents(output) => w.lifecycle_output(output)?,
        ResponseBody::AnimationController(id) => {
            w.u8(RESPONSE_CONTROLLER)?;
            w.u64(id.map(|id| id.to_bits()).unwrap_or(0))?;
        }
        ResponseBody::PlaybackEvents(events) => {
            w.u8(RESPONSE_PLAYBACK)?;
            w.count(events.len(), crate::MAX_PLAYBACK_EVENTS)?;
            for event in events {
                w.controller_state(&event.controller)?;
                w.u32(event.kind as u32)?;
                w.string(event.reason.map(error_name).unwrap_or(""))?;
            }
        }
        ResponseBody::RenderStateUpdatedEvent(change) => {
            if change.tick != response.tick {
                return Err(ProtocolError::Malformed("render state change tick"));
            }
            if change.changes == ipp_core::RenderStatePatch::default() {
                return Err(ProtocolError::Malformed("empty render state change"));
            }
            w.u8(RESPONSE_RENDER_STATE_UPDATED)?;
            w.render_state_patch(&change.changes)?;
        }
        ResponseBody::BatchAborted {
            batch_id,
            message,
        } => {
            w.u8(RESPONSE_BATCH_ABORTED)?;
            w.u64(*batch_id)?;
            w.string(message)?;
        }
        ResponseBody::Batch(outcome) => {
            w.u8(RESPONSE_BATCH)?;
            w.outcome(&outcome.outcome)?;
            if outcome.effects.len() != outcome.outcome.effects.len() {
                return Err(ProtocolError::Malformed("unmapped attachment effects"));
            }
            w.count(outcome.effects.len(), crate::BATCH_OUTCOME_EFFECTS)?;
            for effect in &outcome.effects {
                use crate::attachment_receipts::{AttachmentEffectKind, BatchOperationEffect};
                let effect = match effect {
                    BatchOperationEffect::Attachment(effect) => effect,
                    BatchOperationEffect::Adopted {
                        operation,
                    } => {
                        w.u32(*operation)?;
                        w.u8(OPERATION_ADOPTED)?;
                        continue;
                    }
                };
                w.u32(effect.operation)?;
                w.u8(match effect.kind {
                    AttachmentEffectKind::Written => ATTACHMENT_WRITTEN,
                    AttachmentEffectKind::Detached => ATTACHMENT_DETACHED,
                    AttachmentEffectKind::Superseded => ATTACHMENT_SUPERSEDED,
                })?;
                let receipt = &effect.receipt;
                w.u64(receipt.id)?;
                w.world_reference(receipt.parent)?;
                w.u64(receipt.anchor)?;
                w.u64(receipt.incarnation)?;
                w.u64(receipt.revision)?;
                w.u8(u8::from(receipt.child.is_some()))?;
                if let Some(child) = receipt.child {
                    w.world_reference(child)?;
                }
            }
        }
        ResponseBody::AttachmentReceipt {
            receipt,
            retired,
        } => {
            w.u8(RESPONSE_ATTACHMENT_RECEIPT)?;
            w.u64(*receipt)?;
            w.u8(match retired {
                None => RECEIPT_RELEASED,
                Some(false) => RECEIPT_PENDING,
                Some(true) => RECEIPT_RETIRED,
            })?;
        }
        ResponseBody::CameraNavigated => w.u8(RESPONSE_CAMERA_NAVIGATED)?,
        ResponseBody::GeometryPickResultEvent(outcome) => {
            if outcome.request_id != response.request_id || outcome.tick != response.tick {
                return Err(ProtocolError::Malformed("geometry outcome correlation"));
            }
            w.u8(RESPONSE_GEOMETRY_PICK)?;
            match &outcome.result {
                Ok((view, None)) => {
                    w.u8(PICK_OUTCOME_MISS)?;
                    w.view_descriptor(view)?;
                }
                Ok((view, Some(hit))) => {
                    if hit.identity.hit.distance < 0.0 {
                        return Err(ProtocolError::Malformed("invalid geometry hit"));
                    }
                    w.u8(PICK_OUTCOME_HIT)?;
                    w.view_descriptor(view)?;
                    w.world_reference(hit.identity.world.into())?;
                    w.publication_reference(hit.identity.publication)?;
                    w.u64(hit.identity.entity.to_bits())?;
                    w.u64(hit.identity.incarnation)?;
                    w.u16(hit.identity.component)?;
                    w.u8(if hit.identity.row.is_some() {
                        OPTION_SOME
                    } else {
                        OPTION_NONE
                    })?;
                    if let Some(row) = hit.identity.row {
                        w.u32(row.series)?;
                        w.u64(row.row_id.0)?;
                    }
                    for coordinate in hit.position {
                        w.f32(coordinate)?;
                    }
                    w.f64(hit.identity.hit.distance)?;
                    w.u32(hit.identity.hit.part)?;
                    w.count(hit.identity.path.len(), MAX_MESSAGE_BYTES / 24)?;
                    for (world, anchor) in &hit.identity.path {
                        w.world_reference((*world).into())?;
                        w.u64(anchor.to_bits())?;
                    }
                    w.u8(if hit.view_plane.is_some() {
                        OPTION_SOME
                    } else {
                        OPTION_NONE
                    })?;
                    if let Some(plane) = hit.view_plane {
                        for coordinate in plane.point.into_iter().chain(plane.normal) {
                            w.f32(coordinate)?;
                        }
                    }
                }
                Err(reason) => {
                    w.u8(PICK_OUTCOME_FAILURE)?;
                    w.string(error_name(*reason))?;
                }
            }
        }
        ResponseBody::CameraProjectResultEvent(outcome) => {
            if outcome.request_id != response.request_id || outcome.tick != response.tick {
                return Err(ProtocolError::Malformed("camera projection correlation"));
            }
            w.u8(RESPONSE_CAMERA_PROJECT)?;
            w.u8(if outcome.result.is_ok() {
                OPTION_SOME
            } else {
                OPTION_NONE
            })?;
            if let Ok((view, _)) = &outcome.result {
                w.view_descriptor(view)?;
            }
            w.u8(u8::from(outcome.result.is_ok()))?;
            if let Ok((_, Some(position))) = outcome.result {
                w.u8(OPTION_SOME)?;
                for coordinate in position {
                    w.f32(coordinate)?;
                }
            } else {
                w.u8(OPTION_NONE)?;
            }
            if let Err(reason) = outcome.result {
                w.u8(OPTION_SOME)?;
                w.string(error_name(reason))?;
            } else {
                w.u8(OPTION_NONE)?;
            }
        }
        ResponseBody::Resources {
            resources,
        } => {
            if resources.is_empty() {
                return Err(ProtocolError::Malformed("empty resource event"));
            }
            w.u8(RESPONSE_RESOURCES)?;
            w.count(resources.len(), crate::MAX_RESOURCE_EVENT_RECORDS)?;
            for resource in resources {
                if resource.id == 0 || resource.kind.0 == 0 || resource.source.is_empty() {
                    return Err(ProtocolError::Malformed("invalid resource event"));
                }
                w.resource(resource)?;
            }
        }
        ResponseBody::Frame {
            time,
        } => {
            w.u8(RESPONSE_FRAME)?;
            w.f64(*time)?;
        }
        ResponseBody::EntityTree {
            next,
            time,
            nodes,
        } => {
            w.u8(RESPONSE_ENTITY_TREE)?;
            w.f64(*time)?;
            w.u64(*next)?;
            w.count(nodes.len(), crate::INSPECTION_PAGE_RECORDS)?;
            for node in nodes {
                w.u64(node.id.to_bits())?;
                w.u64(node.parent.map_or(0, ipp_core::EntityId::to_bits))?;
                w.u64(node.order as u64)?;
                w.u64((node.order >> 64) as u64)?;
                w.u16(node.depth)?;
            }
        }
        ResponseBody::Inspect {
            next,
            controllers,
            time,
            entities,
            resources,
            render_diagnostics,
            gui_focus,
            gui_pointers,
            gui_active_items,
            canvas,
            gui_preferences,
        } => {
            w.u8(RESPONSE_INSPECT)?;
            w.f64(*time)?;
            w.u64(*next)?;
            w.count(entities.len(), crate::INSPECTION_PAGE_RECORDS)?;
            for entity in entities {
                w.u64(entity.id.to_bits())?;
                w.metadata(&entity.metadata)?;
                w.u64(entity.link.parent.map_or(0, ipp_core::EntityId::to_bits))?;
                w.u64(entity.link.order.value() as u64)?;
                w.u64((entity.link.order.value() >> 64) as u64)?;
                w.count(entity.components.len(), crate::MAX_INSPECTED_COMPONENTS)?;
                for component in &entity.components {
                    w.component(component)?;
                }
            }
            {
                w.count(resources.len(), crate::INSPECTION_PAGE_RECORDS)?;
                for resource in resources {
                    w.resource(resource)?;
                }
            }
            {
                w.count(render_diagnostics.len(), crate::INSPECTION_PAGE_RECORDS)?;
                for diagnostic in render_diagnostics {
                    w.u64(diagnostic.entity.to_bits())?;
                    w.string(error_name(diagnostic.reason))?;
                }
            }
            {
                w.count(controllers.len(), crate::INSPECTION_PAGE_RECORDS)?;
                for controller in controllers {
                    w.controller(controller)?;
                }
            }
            w.count(gui_focus.len(), crate::INSPECTION_PAGE_RECORDS)?;
            for record in gui_focus {
                w.gui_focus_record(record)?;
            }
            w.count(gui_pointers.len(), crate::INSPECTION_PAGE_RECORDS)?;
            for record in gui_pointers {
                w.gui_pointer_record(record)?;
            }
            w.count(gui_active_items.len(), crate::INSPECTION_PAGE_RECORDS)?;
            for record in gui_active_items {
                w.gui_active_item_record(record)?;
            }
            w.u8(u8::from(canvas.is_some()))?;
            if let Some(record) = canvas {
                w.canvas_state(&record.state)?;
                w.u8(u8::from(record.evaluated.is_some()))?;
                if let Some(evaluated) = record.evaluated {
                    w.f32(evaluated.extent[0])?;
                    w.f32(evaluated.extent[1])?;
                    w.u64(evaluated.tick)?;
                }
            }
            w.u8(u8::from(gui_preferences.is_some()))?;
            if let Some(preferences) = gui_preferences {
                w.u8(u8::from(preferences.reduced_motion))?;
            }
        }
        ResponseBody::Error {
            code,
            message,
        } => {
            w.u8(RESPONSE_ERROR)?;
            w.u16(*code)?;
            w.string(message)?;
        }
    }
    Ok(())
}
