use super::items::SurfaceItems;
use super::{SurfaceItem, SurfaceItemContent, SurfaceItemId, SurfaceItemPatch, SurfaceItemStyle};
use crate::{DynamicProperties, DynamicValue, ErrorReason, components::schema::ComponentLifecycle};
use ipp_schema_derive::SchemaComponent;

const PROPERTY_SUFFIXES: [&str; 6] = [
    "position",
    "scale",
    "color",
    "opacity",
    "font_size",
    "asset",
];

/// Ordered, clipped local-XY presentation attached to one entity.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct Surface {
    /// Centred clipping width in metres.
    pub width: f32,
    /// Centred clipping height in metres.
    pub height: f32,
    /// Stable typed content in explicit painter order.
    items: SurfaceItems,
    /// Sole effective item style and typed asset storage.
    #[schema(ignore)]
    pub properties: DynamicProperties,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            items: Default::default(),
            properties: Default::default(),
        }
    }
}

impl Surface {
    pub(in crate::world) const fn items_field() -> u32 {
        std::mem::offset_of!(Self, items) as u32
    }

    pub(super) fn validate_item_patch(
        &self,
        id: SurfaceItemId,
        patch: &SurfaceItemPatch,
    ) -> Result<(), ErrorReason> {
        let item = self
            .items
            .values
            .iter()
            .find(|item| item.id == id)
            .ok_or(ErrorReason::MissingComponent)?;
        if let Some(content) = &patch.content {
            super::items::validate_content(content).map_err(field_error)?;
        }
        let mut style = self.style(id).ok_or(ErrorReason::InvalidField)?;
        if let Some(value) = patch.position {
            style.position = value;
        }
        if let Some(value) = patch.scale {
            style.scale = value;
        }
        if let Some(value) = patch.color {
            style.color = value;
        }
        if let Some(value) = patch.opacity {
            style.opacity = value;
        }
        if let Some(value) = patch.font_size {
            style.font_size = value;
        }
        if let Some(value) = &patch.asset {
            style.asset = value.clone();
        }
        validate_style(&style)?;
        validate_content_asset(
            patch.content.as_ref().unwrap_or(&item.content),
            style.asset.as_ref(),
        )
    }

    /// Identity a caller must use for the next insertion.
    pub fn next_item_id(&self) -> u32 {
        self.items.next_id
    }

    /// Items in explicit painter order.
    pub fn items(&self) -> &[SurfaceItem] {
        self.items.as_slice()
    }

    /// Inspect one stable item and its authoritative effective style.
    pub fn item(&self, id: SurfaceItemId) -> Option<(&SurfaceItem, SurfaceItemStyle)> {
        let item = self.items().iter().find(|item| item.id == id)?;
        Some((item, self.style(id)?))
    }

    /// Insert one typed item, allocating its never-reused identity.
    pub fn insert_item(
        &mut self,
        index: usize,
        content: SurfaceItemContent,
        style: SurfaceItemStyle,
    ) -> Result<SurfaceItemId, ErrorReason> {
        if index > self.items.values.len() {
            return Err(ErrorReason::InvalidValue);
        }
        super::items::SurfaceItems {
            values: vec![SurfaceItem {
                id: SurfaceItemId(1),
                content: content.clone(),
            }],
            next_id: 2,
        }
        .validate()
        .map_err(field_error)?;
        validate_style(&style)?;
        validate_content_asset(&content, style.asset.as_ref())?;
        let id = SurfaceItemId(self.items.next_id);
        self.items.next_id = self
            .items
            .next_id
            .checked_add(1)
            .filter(|id| *id != 0)
            .ok_or(ErrorReason::Capacity)?;
        self.install_style(id, style)?;
        self.items.as_mut_vec().insert(
            index,
            SurfaceItem {
                id,
                content,
            },
        );
        Ok(id)
    }

    /// Apply a partial content or style edit without changing identity.
    pub fn update_item(
        &mut self,
        id: SurfaceItemId,
        patch: SurfaceItemPatch,
    ) -> Result<(), ErrorReason> {
        let index = self
            .items
            .values
            .iter()
            .position(|item| item.id == id)
            .ok_or(ErrorReason::MissingComponent)?;
        let mut style = self.style(id).ok_or(ErrorReason::InvalidField)?;
        if let Some(value) = patch.position {
            style.position = value;
        }
        if let Some(value) = patch.scale {
            style.scale = value;
        }
        if let Some(value) = patch.color {
            style.color = value;
        }
        if let Some(value) = patch.opacity {
            style.opacity = value;
        }
        if let Some(value) = patch.font_size {
            style.font_size = value;
        }
        if let Some(value) = patch.asset {
            style.asset = value;
        }
        validate_style(&style)?;
        let next_content = patch
            .content
            .as_ref()
            .unwrap_or(&self.items.values[index].content);
        validate_content_asset(next_content, style.asset.as_ref())?;
        if let Some(content) = patch.content {
            super::items::validate_content(&content).map_err(field_error)?;
            replace_content(&mut self.items.values[index].content, content);
        }
        self.install_style(id, style)
    }

    /// Remove one item and all its named property identities.
    pub fn remove_item(&mut self, id: SurfaceItemId) -> Result<SurfaceItem, ErrorReason> {
        let index = self
            .items
            .values
            .iter()
            .position(|item| item.id == id)
            .ok_or(ErrorReason::MissingComponent)?;
        // Removing declarations first invalidates their lifetime keys before collection identity can disappear.
        for suffix in PROPERTY_SUFFIXES {
            self.properties.remove(&property_name(id, suffix));
        }
        Ok(self.items.values.remove(index))
    }

    /// Move one item in painter order without changing its property bindings.
    pub fn move_item(&mut self, id: SurfaceItemId, index: usize) -> Result<(), ErrorReason> {
        if index >= self.items.values.len() {
            return Err(ErrorReason::InvalidValue);
        }
        let previous = self
            .items
            .values
            .iter()
            .position(|item| item.id == id)
            .ok_or(ErrorReason::MissingComponent)?;
        let item = self.items.values.remove(previous);
        self.items.values.insert(index, item);
        Ok(())
    }

    pub(in crate::world) fn validate_complete(&self) -> Result<(), ErrorReason> {
        <Self as ComponentLifecycle>::validate(self)
    }

    /// Copy the authoritative effective style for one item.
    pub fn style(&self, id: SurfaceItemId) -> Option<SurfaceItemStyle> {
        Some(SurfaceItemStyle {
            position: vec2(self.properties.get(&property_name(id, "position"))?)?,
            scale: vec2(self.properties.get(&property_name(id, "scale"))?)?,
            color: vec4(self.properties.get(&property_name(id, "color"))?)?,
            opacity: f32_value(self.properties.get(&property_name(id, "opacity"))?)?,
            font_size: f32_value(self.properties.get(&property_name(id, "font_size"))?)?,
            asset: self.properties.asset(&property_name(id, "asset")).cloned(),
        })
    }

    /// Produce a validated named-property address for an item style lane.
    pub fn property_name(id: SurfaceItemId, suffix: &str) -> Option<String> {
        PROPERTY_SUFFIXES
            .contains(&suffix)
            .then(|| property_name(id, suffix))
    }

    /// Whether a canonical named property belongs to an item removed from the collection.
    pub(in crate::world) fn is_removed_item_property(&self, name: &str) -> bool {
        let Some(rest) = name.strip_prefix("item_") else {
            return false;
        };
        let Some((id, suffix)) = rest.split_once('_') else {
            return false;
        };
        let Ok(id) = id.parse::<u32>() else {
            return false;
        };
        id != 0
            && PROPERTY_SUFFIXES.contains(&suffix)
            && name == property_name(SurfaceItemId(id), suffix)
            && !self.items.values.iter().any(|item| item.id.0 == id)
    }

    /// Conservative local-space enclosure used by headless geometry consumers.
    pub fn local_bounding_geometry(&self) -> crate::systems::geometry::GeometryShape {
        crate::systems::geometry::GeometryShape::Box {
            min: [-(self.width as f64) * 0.5, -(self.height as f64) * 0.5, 0.0],
            max: [(self.width as f64) * 0.5, (self.height as f64) * 0.5, 0.0],
        }
    }

    /// Map a 2D point in Surface content coordinates ([0, width] x [0, height], +X right, +Y down)
    /// to centred entity-local 3D coordinates (+X right, +Y up, front +Z).
    #[inline]
    pub fn content_to_entity_local(&self, x: f32, y: f32) -> [f32; 3] {
        [x - self.width * 0.5, self.height * 0.5 - y, 0.0]
    }

    /// Map a 2D point in centred entity-local coordinates to 2D Surface content coordinates.
    #[inline]
    pub fn entity_local_to_content(&self, entity_x: f32, entity_y: f32) -> [f32; 2] {
        [entity_x + self.width * 0.5, self.height * 0.5 - entity_y]
    }

    /// Whether a point in Surface content coordinates falls within the content bounds [0, width] x [0, height].
    #[inline]
    pub fn contains_content_point(&self, point: [f32; 2]) -> bool {
        point[0] >= 0.0 && point[0] <= self.width && point[1] >= 0.0 && point[1] <= self.height
    }

    /// Map an entity-local XY plane hit to Surface content coordinates if it falls within the surface bounds.
    #[inline]
    pub fn plane_hit_to_content(&self, entity_x: f32, entity_y: f32) -> Option<[f32; 2]> {
        let content = self.entity_local_to_content(entity_x, entity_y);
        self.contains_content_point(content).then_some(content)
    }

    /// The 2D content rectangle bounds [min_x, min_y, max_x, max_y] in content coordinates.
    #[inline]
    pub fn content_bounds(&self) -> [f32; 4] {
        [0.0, 0.0, self.width, self.height]
    }

    fn install_style(
        &mut self,
        id: SurfaceItemId,
        style: SurfaceItemStyle,
    ) -> Result<(), ErrorReason> {
        let values = [
            ("position", DynamicValue::Vec2(style.position)),
            ("scale", DynamicValue::Vec2(style.scale)),
            ("color", DynamicValue::Vec4(style.color)),
            ("opacity", DynamicValue::F32(style.opacity)),
            ("font_size", DynamicValue::F32(style.font_size)),
        ];
        for (suffix, value) in values {
            self.properties
                .set(&property_name(id, suffix), value)
                .map_err(field_error)?;
        }
        let asset_name = property_name(id, "asset");
        if let Some(asset) = style.asset {
            self.properties
                .set(&asset_name, DynamicValue::Asset(asset))
                .map_err(field_error)?;
        } else {
            self.properties.remove(&asset_name);
        }
        Ok(())
    }
}

fn replace_content(current: &mut SurfaceItemContent, next: SurfaceItemContent) {
    match (current, next) {
        (SurfaceItemContent::Label(current), SurfaceItemContent::Label(next)) => {
            current.clear();
            current.push_str(&next);
        }
        (SurfaceItemContent::GlyphRun(current), SurfaceItemContent::GlyphRun(next)) => {
            current.clear();
            current.extend(next);
        }
        (current, next) => *current = next,
    }
}

impl ComponentLifecycle for Surface {
    fn required_components() -> &'static [u16] {
        &[
            crate::ComponentValue::TRANSFORM,
            crate::ComponentValue::BOUNDING_GEOMETRY,
        ]
    }

    fn supports_numeric_property(offset: u32) -> bool {
        crate::components::dynamic_properties::is_dynamic_field(offset)
            && offset != crate::components::dynamic_properties::DYNAMIC_METADATA
    }

    fn validate_numeric_properties(
        &self,
        fields: &[(u32, crate::components::schema::FieldValue)],
    ) -> Result<(), ErrorReason> {
        self.validate()?;
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
            value.validate().map_err(field_error)?;
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
        if !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return Err(ErrorReason::InvalidValue);
        }
        self.items.validate().map_err(field_error)?;
        for item in &self.items.values {
            let Some(style) = self.style(item.id) else {
                // Generated and overlay authoring may install structural and named
                // fields as separate ordered operations. Incomplete items remain
                // non-rendering until their authoritative properties are present.
                continue;
            };
            validate_content_asset(&item.content, style.asset.as_ref())?;
        }
        Ok(())
    }

    fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
        if crate::components::dynamic_properties::is_dynamic_field(offset) {
            return Ok(());
        }
        if offset == std::mem::offset_of!(Self, width) as u32 {
            return (self.width.is_finite() && self.width > 0.0)
                .then_some(())
                .ok_or(ErrorReason::InvalidValue);
        }
        if offset == std::mem::offset_of!(Self, height) as u32 {
            return (self.height.is_finite() && self.height > 0.0)
                .then_some(())
                .ok_or(ErrorReason::InvalidValue);
        }
        self.items.validate().map_err(field_error)
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

fn property_name(id: SurfaceItemId, suffix: &str) -> String {
    format!("item_{}_{}", id.0, suffix)
}

fn validate_style(style: &SurfaceItemStyle) -> Result<(), ErrorReason> {
    if style
        .position
        .iter()
        .chain(&style.scale)
        .chain(&style.color)
        .chain([&style.opacity, &style.font_size])
        .any(|v| !v.is_finite())
        || style.scale.iter().any(|v| *v < 0.0)
        || style.color.iter().any(|v| !(0.0..=1.0).contains(v))
        || !(0.0..=1.0).contains(&style.opacity)
        || style.font_size <= 0.0
    {
        return Err(ErrorReason::InvalidValue);
    }
    if let Some(asset) = &style.asset {
        DynamicValue::Asset(asset.clone())
            .validate()
            .map_err(field_error)?;
    }
    Ok(())
}

fn validate_content_asset(
    content: &SurfaceItemContent,
    asset: Option<&crate::services::asset_management::AssetSource>,
) -> Result<(), ErrorReason> {
    let Some(asset) = asset else {
        return Ok(());
    };
    let expected = match content {
        SurfaceItemContent::Label(_) | SurfaceItemContent::GlyphRun(_) => 17,
        SurfaceItemContent::Drawing => 18,
        SurfaceItemContent::Bitmap {
            ..
        } => 2,
    };
    (asset.kind.0 == expected)
        .then_some(())
        .ok_or(ErrorReason::InvalidValue)
}

pub(crate) fn validate_property_value(name: &str, value: &DynamicValue) -> Result<(), ErrorReason> {
    let valid = if name.ends_with("_position") {
        matches!(value, DynamicValue::Vec2(v) if v.iter().all(|x| x.is_finite()))
    } else if name.ends_with("_scale") {
        matches!(value, DynamicValue::Vec2(v) if v.iter().all(|x| x.is_finite() && *x >= 0.0))
    } else if name.ends_with("_color") {
        matches!(value, DynamicValue::Vec4(v) if v.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x)))
    } else if name.ends_with("_opacity") {
        matches!(value, DynamicValue::F32(v) if v.is_finite() && (0.0..=1.0).contains(v))
    } else if name.ends_with("_font_size") {
        matches!(value, DynamicValue::F32(v) if v.is_finite() && *v > 0.0)
    } else {
        false
    };
    valid.then_some(()).ok_or(ErrorReason::InvalidValue)
}

fn field_error(error: crate::components::schema::FieldError) -> ErrorReason {
    match error {
        crate::components::schema::FieldError::NonFinite => ErrorReason::InvalidValue,
        _ => ErrorReason::InvalidField,
    }
}

fn vec2(value: DynamicValue) -> Option<[f32; 2]> {
    if let DynamicValue::Vec2(v) = value {
        Some(v)
    } else {
        None
    }
}

fn vec4(value: DynamicValue) -> Option<[f32; 4]> {
    if let DynamicValue::Vec4(v) = value {
        Some(v)
    } else {
        None
    }
}

fn f32_value(value: DynamicValue) -> Option<f32> {
    if let DynamicValue::F32(v) = value {
        Some(v)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PositionedGlyph;
    use crate::components::schema::SchemaComponent;

    #[test]
    fn reorder_retains_property_keys_and_removal_never_reuses_identity() {
        let mut surface = Surface::default();
        let first = surface
            .insert_item(0, SurfaceItemContent::Label("a".into()), Default::default())
            .unwrap();
        let second = surface
            .insert_item(1, SurfaceItemContent::Drawing, Default::default())
            .unwrap();
        let key = surface
            .properties
            .key(&property_name(first, "position"))
            .unwrap();
        surface.move_item(first, 1).unwrap();
        assert_eq!(
            surface
                .items()
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            [second, first]
        );
        assert_eq!(
            surface.properties.key(&property_name(first, "position")),
            Some(key)
        );
        surface.remove_item(first).unwrap();
        let replacement = surface
            .insert_item(1, SurfaceItemContent::Drawing, Default::default())
            .unwrap();
        assert!(replacement.0 > first.0);
        assert_ne!(
            surface
                .properties
                .key(&property_name(replacement, "position")),
            Some(key)
        );
    }

    #[test]
    fn removed_item_properties_exclude_live_and_arbitrary_names() {
        let mut surface = Surface::default();
        let item = surface
            .insert_item(0, SurfaceItemContent::Drawing, Default::default())
            .unwrap();
        for suffix in PROPERTY_SUFFIXES {
            assert!(!surface.is_removed_item_property(&property_name(item, suffix)));
        }
        surface.remove_item(item).unwrap();
        for suffix in PROPERTY_SUFFIXES {
            assert!(surface.is_removed_item_property(&property_name(item, suffix)));
        }
        for name in [
            "custom",
            "item_0_color",
            "item_01_color",
            "item_+1_color",
            "item_1_custom",
        ] {
            assert!(!surface.is_removed_item_property(name), "{name}");
        }
    }

    #[test]
    fn typed_collection_round_trips_through_the_schema_field() {
        let mut surface = Surface::default();
        surface
            .insert_item(
                0,
                SurfaceItemContent::GlyphRun(vec![PositionedGlyph {
                    glyph_id: 42,
                    position: [1.0, 2.0],
                    color: Some([0.1, 0.2, 0.3, 0.4]),
                }]),
                Default::default(),
            )
            .unwrap();
        let fields = surface.fields();
        let items_offset = std::mem::offset_of!(Surface, items) as u32;
        let encoded = fields
            .into_iter()
            .find(|(offset, _)| *offset == items_offset)
            .unwrap()
            .1;
        let mut restored = surface.clone();
        restored.items = Default::default();
        restored.set_field(items_offset, encoded).unwrap();
        assert_eq!(restored.items, surface.items);
    }

    #[test]
    fn glyph_colors_and_bitmap_sizes_use_physical_domains() {
        let mut surface = Surface::default();
        assert_eq!(
            surface.insert_item(
                0,
                SurfaceItemContent::GlyphRun(vec![PositionedGlyph {
                    glyph_id: 0,
                    position: [0.0; 2],
                    color: Some([-0.1, 0.0, 0.0, 1.0]),
                }]),
                Default::default(),
            ),
            Err(ErrorReason::InvalidValue)
        );
        assert_eq!(
            surface.insert_item(
                0,
                SurfaceItemContent::Bitmap {
                    size: [0.0, 1.0]
                },
                Default::default(),
            ),
            Err(ErrorReason::InvalidField)
        );
    }

    #[test]
    fn surface_coordinates_top_left_y_down_conversions() {
        let surface = Surface {
            width: 4.0,
            height: 2.0,
            ..Default::default()
        };

        // Origin (top-left) in content coords is (-w/2, +h/2) in entity local coords
        assert_eq!(surface.content_to_entity_local(0.0, 0.0), [-2.0, 1.0, 0.0]);
        assert_eq!(surface.entity_local_to_content(-2.0, 1.0), [0.0, 0.0]);

        // Bottom-right in content coords is (+w/2, -h/2) in entity local coords
        assert_eq!(surface.content_to_entity_local(4.0, 2.0), [2.0, -1.0, 0.0]);
        assert_eq!(surface.entity_local_to_content(2.0, -1.0), [4.0, 2.0]);

        // Center in content coords is (w/2, h/2) and (0, 0) in entity local coords
        assert_eq!(surface.content_to_entity_local(2.0, 1.0), [0.0, 0.0, 0.0]);
        assert_eq!(surface.entity_local_to_content(0.0, 0.0), [2.0, 1.0]);

        // Bounds and hit tests
        assert_eq!(surface.content_bounds(), [0.0, 0.0, 4.0, 2.0]);
        assert!(surface.contains_content_point([0.0, 0.0]));
        assert!(surface.contains_content_point([4.0, 2.0]));
        assert!(!surface.contains_content_point([-0.1, 1.0]));
        assert!(!surface.contains_content_point([4.1, 1.0]));
        assert!(!surface.contains_content_point([2.0, -0.1]));
        assert!(!surface.contains_content_point([2.0, 2.1]));

        assert_eq!(surface.plane_hit_to_content(0.0, 0.0), Some([2.0, 1.0]));
        assert_eq!(surface.plane_hit_to_content(-2.0, 1.0), Some([0.0, 0.0]));
        assert_eq!(surface.plane_hit_to_content(2.1, 0.0), None);
    }
}
