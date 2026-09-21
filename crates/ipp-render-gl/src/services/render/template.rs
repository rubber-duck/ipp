//! Runtime text substitution and conditions, independent of shader families.

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RenderShaderTemplateError(pub(crate) String);

/// Expand named slots and nested `{{#if flag}}...{{/if}}` sections once.
/// Replacement text is opaque: it cannot introduce directives or recursion.
pub(crate) fn evaluate(
    source: &str,
    values: &[(&str, &str)],
    conditions: &[(&str, bool)],
) -> Result<String, RenderShaderTemplateError> {
    let mut output = String::with_capacity(source.len());
    let mut remaining = source;
    let mut active = vec![true];
    while let Some(start) = remaining.find("{{") {
        if *active.last().unwrap() {
            output.push_str(&remaining[..start]);
        }
        remaining = &remaining[start + 2..];
        let end = remaining
            .find("}}")
            .ok_or_else(|| RenderShaderTemplateError("unterminated template directive".into()))?;
        let name = remaining[..end].trim();
        if let Some(flag) = name.strip_prefix("#if ") {
            let enabled = conditions
                .iter()
                .find(|(key, _)| *key == flag.trim())
                .ok_or_else(|| {
                    RenderShaderTemplateError(format!("unknown template condition {flag}"))
                })?
                .1;
            active.push(*active.last().unwrap() && enabled);
        } else if name == "/if" {
            if active.len() == 1 {
                return Err(RenderShaderTemplateError("unmatched template /if".into()));
            }
            active.pop();
        } else if *active.last().unwrap() {
            let value = values.iter().find(|(key, _)| *key == name).ok_or_else(|| {
                RenderShaderTemplateError(format!("unknown template slot {name}"))
            })?;
            output.push_str(value.1);
        }
        remaining = &remaining[end + 2..];
    }
    if active.len() != 1 {
        return Err(RenderShaderTemplateError(
            "unclosed template condition".into(),
        ));
    }
    output.push_str(remaining);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    #[test]
    fn substitution_is_stage_independent_and_nonrecursive() {
        assert_eq!(
            evaluate(
                "{{ stage }}: {{body}} / {{body}}",
                &[("stage", "fragment"), ("body", "{{literal}}")],
                &[]
            )
            .unwrap(),
            "fragment: {{literal}} / {{literal}}"
        );
        assert_eq!(evaluate("plain", &[], &[]).unwrap(), "plain");
        assert!(evaluate("{{missing}}", &[], &[]).is_err());
        assert!(evaluate("{{unfinished", &[], &[]).is_err());
    }

    #[test]
    fn conditions_compose_independently_and_nest() {
        let source = "A{{#if first}}B{{#if second}}{{value}}{{/if}}C{{/if}}D{{#if second}}E{{/if}}";
        for (first, second, expected) in [
            (false, false, "AD"),
            (true, false, "ABCD"),
            (false, true, "ADE"),
            (true, true, "ABVCDE"),
        ] {
            assert_eq!(
                evaluate(
                    source,
                    &[("value", "V")],
                    &[("first", first), ("second", second)]
                )
                .unwrap(),
                expected
            );
        }
        assert!(evaluate("{{#if missing}}x{{/if}}", &[], &[]).is_err());
        assert!(evaluate("{{/if}}", &[], &[]).is_err());
        assert!(evaluate("{{#if on}}", &[], &[("on", false)]).is_err());
    }
}
