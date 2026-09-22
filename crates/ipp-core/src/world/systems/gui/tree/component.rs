use super::controls::{GuiControlState, GuiControls};
use super::nodes::{GuiNodeId, GuiNodePatch, GuiNodeStyle, GuiNodes};
use crate::components::schema::ComponentLifecycle;
use crate::{DynamicProperties, DynamicValue, ErrorReason};
use ipp_schema_derive::SchemaComponent;

use crate::DynamicPropertyKind as Kind;

/// Node style lanes and their exact storage types.
const GUI_NODE_PROPERTIES: [(&str, Kind); 19] = [
    ("enabled", Kind::Bool),
    ("width", Kind::F32),
    ("height", Kind::F32),
    ("min_width", Kind::F32),
    ("min_height", Kind::F32),
    ("max_width", Kind::F32),
    ("max_height", Kind::F32),
    ("flex", Kind::F32),
    ("align_x", Kind::F32),
    ("align_y", Kind::F32),
    ("color", Kind::Vec4),
    ("background_color", Kind::Vec4),
    ("opacity", Kind::F32),
    ("font_size", Kind::F32),
    ("asset", Kind::Asset),
    ("position", Kind::Vec2),
    ("scale", Kind::Vec2),
    ("padding", Kind::Vec4),
    ("margin", Kind::Vec4),
];

/// Named skin-part lanes and their exact storage types.
/// Ordered by descending suffix length so longer composite suffixes match first.
pub(crate) const GUI_PART_PROPERTIES: [(&str, Kind); 22] = [
    ("gradient_color0", Kind::Vec4),
    ("gradient_color1", Kind::Vec4),
    ("gradient_radius", Kind::F32),
    ("glow_intensity", Kind::F32),
    ("gradient_start", Kind::Vec2),
    ("corner_radius", Kind::Vec2),
    ("gradient_end", Kind::Vec2),
    ("border_color", Kind::Vec4),
    ("border_width", Kind::F32),
    ("glow_falloff", Kind::F32),
    ("glow_radius", Kind::F32),
    ("glow_color", Kind::Vec4),
    ("fill_mode", Kind::F32),
    ("duration", Kind::F32),
    ("opacity", Kind::F32),
    ("easing", Kind::F32),
    ("motion", Kind::Asset),
    ("color", Kind::Vec4),
    ("scale", Kind::Vec2),
    ("asset", Kind::Asset),
    ("track", Kind::F32),
    ("time", Kind::F32),
];

/// Root GUI component owning the content of its entity's Surface.
///
/// The node tree, including committed control values, changes only through
/// GuiCommand while a root incarnation is live; a new incarnation may supply
/// it. Node style is stored once, as `node_<id>_<lane>` named properties.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiRoot {
    /// Authoritative root-local node tree and committed control values.
    nodes: GuiNodes,
    /// Authoritative style, layout and named-part properties.
    #[schema(ignore)]
    pub properties: DynamicProperties,
}

impl GuiRoot {
    pub(in crate::world::systems::gui) const fn nodes_field() -> u32 {
        std::mem::offset_of!(Self, nodes) as u32
    }

    /// Read-only access to root-local nodes.
    pub fn nodes(&self) -> &GuiNodes {
        &self.nodes
    }

    pub(in crate::world::systems::gui) fn nodes_mut(&mut self) -> &mut GuiNodes {
        &mut self.nodes
    }

    /// Committed control values.
    pub fn controls(&self) -> &GuiControls {
        &self.nodes.controls
    }

    pub(in crate::world::systems::gui) fn controls_mut(&mut self) -> &mut GuiControls {
        &mut self.nodes.controls
    }

    /// Number of live nodes in this tree.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Identity a caller must use for the next node insertion.
    pub fn next_node_id(&self) -> u32 {
        self.nodes.next_node_id()
    }

    /// Committed control state for one node; None for non-control nodes.
    pub fn control_state(&self, id: GuiNodeId) -> Option<&GuiControlState> {
        self.nodes.controls.get(id)
    }

    /// Visual translation and axis-aligned scale for one node, read from
    /// the `position` and `scale` lanes. Missing lanes default to identity;
    /// layout rejects singular (zero) scales with a diagnostic instead of
    /// flowing them into paint or hit testing. These lanes never reflow:
    /// they move paint and hit regions together.
    pub fn visual_transform(&self, id: GuiNodeId) -> ([f32; 2], [f32; 2]) {
        let lane = |suffix: &str, default: [f32; 2]| match self
            .properties
            .get(&node_property_name(id, suffix))
        {
            Some(DynamicValue::Vec2(value)) => value,
            _ => default,
        };
        (lane("position", [0.0, 0.0]), lane("scale", [1.0, 1.0]))
    }

    /// Copy authoritative effective style for one node.
    pub fn style(&self, id: GuiNodeId) -> Option<GuiNodeStyle> {
        self.nodes.node(id)?;
        let mut style = GuiNodeStyle::default();

        if let Some(DynamicValue::Bool(enabled)) =
            self.properties.get(&node_property_name(id, "enabled"))
        {
            style.enabled = enabled;
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "width")) {
            style.width = f32_value(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "height")) {
            style.height = f32_value(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "min_width")) {
            style.min_width = f32_value(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "min_height")) {
            style.min_height = f32_value(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "max_width")) {
            style.max_width = f32_value(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "max_height")) {
            style.max_height = f32_value(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "padding")) {
            style.padding = vec4(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "margin")) {
            style.margin = vec4(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "flex")) {
            style.flex = f32_value(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "align_x")) {
            style.align_x = f32_value(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "align_y")) {
            style.align_y = f32_value(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "color"))
            && let Some(c) = vec4(&val)
        {
            style.color = c;
        }
        if let Some(val) = self
            .properties
            .get(&node_property_name(id, "background_color"))
        {
            style.background_color = vec4(&val);
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "opacity"))
            && let Some(o) = f32_value(&val)
        {
            style.opacity = o;
        }
        if let Some(val) = self.properties.get(&node_property_name(id, "font_size"))
            && let Some(fs) = f32_value(&val)
        {
            style.font_size = fs;
        }
        if let Some(asset) = self.properties.asset(&node_property_name(id, "asset")) {
            style.asset = Some(asset.clone());
        }

        Some(style)
    }

    /// Apply partial style changes to named properties, preserving omitted fields.
    /// An explicit clear removes the property, invalidating its bindings.
    pub(in crate::world::systems::gui) fn apply_patch(
        &mut self,
        id: GuiNodeId,
        patch: &GuiNodePatch,
    ) -> Result<(), ErrorReason> {
        let f32_lane = |value: Option<Option<f32>>| value.map(|value| value.map(DynamicValue::F32));
        let vec4_lane =
            |value: Option<Option<[f32; 4]>>| value.map(|value| value.map(DynamicValue::Vec4));
        let lanes = [
            (
                "enabled",
                patch.enabled.map(|value| Some(DynamicValue::Bool(value))),
            ),
            ("width", f32_lane(patch.width)),
            ("height", f32_lane(patch.height)),
            ("min_width", f32_lane(patch.min_width)),
            ("min_height", f32_lane(patch.min_height)),
            ("max_width", f32_lane(patch.max_width)),
            ("max_height", f32_lane(patch.max_height)),
            ("padding", vec4_lane(patch.padding)),
            ("margin", vec4_lane(patch.margin)),
            ("flex", f32_lane(patch.flex)),
            ("align_x", f32_lane(patch.align_x)),
            ("align_y", f32_lane(patch.align_y)),
            (
                "color",
                patch.color.map(|value| Some(DynamicValue::Vec4(value))),
            ),
            ("background_color", vec4_lane(patch.background_color)),
            (
                "opacity",
                patch.opacity.map(|value| Some(DynamicValue::F32(value))),
            ),
            (
                "font_size",
                patch.font_size.map(|value| Some(DynamicValue::F32(value))),
            ),
            (
                "asset",
                patch
                    .asset
                    .clone()
                    .map(|value| value.map(DynamicValue::Asset)),
            ),
        ];
        for (suffix, change) in lanes {
            let Some(value) = change else {
                continue;
            };
            let name = node_property_name(id, suffix);
            match value {
                Some(value) => {
                    self.properties.set(&name, value).map_err(field_error)?;
                }
                None => {
                    self.properties.remove(&name);
                }
            }
        }
        Ok(())
    }

    /// Install a newly inserted node's complete style.
    pub(in crate::world::systems::gui) fn install_node_style(
        &mut self,
        id: GuiNodeId,
        style: &GuiNodeStyle,
    ) -> Result<(), ErrorReason> {
        self.apply_patch(
            id,
            &GuiNodePatch {
                content: None,
                enabled: Some(style.enabled),
                width: Some(style.width),
                height: Some(style.height),
                min_width: Some(style.min_width),
                min_height: Some(style.min_height),
                max_width: Some(style.max_width),
                max_height: Some(style.max_height),
                padding: Some(style.padding),
                margin: Some(style.margin),
                flex: Some(style.flex),
                align_x: Some(style.align_x),
                align_y: Some(style.align_y),
                color: Some(style.color),
                background_color: Some(style.background_color),
                opacity: Some(style.opacity),
                font_size: Some(style.font_size),
                asset: Some(style.asset.clone()),
            },
        )
    }

    /// Remove every property owned by a removed node or its named parts.
    pub(in crate::world::systems::gui) fn remove_node_properties(&mut self, id: GuiNodeId) {
        let prefix = format!("node_{}_", id.0);
        let owned: Vec<String> = self
            .properties
            .descriptors()
            .range(prefix.clone()..)
            .take_while(|(name, _)| name.starts_with(&prefix))
            .map(|(name, _)| name.clone())
            .collect();
        for name in owned {
            self.properties.remove(&name);
        }
    }

    /// Produce a validated named-property address for a node style lane.
    pub fn property_name(id: GuiNodeId, suffix: &str) -> Option<String> {
        lane_kind(&GUI_NODE_PROPERTIES, suffix).map(|_| node_property_name(id, suffix))
    }

    /// Produce a validated named-property address for a named skin part lane.
    pub fn part_property_name(id: GuiNodeId, part: &str, suffix: &str) -> Option<String> {
        if valid_part_name(part) && lane_kind(&GUI_PART_PROPERTIES, suffix).is_some() {
            Some(format!("node_{}_part_{}_{}", id.0, part, suffix))
        } else {
            None
        }
    }

    /// Whether a canonical named property belongs to a node removed from this root.
    pub(in crate::world) fn is_removed_node_property(&self, name: &str) -> bool {
        property_lane(name).is_some_and(|(id, _, _)| self.nodes.node(id).is_none())
    }

    /// Every named property is a GUI lane with its declared type and range.
    pub(in crate::world::systems::gui) fn validate_properties(&self) -> Result<(), ErrorReason> {
        for name in self.properties.descriptors().keys() {
            let value = self.properties.get(name).ok_or(ErrorReason::InvalidField)?;
            validate_property_value(name, &value)?;
        }
        Ok(())
    }

    pub(in crate::world) fn validate_complete(&self) -> Result<(), ErrorReason> {
        <Self as ComponentLifecycle>::validate(self)
    }
}

impl ComponentLifecycle for GuiRoot {
    fn required_components() -> &'static [u16] {
        &[crate::ComponentValue::SURFACE]
    }

    fn supports_numeric_property(offset: u32) -> bool {
        crate::components::dynamic_properties::is_dynamic_field(offset)
            && offset != crate::components::dynamic_properties::DYNAMIC_METADATA
    }

    fn validate_numeric_properties(
        &self,
        fields: &[(u32, crate::components::schema::FieldValue)],
    ) -> Result<(), ErrorReason> {
        // Numeric writes cannot change structure, so only the written lanes need checks.
        for (offset, field) in fields {
            let crate::components::schema::FieldValue::Dynamic(value) = field else {
                return Err(ErrorReason::InvalidField);
            };
            if value.kind() == crate::DynamicPropertyKind::Asset
                || self
                    .properties
                    .get_key(*offset)
                    .is_none_or(|old| old.kind() != value.kind())
            {
                return Err(ErrorReason::InvalidField);
            }
            let name = self
                .properties
                .descriptors()
                .iter()
                .find_map(|(name, descriptor)| (descriptor.key == *offset).then_some(name.as_str()))
                .ok_or(ErrorReason::InvalidField)?;
            validate_property_value(name, value)?;
        }
        Ok(())
    }

    fn supports_dynamic_properties() -> bool {
        true
    }

    fn dynamic_properties(&self) -> Option<&DynamicProperties> {
        Some(&self.properties)
    }

    fn dynamic_properties_mut(&mut self) -> Option<&mut DynamicProperties> {
        Some(&mut self.properties)
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        self.nodes.validate().map_err(field_error)?;
        self.validate_properties()
    }

    /// Named lanes are checked wherever they are written: generic field writes
    /// and StateOverlay declarations reach this boundary before live state changes.
    fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
        if offset == crate::components::dynamic_properties::DYNAMIC_METADATA {
            return self.validate_properties();
        }
        if crate::components::dynamic_properties::is_dynamic_field(offset) {
            let (name, _) = self
                .properties
                .descriptors()
                .iter()
                .find(|(_, descriptor)| descriptor.key == offset)
                .ok_or(ErrorReason::InvalidField)?;
            let value = self.properties.get(name).ok_or(ErrorReason::InvalidField)?;
            return validate_property_value(name, &value);
        }
        self.validate()
    }

    fn resource_demand(
        &self,
        demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::service::AssetDemandSelection,
        >,
    ) {
        self.properties.resource_demand(demand);
    }
}

fn node_property_name(id: GuiNodeId, suffix: &str) -> String {
    format!("node_{}_{}", id.0, suffix)
}

fn field_error(_: crate::components::schema::FieldError) -> ErrorReason {
    ErrorReason::InvalidValue
}

fn f32_value(value: &DynamicValue) -> Option<f32> {
    match value {
        DynamicValue::F32(v) => Some(*v),
        _ => None,
    }
}

fn vec4(value: &DynamicValue) -> Option<[f32; 4]> {
    match value {
        DynamicValue::Vec4(v) => Some(*v),
        _ => None,
    }
}

/// Parse `node_<id>_<lane>` or `node_<id>_part_<part>_<lane>` into its node, lane and type.
fn property_lane(name: &str) -> Option<(GuiNodeId, &str, Kind)> {
    let (id, lane) = name.strip_prefix("node_")?.split_once('_')?;
    if id.starts_with('0') {
        return None;
    }
    let id = GuiNodeId(id.parse().ok()?);
    match lane.strip_prefix("part_") {
        Some(part_and_suffix) => {
            for &(suffix, kind) in &GUI_PART_PROPERTIES {
                if let Some(part) = part_and_suffix.strip_suffix(suffix)
                    && let Some(part) = part.strip_suffix('_')
                    && valid_part_name(part)
                {
                    return Some((id, suffix, kind));
                }
            }
            None
        }
        None => Some((id, lane, lane_kind(&GUI_NODE_PROPERTIES, lane)?)),
    }
}

fn lane_kind(lanes: &[(&str, Kind)], suffix: &str) -> Option<Kind> {
    lanes
        .iter()
        .find_map(|(lane, kind)| (*lane == suffix).then_some(*kind))
}

fn valid_part_name(part: &str) -> bool {
    !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Convert an authored F32 base track only when all three skin lanes fit.
pub(in crate::world::systems::gui) fn skin_motion_base_track(value: f32) -> Option<u32> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    let base = u32::try_from(value as u64).ok()?;
    base.checked_add(2)?;
    Some(base)
}

/// Accept only GUI-addressed names with their exact type and value range.
pub(crate) fn validate_property_value(name: &str, value: &DynamicValue) -> Result<(), ErrorReason> {
    let (_, lane, kind) = property_lane(name).ok_or(ErrorReason::InvalidField)?;
    if value.kind() != kind {
        return Err(ErrorReason::InvalidField);
    }
    value.validate().map_err(field_error)?;
    let valid = match value {
        DynamicValue::F32(value) => {
            value.is_finite()
                && match lane {
                    "opacity" => (0.0..=1.0).contains(value),
                    "font_size" => *value > 0.0,
                    "duration" | "time" => *value >= 0.0,
                    "easing" => matches!(*value, 0.0 | 1.0),
                    "track" => skin_motion_base_track(*value).is_some(),
                    "width" | "height" | "min_width" | "min_height" | "max_width"
                    | "max_height" | "flex" => *value >= 0.0,
                    "border_width" | "gradient_radius" | "glow_intensity" | "glow_radius"
                    | "glow_falloff" => *value >= 0.0,
                    "fill_mode" => matches!(*value, 0.0 | 1.0 | 2.0),
                    _ => true,
                }
        }
        DynamicValue::Vec2(value) => {
            value.iter().all(|v| v.is_finite())
                && match lane {
                    "corner_radius" => value.iter().all(|v| *v >= 0.0),
                    _ => true,
                }
        }
        DynamicValue::Vec4(value) => {
            value.iter().all(|value| value.is_finite())
                && match lane {
                    "color" | "background_color" | "border_color" | "gradient_color0"
                    | "gradient_color1" | "glow_color" => {
                        value.iter().all(|value| (0.0..=1.0).contains(value))
                    }
                    "padding" => value.iter().all(|value| *value >= 0.0),
                    _ => true,
                }
        }
        DynamicValue::Bool(_) => true,
        DynamicValue::Asset(_) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}
