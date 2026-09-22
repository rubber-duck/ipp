use super::*;

impl Writer {
    pub(crate) fn raw(&mut self, v: &[u8]) -> Result<(), ProtocolError> {
        if self
            .0
            .len()
            .checked_add(v.len())
            .is_none_or(|n| n > MAX_MESSAGE_BYTES)
        {
            return Err(ProtocolError::Limit("message"));
        }
        self.0.extend_from_slice(v);
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

    pub(super) fn f64(&mut self, v: f64) -> Result<(), ProtocolError> {
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
        self.count(s.len(), 65536)?;
        self.raw(s.as_bytes())
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> Result<(), ProtocolError> {
        self.count(bytes.len(), 65536)?;
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
        self.count(m.classes.len(), 256)?;
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
        self.count(aliases.len(), 4096)?;
        for (alias, id) in aliases {
            self.u32(*alias)?;
            self.u64(id.to_bits())?;
        }
        {
            self.count(outcome.state_overlays.len(), 4096)?;
            for resource in &outcome.state_overlays {
                self.u32(resource.alias)?;
                self.u64(resource.id)?;
                self.u8(match resource.kind {
                    ipp_core::StateOverlayHandleKind::Owner => STATE_OVERLAY_KIND_OWNER,
                    ipp_core::StateOverlayHandleKind::EntityOverlayBinding => {
                        STATE_OVERLAY_KIND_ENTITY_BINDING
                    }
                    ipp_core::StateOverlayHandleKind::ComponentStateOverlay => {
                        STATE_OVERLAY_KIND_COMPONENT
                    }
                })?;
                self.u8(if resource.entity.is_some() {
                    OPTION_SOME
                } else {
                    OPTION_NONE
                })?;
                if let Some(entity) = resource.entity {
                    self.u64(entity.to_bits())?;
                }
            }
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

    pub(super) fn component(&mut self, component: &ComponentValue) -> Result<(), ProtocolError> {
        self.u16(component.type_id())?;
        let fields = component.fields();
        self.count(fields.len(), 65_536)?;
        for (offset, value) in fields {
            self.resolved_field(offset, value)?;
        }
        Ok(())
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

    pub(super) fn resolved_field(
        &mut self,
        offset: u32,
        value: ResolvedValue,
    ) -> Result<(), ProtocolError> {
        self.u32(offset)?;
        self.u8(value.kind() as u8)?;
        match value {
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
        }
        Ok(())
    }
}

/// Encode owned results, rejecting oversized or unsupported response data.
pub fn encode_response(response: &Response) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = Vec::new();
    encode_response_into(response, &mut bytes)?;
    Ok(bytes)
}

/// Encode into exclusive caller-owned storage, retaining capacity even on failure.
/// Failed encodes leave an empty buffer; no partial message can be published.
pub fn encode_response_into(response: &Response, bytes: &mut Vec<u8>) -> Result<(), ProtocolError> {
    bytes.clear();
    let mut writer = Writer(std::mem::take(bytes));
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
            | ResponseBody::CameraStateChangedEvent(_)
            | ResponseBody::Frame { .. }
            | ResponseBody::Lifecycle { .. }
            | ResponseBody::Resources { .. }
    ) || {
        #[cfg(feature = "gui")]
        {
            matches!(
                response.body,
                ResponseBody::GuiObservations { .. } | ResponseBody::GuiUnhandledInputs { .. }
            )
        }
        #[cfg(not(feature = "gui"))]
        {
            false
        }
    };
    if unsolicited != (response.request_id == 0) {
        return Err(ProtocolError::Malformed("reserved response identity"));
    }

    w.u64(response.session)?;
    w.u64(response.request_id)?;
    w.u64(response.tick)?;
    match &response.body {
        #[cfg(feature = "surfaces")]
        ResponseBody::SurfaceCommand => w.u8(RESPONSE_SURFACE)?,
        #[cfg(feature = "gui")]
        ResponseBody::GuiCommands {
            applied,
            error,
        } => {
            w.u8(RESPONSE_GUI)?;
            w.u32(*applied)?;
            w.u8(u8::from(error.is_some()))?;
            if let Some(error) = error {
                w.string(error_name(*error))?;
            }
        }
        #[cfg(feature = "gui")]
        ResponseBody::GuiInspect(response) => {
            w.u8(RESPONSE_GUI_INSPECT)?;
            w.gui_inspect_response(response)?;
        }
        #[cfg(feature = "gui")]
        ResponseBody::GuiInput {
            tick,
            unhandled,
        } => {
            w.u8(RESPONSE_GUI_INPUT)?;
            w.u64(*tick)?;
            let (reason, blocker) = match unhandled {
                None => (0, None),
                Some(ipp_core::GuiUnhandledReason::NoPanelHit) => (1, None),
                Some(ipp_core::GuiUnhandledReason::Blocked {
                    entity,
                }) => (2, Some(entity.to_bits())),
                Some(ipp_core::GuiUnhandledReason::StaleTarget) => (3, None),
                Some(ipp_core::GuiUnhandledReason::NoFocus) => (4, None),
                Some(ipp_core::GuiUnhandledReason::NoCapture) => (5, None),
                Some(ipp_core::GuiUnhandledReason::NotFocusable) => (6, None),
                Some(ipp_core::GuiUnhandledReason::NotOwner) => (7, None),
            };
            w.u16(reason)?;
            w.u8(u8::from(blocker.is_some()))?;
            if let Some(entity) = blocker {
                w.u64(entity)?;
            }
        }
        #[cfg(feature = "gui")]
        ResponseBody::GuiObservations {
            effects,
            conflicts,
            cancellations,
            text_focus_updates,
        } => {
            w.u8(RESPONSE_GUI_OBSERVATIONS)?;
            let inner = crate::observations::encode_gui_observations_inner(
                effects,
                conflicts,
                cancellations,
                text_focus_updates,
            )?;
            w.count(inner.len(), MAX_MESSAGE_BYTES)?;
            w.raw(&inner)?;
        }
        #[cfg(feature = "gui")]
        ResponseBody::GuiUnhandledInputs {
            inputs,
        } => {
            w.u8(RESPONSE_GUI_UNHANDLED)?;
            let inner = crate::observations::encode_gui_unhandled_inner(inputs)?;
            w.count(inner.len(), MAX_MESSAGE_BYTES)?;
            w.raw(&inner)?;
        }
        #[cfg(feature = "gui")]
        ResponseBody::GuiSemanticSnapshot(tree) => {
            w.u8(RESPONSE_GUI_SEMANTIC_SNAPSHOT)?;
            w.gui_semantic_snapshot_response(tree)?;
        }
        ResponseBody::RuntimeFailure {
            scope,
            faulted,
            message,
        } => {
            if message.len() > 2048 {
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
            w.count(events.len(), 1024)?;
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
        ResponseBody::BatchFinished(id) => {
            w.u8(RESPONSE_BATCH_FINISHED)?;
            w.u64(*id)?;
        }
        ResponseBody::BatchStarted(id) => {
            w.u8(RESPONSE_BATCH_STARTED)?;
            w.u64(*id)?;
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
            w.outcome(outcome)?;
        }
        ResponseBody::CameraStateChangedEvent(change) => {
            if change.tick != response.tick {
                return Err(ProtocolError::Malformed("camera state change tick"));
            }
            let camera = change
                .changes
                .active_camera
                .ok_or(ProtocolError::Malformed("empty camera state change"))?;
            w.u8(RESPONSE_CAMERA_STATE_CHANGED)?;
            w.u16(1)?;
            w.u64(camera.to_bits())?;
        }
        ResponseBody::GeometryPickResultEvent(outcome) => {
            if outcome.request_id != response.request_id || outcome.tick != response.tick {
                return Err(ProtocolError::Malformed("geometry outcome correlation"));
            }
            w.u8(RESPONSE_GEOMETRY_PICK)?;
            w.u8(if outcome.camera.is_some() {
                OPTION_SOME
            } else {
                OPTION_NONE
            })?;
            if let Some(camera) = outcome.camera {
                w.u64(camera.to_bits())?;
            }
            match &outcome.result {
                Ok(None) => {
                    if outcome.camera.is_none() {
                        return Err(ProtocolError::Malformed("geometry miss requires camera"));
                    }
                    w.u8(PICK_OUTCOME_MISS)?;
                }
                Ok(Some(hit)) => {
                    if outcome.camera.is_none() || hit.distance < 0.0 {
                        return Err(ProtocolError::Malformed("invalid geometry hit"));
                    }
                    w.u8(PICK_OUTCOME_HIT)?;
                    w.u64(hit.entity.to_bits())?;
                    for coordinate in hit.position {
                        w.f32(coordinate)?;
                    }
                    w.f32(hit.distance)?;
                    w.u32(hit.part)?;
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
            if outcome.result.is_ok() && outcome.camera.is_none() {
                return Err(ProtocolError::Malformed(
                    "camera projection requires camera",
                ));
            }
            w.u8(RESPONSE_CAMERA_PROJECT)?;
            w.u8(if outcome.camera.is_some() {
                OPTION_SOME
            } else {
                OPTION_NONE
            })?;
            if let Some(camera) = outcome.camera {
                w.u64(camera.to_bits())?;
            }
            w.u8(u8::from(outcome.result.is_ok()))?;
            if let Ok(Some(position)) = outcome.result {
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
            w.count(resources.len(), 128)?;
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
        ResponseBody::Lifecycle {
            diagnostics,
        } => {
            w.u8(RESPONSE_STATE_OVERLAY_LIFECYCLE)?;
            w.count(diagnostics.len(), 16384)?;
            for diagnostic in diagnostics {
                w.u64(diagnostic.owner)?;
                w.u64(diagnostic.state_overlay)?;
                w.u64(diagnostic.entity.to_bits())?;
                w.u8(if diagnostic.component.is_some() {
                    OPTION_SOME
                } else {
                    OPTION_NONE
                })?;
                if let Some(component) = diagnostic.component {
                    w.u16(component)?;
                }
                w.u8(match diagnostic.reason {
                    ipp_core::StateOverlayLifecycleReason::EntityDeleted => {
                        STATE_OVERLAY_ENTITY_DELETED
                    }
                    ipp_core::StateOverlayLifecycleReason::ComponentReplaced => {
                        STATE_OVERLAY_COMPONENT_REPLACED
                    }
                    ipp_core::StateOverlayLifecycleReason::ComponentRemoved => {
                        STATE_OVERLAY_COMPONENT_REMOVED
                    }
                })?;
            }
        }
        ResponseBody::Inspect {
            next,
            controllers,
            time,
            entities,
            resources,
            render_diagnostics,
        } => {
            w.u8(RESPONSE_INSPECT)?;
            w.f64(*time)?;
            w.u64(*next)?;
            w.count(entities.len(), 256)?;
            for entity in entities {
                w.u64(entity.id.to_bits())?;
                w.metadata(&entity.metadata)?;
                for components in [&entity.base, &entity.effective] {
                    w.count(components.len(), 256)?;
                    for component in components {
                        w.component(component)?;
                    }
                }
            }
            {
                w.count(resources.len(), 256)?;
                for resource in resources {
                    w.resource(resource)?;
                }
            }
            {
                w.count(render_diagnostics.len(), 256)?;
                for diagnostic in render_diagnostics {
                    w.u64(diagnostic.entity.to_bits())?;
                    w.string(error_name(diagnostic.reason))?;
                }
            }
            {
                w.count(controllers.len(), 256)?;
                for controller in controllers {
                    w.controller(controller)?;
                }
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
