//! Target-executed layout and owned dispatch fixture.

use ipp_core::EntityId;
use ipp_core::components::rows::Rows;
use ipp_core::components::schema::{ContractSink, FieldValue, SchemaComponent};
use std::sync::Arc;

#[repr(C)]
#[derive(ipp_core::components::schema::SchemaComponent)]
struct LayoutFixture {
    marker: u32,
    #[schema(ignore)]
    internal: usize,
    value: f32,
    label: Arc<str>,
    bytes: Vec<u8>,
    source: EntityId,
    visible: bool,
}

impl Default for LayoutFixture {
    fn default() -> Self {
        Self {
            marker: 17,
            internal: 0,
            value: 1.25,
            label: "fixture".into(),
            bytes: vec![1, 2, 3],
            source: EntityId::from_bits(0x1_0000_0007),
            visible: true,
        }
    }
}

// A native subsystem can supply this authored value; binding requires no Default.
#[repr(C)]
#[derive(ipp_core::components::schema::SchemaComponent)]
#[schema(no_create)]
struct BoundFixture {
    label: Arc<str>,
    #[schema(ignore)]
    internal: usize,
}

/// Row properties cover a required scalar, a hinted optional Vec4, an asset and
/// bounded text.
#[derive(Debug, Default, PartialEq, ipp_core::components::rows::SchemaRow)]
struct FixtureRow {
    weight: f32,
    #[schema(rotation)]
    rotation: Option<[f32; 4]>,
    source: Option<ipp_core::services::asset_management::AssetSource>,
    #[schema(text = 8)]
    label: Option<Arc<str>>,
    flag: Option<bool>,
    count: Option<u32>,
    signed: Option<i32>,
    pair: Option<[f32; 2]>,
    triple: Option<[f32; 3]>,
}

fn rows_example() -> Rows<FixtureRow> {
    let mut rows = RowsFixture::default().rows;
    rows.insert(
        4,
        FixtureRow {
            weight: -2.5,
            rotation: Some([0.0, 0.0, 0.0, 1.0]),
            source: Some(ipp_core::services::asset_management::AssetSource {
                kind: ipp_core::TEXTURE_TYPE,
                uri: "memory:✓".into(),
                variant: 7,
            }),
            label: Some("é🙂".into()),
            flag: Some(true),
            count: Some(u32::MAX),
            signed: Some(i32::MIN),
            pair: Some([-0.0, 2.0]),
            triple: Some([1.0, 2.0, 3.0]),
        },
    )
    .unwrap();
    rows.insert(7, FixtureRow::default()).unwrap();
    rows.remove(7);
    let decoded = Rows::<FixtureRow>::decode(&rows.encode()).unwrap();
    assert_eq!(decoded.next_slot(), 8);
    assert_eq!(decoded.get(4), rows.get(4));
    rows
}

fn paint_example() -> Vec<u8> {
    use ipp_core::systems::gui::presentation::{GuiPaintPart, GuiTheme};
    use ipp_core::systems::gui::{GuiPartId, GuiPartVariant, GuiPrimitivePart, GuiSkinState};

    let mut theme = GuiTheme::default();
    let mut base = GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background)).unwrap();
    base.color = Some([0.25, 0.5, 0.75, 1.0]);
    theme.parts.insert(2, base).unwrap();
    let mut checked = GuiPaintPart::keyed(GuiPartId::variant(
        GuiPrimitivePart::Fill,
        GuiSkinState::Pressed,
        GuiPartVariant::Checked,
    ))
    .unwrap();
    checked.opacity = Some(0.5);
    theme.parts.insert(5, checked).unwrap();
    let bytes = theme.parts.encode();
    let mut restored = ipp_core::ComponentValue::GuiTheme(GuiTheme::default());
    ipp_core::components::registry::write(
        &mut restored,
        &ipp_core::FieldWrite {
            offset: std::mem::offset_of!(GuiTheme, parts) as u32,
            value: ipp_core::FieldValue::Rows(bytes.clone()),
        },
    )
    .unwrap();
    assert_eq!(
        restored.field(std::mem::offset_of!(GuiTheme, parts) as u32),
        Ok(FieldValue::Rows(bytes.clone()))
    );
    bytes
}

// The ignored pointer moves the table's real offset between targets; the region
// addresses and table encoding stay target-independent.
#[repr(C)]
#[derive(ipp_core::components::schema::SchemaComponent)]
struct RowsFixture {
    #[schema(ignore)]
    internal: usize,
    marker: u32,
    #[schema(rows)]
    rows: Rows<FixtureRow>,
}

impl Default for RowsFixture {
    fn default() -> Self {
        let mut rows = Rows::new();
        rows.insert(
            1,
            FixtureRow {
                weight: 0.5,
                label: Some("ok".into()),
                ..FixtureRow::default()
            },
        )
        .expect("fixture slot");
        Self {
            internal: 0,
            marker: 23,
            rows,
        }
    }
}

/// Row property addressing, slot liveness, bounded text and whole-table round trips.
fn check_rows() -> bool {
    use ipp_core::DynamicValue;
    use ipp_core::components::schema::FieldError;

    let property = |slot, index| Rows::<FixtureRow>::offset(0, slot, index).expect("offset");
    let table = std::mem::offset_of!(RowsFixture, rows) as u32;
    let mut fixture = RowsFixture::default();
    if !RowsFixture::has_field(property(1, 1))
        || fixture.field(property(1, 1)) != Ok(FieldValue::Unset)
        || fixture.field(property(0, 0)) != Err(FieldError::UnknownField)
        || fixture
            .set_field(
                property(1, 1),
                FieldValue::Dynamic(DynamicValue::Vec4([0.0, 0.0, 0.0, 1.0])),
            )
            .is_err()
        || fixture.set_field(property(1, 0), FieldValue::Dynamic(DynamicValue::U32(1)))
            != Err(FieldError::WrongType)
        || fixture.set_field(property(1, 0), FieldValue::Unset) != Err(FieldError::WrongType)
        || fixture.set_field(property(0, 0), FieldValue::Dynamic(DynamicValue::F32(1.0)))
            != Err(FieldError::UnknownField)
        || fixture.field(property(1, 3)) != Ok(FieldValue::String("ok".into()))
        || fixture.set_field(property(1, 3), FieldValue::String("ünï ok".into())) != Ok(())
        || fixture.set_field(property(1, 3), FieldValue::String("ninebytes".into()))
            != Err(FieldError::TextTooLong)
        || fixture.set_field(property(1, 0), FieldValue::String("1".into()))
            != Err(FieldError::WrongType)
    {
        return false;
    }

    let Ok(value @ FieldValue::Rows(_)) = fixture.field(table) else {
        return false;
    };
    let mut restored = RowsFixture {
        rows: Rows::new(),
        ..RowsFixture::default()
    };
    restored.set_field(table, value).is_ok()
        && restored.rows.next_slot() == 2
        && restored.rows.get(1).and_then(|row| row.rotation) == Some([0.0, 0.0, 0.0, 1.0])
        && restored.rows.get(1).map(|row| row.weight) == Some(0.5)
        && restored.rows.get(1).and_then(|row| row.label.as_deref()) == Some("ünï ok")
        && restored
            .set_field(property(1, 1), FieldValue::Unset)
            .is_ok()
        && restored.rows.get(1).and_then(|row| row.rotation).is_none()
        && restored.internal == 0
        && restored.marker == 23
}

/// Runs real generated owned typed dispatch inside the selected host target.
pub fn check() -> bool {
    use ipp_core::components::schema::FieldError;
    let mut bound = BoundFixture {
        label: "native".into(),
        internal: 7,
    };
    let bound_label = std::mem::offset_of!(BoundFixture, label) as u32;
    if BoundFixture::create().is_some()
        || !BoundFixture::has_field(bound_label)
        || bound
            .set_field(bound_label, FieldValue::String("bound".into()))
            .is_err()
        || &*bound.label != "bound"
        || bound.internal != 7
    {
        return false;
    }
    if !check_rows() {
        return false;
    }
    let mut fixture = LayoutFixture::default();
    let value = std::mem::offset_of!(LayoutFixture, value) as u32;
    let visible = std::mem::offset_of!(LayoutFixture, visible) as u32;
    let label = std::mem::offset_of!(LayoutFixture, label) as u32;
    let bytes = std::mem::offset_of!(LayoutFixture, bytes) as u32;
    let internal = std::mem::offset_of!(LayoutFixture, internal) as u32;
    if fixture.set_field(visible, FieldValue::U32(1)) != Err(FieldError::WrongType)
        || fixture.set_field(visible, FieldValue::Bool(false)).is_err()
        || fixture.set_field(value, FieldValue::U32(9)) != Err(FieldError::WrongType)
        || fixture.set_field(value + 1, FieldValue::F32(9.0)) != Err(FieldError::UnknownField)
        || fixture.set_field(internal, FieldValue::U64(0)) != Err(FieldError::UnknownField)
        || fixture.set_field(label + 1, FieldValue::String("bad".into()))
            != Err(FieldError::UnknownField)
        || fixture.set_field(value, FieldValue::F32(f32::NAN)) != Err(FieldError::NonFinite)
    {
        return false;
    }
    let owned = Arc::<str>::from("owned ✓");
    if fixture.set_field(label, FieldValue::String(owned)).is_err()
        || fixture
            .set_field(bytes, FieldValue::Bytes(vec![9, 8]))
            .is_err()
        || fixture.set_field(value, FieldValue::F32(-2.5)).is_err()
    {
        return false;
    }
    &*fixture.label == "owned ✓"
        && fixture.bytes == [9, 8]
        && fixture.value == -2.5
        && fixture.internal == 0
        && fixture.marker == 17
        && !fixture.visible
}

/// Export actual field types/layout/defaults after proving typed dispatch works.
pub fn export() -> Vec<u8> {
    assert!(check(), "target fixture failed");
    let mut bytes = b"IPPF".to_vec();
    bytes.write(&[usize::BITS as u8]);
    LayoutFixture::write_contract(&mut bytes);
    BoundFixture::write_contract(&mut bytes);
    RowsFixture::write_contract(&mut bytes);
    let rows = rows_example().encode();
    bytes.write(&(rows.len() as u32).to_le_bytes());
    bytes.write(&rows);
    let paint = paint_example();
    bytes.write(&(paint.len() as u32).to_le_bytes());
    bytes.write(&paint);
    bytes
}

#[cfg(test)]
mod tests {
    #[test]
    fn exact_fields_and_owned_values() {
        assert!(super::check());
    }
}
