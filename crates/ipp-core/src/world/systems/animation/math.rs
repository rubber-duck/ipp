//! Deterministic interpolation without runtime dependencies.

use super::AnimationValue;
use crate::{ErrorReason, components::schema::FieldValue};

pub(in crate::world) fn bezier_parameter(x: f64, x1: f64, x2: f64) -> f64 {
    let (mut low, mut high) = (0.0, 1.0);

    // Bisection also handles horizontal endpoint tangents; no division by a derivative.
    for _ in 0..52 {
        let u = (low + high) * 0.5;
        let v = 1.0 - u;
        let sample = 3.0 * v * v * u * x1 + 3.0 * v * u * u * x2 + u * u * u;
        if sample < x {
            low = u;
        } else {
            high = u;
        }
    }

    (low + high) * 0.5
}

pub(crate) fn normalize(q: [f32; 4]) -> [f32; 4] {
    let length = q.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>().sqrt();
    q.map(|v| (f64::from(v) / length) as f32)
}

pub(in crate::world) fn slerp(a: [f32; 4], b: [f32; 4], u: f64) -> [f32; 4] {
    let a = normalize(a);
    let mut b = normalize(b);
    let mut dot = a
        .iter()
        .zip(b)
        .map(|(a, b)| f64::from(*a) * f64::from(b))
        .sum::<f64>();
    if dot < 0.0 {
        b = b.map(|v| -v);
        dot = -dot;
    }

    let (wa, wb) = if dot > 0.9995 {
        (1.0 - u, u)
    } else {
        let angle = dot.clamp(-1.0, 1.0).acos();
        (
            ((1.0 - u) * angle).sin() / angle.sin(),
            (u * angle).sin() / angle.sin(),
        )
    };

    normalize(std::array::from_fn(|i| {
        (f64::from(a[i]) * wa + f64::from(b[i]) * wb) as f32
    }))
}

fn integer(values: &[u64], weights: &[f64]) -> u64 {
    let anchor = *values.iter().min().unwrap();
    let delta = values
        .iter()
        .zip(weights)
        .map(|(value, weight)| (*value - anchor) as f64 * weight)
        .sum::<f64>();

    // Keep the integer anchor exact, including adjacent values above 2^53.
    (u128::from(anchor) + (delta.round().max(0.0) as u128)).min(u128::from(u64::MAX)) as u64
}

pub(crate) fn mix(a: &AnimationValue, b: &AnimationValue, u: f64) -> AnimationValue {
    if u <= 0.0 {
        return a.clone();
    }
    if u >= 1.0 {
        return b.clone();
    }

    match (a, b) {
        (
            AnimationValue::Field(FieldValue::Dynamic(a)),
            AnimationValue::Field(FieldValue::Dynamic(b)),
        ) => AnimationValue::Field(FieldValue::Dynamic(
            crate::DynamicValue::weighted(&[a, b], &[1.0 - u, u]).unwrap_or_else(|_| b.clone()),
        )),
        (AnimationValue::Field(FieldValue::F32(a)), AnimationValue::Field(FieldValue::F32(b))) => {
            AnimationValue::Field(FieldValue::F32(
                (f64::from(*a) * (1.0 - u) + f64::from(*b) * u) as f32,
            ))
        }
        (AnimationValue::Field(FieldValue::U32(a)), AnimationValue::Field(FieldValue::U32(b))) => {
            AnimationValue::Field(FieldValue::U32(integer(
                &[u64::from(*a), u64::from(*b)],
                &[1.0 - u, u],
            ) as u32))
        }
        (AnimationValue::Field(FieldValue::U64(a)), AnimationValue::Field(FieldValue::U64(b))) => {
            AnimationValue::Field(FieldValue::U64(integer(&[*a, *b], &[1.0 - u, u])))
        }
        (AnimationValue::Rotation(a), AnimationValue::Rotation(b)) => {
            AnimationValue::Rotation(slerp(*a, *b, u))
        }
        #[cfg(feature = "skeletal-animation")]
        (AnimationValue::Pose(a), AnimationValue::Pose(b)) => {
            AnimationValue::Pose(super::pose::mix(a, b, u))
        }
        _ => b.clone(),
    }
}

pub(in crate::world) fn bezier_value(
    a: &AnimationValue,
    b: &AnimationValue,
    c: &AnimationValue,
    d: &AnimationValue,
    u: f64,
) -> AnimationValue {
    let v = 1.0 - u;
    let weights = [v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u];

    match (a, b, c, d) {
        (
            AnimationValue::Field(FieldValue::Dynamic(a)),
            AnimationValue::Field(FieldValue::Dynamic(b)),
            AnimationValue::Field(FieldValue::Dynamic(c)),
            AnimationValue::Field(FieldValue::Dynamic(d)),
        ) => AnimationValue::Field(FieldValue::Dynamic(
            crate::DynamicValue::weighted(&[a, b, c, d], &weights).unwrap_or_else(|_| d.clone()),
        )),
        (
            AnimationValue::Field(FieldValue::F32(a)),
            AnimationValue::Field(FieldValue::F32(b)),
            AnimationValue::Field(FieldValue::F32(c)),
            AnimationValue::Field(FieldValue::F32(d)),
        ) => AnimationValue::Field(FieldValue::F32(
            [a, b, c, d]
                .iter()
                .zip(weights)
                .map(|(value, weight)| f64::from(**value) * weight)
                .sum::<f64>() as f32,
        )),
        (
            AnimationValue::Field(FieldValue::U64(a)),
            AnimationValue::Field(FieldValue::U64(b)),
            AnimationValue::Field(FieldValue::U64(c)),
            AnimationValue::Field(FieldValue::U64(d)),
        ) => AnimationValue::Field(FieldValue::U64(integer(&[*a, *b, *c, *d], &weights))),
        (
            AnimationValue::Field(FieldValue::U32(a)),
            AnimationValue::Field(FieldValue::U32(b)),
            AnimationValue::Field(FieldValue::U32(c)),
            AnimationValue::Field(FieldValue::U32(d)),
        ) => AnimationValue::Field(FieldValue::U32(integer(
            &[u64::from(*a), u64::from(*b), u64::from(*c), u64::from(*d)],
            &weights,
        ) as u32)),
        _ => mix(
            &mix(&mix(a, b, u), &mix(b, c, u), u),
            &mix(&mix(b, c, u), &mix(c, d, u), u),
            u,
        ),
    }
}

fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [x, y, z, w] = a;
    let [i, j, k, l] = b;
    normalize([
        w * i + x * l + y * k - z * j,
        w * j - x * k + y * l + z * i,
        w * k + x * j - y * i + z * l,
        w * l - x * i - y * j - z * k,
    ])
}

pub(crate) fn additive(
    base: &AnimationValue,
    sample: &AnimationValue,
    reference: &AnimationValue,
    weight: f64,
) -> Result<AnimationValue, ErrorReason> {
    fn int(
        base: u64,
        sample: u64,
        reference: u64,
        weight: f64,
        max: u64,
    ) -> Result<u64, ErrorReason> {
        let delta = i128::from(sample) - i128::from(reference);
        // A finite layer weight is an exact binary fraction. Multiply its
        // significand as an integer so cancellation near u64::MAX stays exact.
        let bits = weight.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as u32;
        let significand = (bits & ((1u64 << 52) - 1))
            | if exponent == 0 {
                0
            } else {
                1u64 << 52
            };
        let shift = if exponent == 0 {
            1074
        } else {
            1075 - exponent
        };
        let product = delta.unsigned_abs() * u128::from(significand);
        let magnitude = if shift >= 128 {
            0
        } else {
            let remainder = product & ((1u128 << shift) - 1);
            let half = 1u128 << (shift - 1);
            let round_up = if delta < 0 {
                remainder > half
            } else {
                remainder >= half
            };
            (product >> shift) + u128::from(round_up)
        };
        let change = if delta < 0 {
            -(magnitude as i128)
        } else {
            magnitude as i128
        };
        let result = i128::from(base) + change;

        if result < 0 || result > i128::from(max) {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(result as u64)
    }

    Ok(match (base, sample, reference) {
        (
            AnimationValue::Field(FieldValue::Dynamic(a)),
            AnimationValue::Field(FieldValue::Dynamic(b)),
            AnimationValue::Field(FieldValue::Dynamic(c)),
        ) => AnimationValue::Field(FieldValue::Dynamic(
            crate::DynamicValue::weighted(&[a, b, c], &[1.0, weight, -weight])
                .map_err(|_| ErrorReason::InvalidValue)?,
        )),
        (
            AnimationValue::Field(FieldValue::F32(a)),
            AnimationValue::Field(FieldValue::F32(b)),
            AnimationValue::Field(FieldValue::F32(c)),
        ) => AnimationValue::Field(FieldValue::F32(
            (f64::from(*a) + (f64::from(*b) - f64::from(*c)) * weight) as f32,
        )),
        (
            AnimationValue::Field(FieldValue::U32(a)),
            AnimationValue::Field(FieldValue::U32(b)),
            AnimationValue::Field(FieldValue::U32(c)),
        ) => AnimationValue::Field(FieldValue::U32(int(
            u64::from(*a),
            u64::from(*b),
            u64::from(*c),
            weight,
            u64::from(u32::MAX),
        )? as u32)),
        (
            AnimationValue::Field(FieldValue::U64(a)),
            AnimationValue::Field(FieldValue::U64(b)),
            AnimationValue::Field(FieldValue::U64(c)),
        ) => AnimationValue::Field(FieldValue::U64(int(*a, *b, *c, weight, u64::MAX)?)),
        (
            AnimationValue::Rotation(base),
            AnimationValue::Rotation(sample),
            AnimationValue::Rotation(reference),
        ) => {
            let [x, y, z, w] = normalize(*reference);
            let relative = multiply([-x, -y, -z, w], normalize(*sample));
            AnimationValue::Rotation(multiply(
                normalize(*base),
                slerp([0.0, 0.0, 0.0, 1.0], relative, weight),
            ))
        }
        #[cfg(feature = "skeletal-animation")]
        (
            AnimationValue::Pose(base),
            AnimationValue::Pose(sample),
            AnimationValue::Pose(reference),
        ) => AnimationValue::Pose(super::pose::additive(base, sample, reference, weight)?),
        _ => return Err(ErrorReason::InvalidValue),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn additive_integer_cancellation_and_halfway_rounding_preserve_full_width() {
        let value = |v| AnimationValue::Field(FieldValue::U64(v));
        assert_eq!(
            additive(&value(u64::MAX), &value(0), &value(u64::MAX), 1.0).unwrap(),
            value(0)
        );
        assert_eq!(
            additive(&value(u64::MAX), &value(0), &value(u64::MAX), 0.5).unwrap(),
            value(1 << 63)
        );
        assert_eq!(
            additive(&value(10), &value(u64::MAX - 1), &value(u64::MAX), 0.5).unwrap(),
            value(10)
        );
        assert_eq!(
            additive(&value(10), &value(u64::MAX), &value(u64::MAX - 1), 0.5).unwrap(),
            value(11)
        );
        assert_eq!(
            additive(&value(u64::MAX), &value(1), &value(0), 1.0),
            Err(ErrorReason::InvalidValue)
        );
    }
}
