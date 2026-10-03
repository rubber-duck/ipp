use super::*;

fn invalid(reason: &ExpressionInvalid) -> ExpressionResult {
    ExpressionResult::Invalid(reason.clone())
}

pub(super) fn apply_unary(operator: UnaryOperator, source: &ExpressionResult) -> ExpressionResult {
    let ExpressionResult::Valid(value) = source else {
        let ExpressionResult::Invalid(reason) = source else {
            unreachable!()
        };
        return invalid(reason);
    };
    use DynamicValue as V;
    let result = match (operator, value) {
        (UnaryOperator::Negate, V::F32(v)) => Some(V::F32(-v)),
        (UnaryOperator::Negate, V::I32(v)) => v.checked_neg().map(V::I32),
        (UnaryOperator::Negate, V::Vec2(v)) => Some(V::Vec2(v.map(|x| -x))),
        (UnaryOperator::Negate, V::Vec3(v)) => Some(V::Vec3(v.map(|x| -x))),
        (UnaryOperator::Negate, V::Vec4(v)) => Some(V::Vec4(v.map(|x| -x))),
        (UnaryOperator::Absolute, V::F32(v)) => Some(V::F32(v.abs())),
        (UnaryOperator::Absolute, V::I32(v)) => v.checked_abs().map(V::I32),
        (UnaryOperator::Absolute, V::Vec2(v)) => Some(V::Vec2(v.map(f32::abs))),
        (UnaryOperator::Absolute, V::Vec3(v)) => Some(V::Vec3(v.map(f32::abs))),
        (UnaryOperator::Absolute, V::Vec4(v)) => Some(V::Vec4(v.map(f32::abs))),
        (UnaryOperator::Not, V::Bool(v)) => Some(V::Bool(!v)),
        (UnaryOperator::Length, V::Text(v)) => u32::try_from(v.chars().count()).ok().map(V::U32),
        _ => unreachable!("operator type checked during preparation"),
    };
    checked_result(result)
}

fn checked_result(value: Option<DynamicValue>) -> ExpressionResult {
    match value {
        Some(value) if value.validate().is_ok() => ExpressionResult::Valid(value),
        _ => ExpressionResult::Invalid(ExpressionInvalid::Calculation),
    }
}

pub(super) fn apply_binary(
    operator: BinaryOperator,
    left: &ExpressionResult,
    right: &ExpressionResult,
) -> ExpressionResult {
    let (ExpressionResult::Valid(a), ExpressionResult::Valid(b)) = (left, right) else {
        return match left {
            ExpressionResult::Invalid(reason) => invalid(reason),
            _ => match right {
                ExpressionResult::Invalid(reason) => invalid(reason),
                _ => unreachable!(),
            },
        };
    };
    use BinaryOperator as O;
    use DynamicValue as V;
    if operator == O::Equal {
        return ExpressionResult::Valid(V::Bool(a == b));
    }
    let result = match (a, b) {
        (V::F32(a), V::F32(b)) => scalar_float(operator, *a, *b)
            .map(V::F32)
            .or_else(|| compare_float(operator, *a, *b).map(V::Bool)),
        (V::I32(a), V::I32(b)) => scalar_i32(operator, *a, *b)
            .map(V::I32)
            .or_else(|| compare_ord(operator, a, b).map(V::Bool)),
        (V::U32(a), V::U32(b)) => scalar_u32(operator, *a, *b)
            .map(V::U32)
            .or_else(|| compare_ord(operator, a, b).map(V::Bool)),
        (V::Vec2(a), V::Vec2(b)) => vector(operator, a, b).map(V::Vec2),
        (V::Vec3(a), V::Vec3(b)) => vector(operator, a, b).map(V::Vec3),
        (V::Vec4(a), V::Vec4(b)) => vector(operator, a, b).map(V::Vec4),
        (V::Bool(a), V::Bool(b)) => match operator {
            O::And => Some(V::Bool(*a && *b)),
            O::Or => Some(V::Bool(*a || *b)),
            _ => None,
        },
        _ => None,
    };
    checked_result(result)
}

fn scalar_float(operator: BinaryOperator, a: f32, b: f32) -> Option<f32> {
    use BinaryOperator as O;
    match operator {
        O::Add => Some(a + b),
        O::Subtract => Some(a - b),
        O::Multiply => Some(a * b),
        O::Divide if b != 0.0 => Some(a / b),
        O::Minimum => Some(a.min(b)),
        O::Maximum => Some(a.max(b)),
        _ => None,
    }
}

fn compare_float(operator: BinaryOperator, a: f32, b: f32) -> Option<bool> {
    use BinaryOperator as O;
    match operator {
        O::Less => Some(a < b),
        O::Greater => Some(a > b),
        _ => None,
    }
}

fn compare_ord<T: Ord>(operator: BinaryOperator, a: &T, b: &T) -> Option<bool> {
    use BinaryOperator as O;
    match operator {
        O::Less => Some(a < b),
        O::Greater => Some(a > b),
        _ => None,
    }
}

fn scalar_i32(operator: BinaryOperator, a: i32, b: i32) -> Option<i32> {
    use BinaryOperator as O;
    match operator {
        O::Add => a.checked_add(b),
        O::Subtract => a.checked_sub(b),
        O::Multiply => a.checked_mul(b),
        O::Divide => a.checked_div(b),
        O::Minimum => Some(a.min(b)),
        O::Maximum => Some(a.max(b)),
        _ => None,
    }
}

fn scalar_u32(operator: BinaryOperator, a: u32, b: u32) -> Option<u32> {
    use BinaryOperator as O;
    match operator {
        O::Add => a.checked_add(b),
        O::Subtract => a.checked_sub(b),
        O::Multiply => a.checked_mul(b),
        O::Divide => a.checked_div(b),
        O::Minimum => Some(a.min(b)),
        O::Maximum => Some(a.max(b)),
        _ => None,
    }
}

fn vector<const N: usize>(
    operator: BinaryOperator,
    a: &[f32; N],
    b: &[f32; N],
) -> Option<[f32; N]> {
    let mut result = [0.0; N];
    for i in 0..N {
        result[i] = scalar_float(operator, a[i], b[i])?;
    }
    Some(result)
}

pub(super) fn apply_clamp(
    value: &ExpressionResult,
    minimum: &ExpressionResult,
    maximum: &ExpressionResult,
) -> ExpressionResult {
    for result in [value, minimum, maximum] {
        if let ExpressionResult::Invalid(reason) = result {
            return invalid(reason);
        }
    }
    let (
        ExpressionResult::Valid(value),
        ExpressionResult::Valid(minimum),
        ExpressionResult::Valid(maximum),
    ) = (value, minimum, maximum)
    else {
        unreachable!()
    };
    use DynamicValue as V;
    let result = match (value, minimum, maximum) {
        (V::F32(v), V::F32(lo), V::F32(hi)) if lo <= hi => Some(V::F32(v.clamp(*lo, *hi))),
        (V::I32(v), V::I32(lo), V::I32(hi)) if lo <= hi => Some(V::I32((*v).clamp(*lo, *hi))),
        (V::U32(v), V::U32(lo), V::U32(hi)) if lo <= hi => Some(V::U32((*v).clamp(*lo, *hi))),
        (V::Vec2(v), V::Vec2(lo), V::Vec2(hi)) => clamp_vector(v, lo, hi).map(V::Vec2),
        (V::Vec3(v), V::Vec3(lo), V::Vec3(hi)) => clamp_vector(v, lo, hi).map(V::Vec3),
        (V::Vec4(v), V::Vec4(lo), V::Vec4(hi)) => clamp_vector(v, lo, hi).map(V::Vec4),
        _ => None,
    };
    checked_result(result)
}

fn clamp_vector<const N: usize>(
    value: &[f32; N],
    minimum: &[f32; N],
    maximum: &[f32; N],
) -> Option<[f32; N]> {
    let mut result = [0.0; N];
    for i in 0..N {
        if minimum[i] > maximum[i] {
            return None;
        }
        result[i] = value[i].clamp(minimum[i], maximum[i]);
    }
    Some(result)
}
