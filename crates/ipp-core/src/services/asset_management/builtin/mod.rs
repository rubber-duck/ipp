//! Geometry construction shared by private debug meshes and optional `ipp://` sources.
//! URI parsing and public recipes compile only with `builtin-assets`.

use crate::ErrorReason;
#[cfg(feature = "builtin-assets")]
use std::borrow::Cow;

mod shapes;

#[cfg(all(feature = "builtin-assets", feature = "skeletal-animation"))]
mod rig;

#[cfg(all(feature = "builtin-assets", feature = "skeletal-animation"))]
pub use rig::rig;

#[cfg(feature = "builtin-assets")]
fn arguments<'a, const N: usize>(
    uri: &'a str,
    prefix: &str,
    names: [&str; N],
) -> Result<[Cow<'a, str>; N], ErrorReason> {
    argument_values(uri, prefix, names, N)
}

// Trailing optional parameters remain empty until the recipe applies its defaults.
#[cfg(feature = "builtin-assets")]
fn argument_values<'a, const N: usize>(
    uri: &'a str,
    prefix: &str,
    names: [&str; N],
    required: usize,
) -> Result<[Cow<'a, str>; N], ErrorReason> {
    if !uri.is_ascii() || uri.contains('#') {
        return Err(ErrorReason::InvalidAsset);
    }

    let query = uri.strip_prefix(prefix).ok_or(ErrorReason::InvalidAsset)?;
    let mut values = std::array::from_fn(|_| Cow::Borrowed(""));
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').ok_or(ErrorReason::InvalidAsset)?;
        let name = query_value(name)?;
        let index = names
            .iter()
            .position(|expected| *expected == name)
            .ok_or(ErrorReason::InvalidAsset)?;
        if !values[index].is_empty() || value.is_empty() {
            return Err(ErrorReason::InvalidAsset);
        }
        values[index] = query_value(value)?;
    }
    if values[..required].iter().any(|value| value.is_empty()) {
        return Err(ErrorReason::InvalidAsset);
    }

    Ok(values)
}

// The query contains only ASCII parameter names and numeric literals.
// Ordinary unescaped values borrow the URI; decode only when needed.
#[cfg(feature = "builtin-assets")]
fn query_value(value: &str) -> Result<Cow<'_, str>, ErrorReason> {
    if !value.contains(['%', '+']) {
        return Ok(Cow::Borrowed(value));
    }

    let mut decoded = String::new();
    decoded
        .try_reserve_exact(value.len())
        .map_err(|_| ErrorReason::Capacity)?;
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        let byte = match byte {
            b'+' => b' ',
            b'%' => {
                let high = bytes.next().and_then(|byte| (byte as char).to_digit(16));
                let low = bytes.next().and_then(|byte| (byte as char).to_digit(16));
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(ErrorReason::InvalidAsset);
                };
                (high * 16 + low) as u8
            }
            byte => byte,
        };
        if !byte.is_ascii() {
            return Err(ErrorReason::InvalidAsset);
        }
        decoded.push(byte as char);
    }

    Ok(Cow::Owned(decoded))
}

mod resources;
pub use resources::debug_mesh;
#[cfg(feature = "builtin-assets")]
pub use resources::{mesh, texture};
