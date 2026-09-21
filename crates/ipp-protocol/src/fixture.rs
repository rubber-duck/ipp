//! Target-executed layout and owned dispatch fixture; excluded without schema-export.

use ipp_core::EntityId;
use ipp_core::components::schema::{ContractSink, FieldValue, SchemaComponent};

#[repr(C)]
#[derive(ipp_core::components::schema::SchemaComponent)]
struct LayoutFixture {
    marker: u32,
    #[schema(ignore)]
    internal: usize,
    value: f32,
    label: String,
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
    label: String,
    #[schema(ignore)]
    internal: usize,
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
        || bound.label != "bound"
        || bound.internal != 7
    {
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
    let owned = String::from("owned ✓");
    if fixture.set_field(label, FieldValue::String(owned)).is_err()
        || fixture
            .set_field(bytes, FieldValue::Bytes(vec![9, 8]))
            .is_err()
        || fixture.set_field(value, FieldValue::F32(-2.5)).is_err()
    {
        return false;
    }
    fixture.label == "owned ✓"
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
    bytes
}

#[cfg(test)]
mod tests {
    #[test]
    fn exact_fields_and_owned_values() {
        assert!(super::check());
    }
}
