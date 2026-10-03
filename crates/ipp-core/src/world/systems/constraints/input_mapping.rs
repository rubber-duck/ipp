//! Canonical expression-driver input selectors. Addresses come from generated
//! component schema/metadata. The single entity handle is the driver's typed
//! `source` field, so snapshots remap it normally; bytes contain no identities.

use crate::ErrorReason;

/// Exact schema field, numeric row address or dynamic property metadata key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DriverProperty {
    /// Registered component identity.
    pub component: u16,
    /// Exact schema field/property address on the compiled target.
    pub offset: u32,
}

/// One consumer selection for an asset's named declared input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionDriverInput {
    /// Exact input name from the expression declaration.
    pub name: String,
    /// Property on the driver's single source entity.
    pub property: DriverProperty,
}

/// Maximum inputs supplied by one component driver (the shared engine has its own limits).
pub const EXPRESSION_DRIVER_MAX_INPUTS: usize = 256;

const MAX_NAME: usize = 1024;
const MAX_BYTES: usize = 8 + EXPRESSION_DRIVER_MAX_INPUTS * (8 + MAX_NAME);

/// IPDI v1: magic, LE u16 version/count, then strictly name-sorted entries of
/// LE u16 UTF-8 length, name bytes, LE u16 component, LE u32 property address.
/// Empty bytes canonically encode zero inputs. No duplicate names/trailing bytes.
/// Encoding sorts selections without changing the caller's input slice.
pub fn encode_expression_driver_inputs(
    inputs: &[ExpressionDriverInput],
) -> Result<Vec<u8>, ErrorReason> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    if inputs.len() > EXPRESSION_DRIVER_MAX_INPUTS {
        return Err(ErrorReason::InvalidValue);
    }
    let mut sorted: Vec<_> = inputs.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut bytes = b"IPDI".to_vec();
    bytes.extend(1u16.to_le_bytes());
    bytes.extend((inputs.len() as u16).to_le_bytes());
    let mut previous: Option<&str> = None;
    for input in sorted {
        if input.name.is_empty()
            || input.name.len() > MAX_NAME
            || previous.is_some_and(|name| name >= input.name.as_str())
        {
            return Err(ErrorReason::InvalidValue);
        }
        previous = Some(&input.name);
        bytes.extend((input.name.len() as u16).to_le_bytes());
        bytes.extend(input.name.as_bytes());
        bytes.extend(input.property.component.to_le_bytes());
        bytes.extend(input.property.offset.to_le_bytes());
    }
    Ok(bytes)
}

/// Decode the bounded canonical format; rejects malformed and noncanonical bytes.
pub fn decode_expression_driver_inputs(
    bytes: &[u8],
) -> Result<Vec<ExpressionDriverInput>, ErrorReason> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.len() < 8 || bytes.len() > MAX_BYTES || &bytes[..6] != b"IPDI\x01\x00" {
        return Err(ErrorReason::InvalidValue);
    }
    let count = u16::from_le_bytes(bytes[6..8].try_into().unwrap()) as usize;
    if count == 0 || count > EXPRESSION_DRIVER_MAX_INPUTS {
        return Err(ErrorReason::InvalidValue);
    }
    let mut rest = &bytes[8..];
    let mut inputs: Vec<ExpressionDriverInput> = Vec::with_capacity(count);
    for _ in 0..count {
        let length = rest
            .get(..2)
            .map(|length| u16::from_le_bytes(length.try_into().unwrap()) as usize)
            .ok_or(ErrorReason::InvalidValue)?;
        if length == 0 || length > MAX_NAME || rest.len() < length + 8 {
            return Err(ErrorReason::InvalidValue);
        }
        let name =
            std::str::from_utf8(&rest[2..2 + length]).map_err(|_| ErrorReason::InvalidValue)?;
        if inputs
            .last()
            .is_some_and(|previous| previous.name.as_str() >= name)
        {
            return Err(ErrorReason::InvalidValue);
        }
        let tail = &rest[2 + length..];
        inputs.push(ExpressionDriverInput {
            name: name.into(),
            property: DriverProperty {
                component: u16::from_le_bytes(tail[..2].try_into().unwrap()),
                offset: u32::from_le_bytes(tail[2..6].try_into().unwrap()),
            },
        });
        rest = &tail[6..];
    }
    if !rest.is_empty() {
        return Err(ErrorReason::InvalidValue);
    }
    Ok(inputs)
}
