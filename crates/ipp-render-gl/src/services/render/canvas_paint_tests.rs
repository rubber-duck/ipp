//! Tests for paint body checks, generated canvas program text, slot admission and
//! per-canvas parameter blocks.

use super::*;
use ipp_core::DynamicValue;
use ipp_core::services::asset_management::shader::ShaderParameterKind as Kind;

fn source(body: &str, parameters: &[(&str, Kind)]) -> CanvasPaintSource {
    CanvasPaintSource {
        body: body.into(),
        parameters: parameters
            .iter()
            .map(|(name, kind)| (name.to_string(), *kind))
            .collect(),
    }
}

fn target(entity: u64) -> CanvasTarget {
    CanvasTarget {
        entity: ipp_core::EntityId::from_bits(entity),
        component: ipp_core::ComponentValue::CANVAS_PAINT,
        incarnation: 1,
    }
}

fn instance(entity: u64, properties: &[(&str, DynamicValue)]) -> CanvasPaintInstance {
    CanvasPaintInstance {
        target: target(entity),
        source: "paint:///test".into(),
        shader: None,
        properties: properties
            .iter()
            .map(|(name, value)| (Arc::from(*name), value.clone()))
            .collect(),
    }
}

fn key(slot: u32) -> AssetKey {
    AssetKey::from_u64(u64::from(slot) + 1)
}

#[test]
fn a_body_is_the_statements_of_one_function() {
    for body in [
        "return color;",
        "float line = step(0.5, fract(position.y / p_spacing)); return vec4(color.rgb * line, color.a);",
        "if (edge > -1.0) { return vec4(1.0); } // closing } in a comment\nreturn color;",
        "/* { */ return color;",
        "vec2 cell = fract(position / 8.0); for (int i = 0; i < 2; i++) { cell *= 2.0; } return color;",
    ] {
        assert_eq!(check_paint_body(body), Ok(()), "{body}");
    }

    for (body, reason) in [
        ("   \n", "empty"),
        ("#define X 1\nreturn color;", "preprocessor"),
        ("return color; } vec4 other() { return vec4(0.0);", "close"),
        ("{ return color;", "open"),
        ("discard; return color;", "discard"),
        ("float ippLight = 1.0; return color;", "reserved"),
        ("return color * u_paint_blocks[0];", "reserved"),
        ("vec4 v_color = color; return v_color;", "reserved"),
        ("return color; /* unterminated", "unterminated"),
    ] {
        let error = check_paint_body(body).expect_err(body);
        assert!(error.contains(reason), "{body}: {error}");
    }
}

#[test]
fn a_paint_function_reads_its_parameters_from_its_block() {
    let paint = source(
        "return color * p_tint * p_gain;",
        &[
            ("gain", Kind::F32),
            ("offset", Kind::Vec2),
            ("tint", Kind::Vec4),
        ],
    );
    let text = paint_function("ipp_paint_3", &paint);
    assert!(text.contains("#define p_gain (u_paint_blocks[ipp_block + 0].x)\n"));
    assert!(text.contains("#define p_offset (u_paint_blocks[ipp_block + 1].xy)\n"));
    assert!(text.contains("#define p_tint (u_paint_blocks[ipp_block + 2])\n"));
    assert!(text.contains(
        "vec4 ipp_paint_3(vec2 position, vec2 size, vec4 color, float edge, int ipp_block) {\nreturn color * p_tint * p_gain;\n}\n"
    ));
    // The macros end with the function, so another paint may reuse the names.
    assert!(text.ends_with("#undef p_gain\n#undef p_offset\n#undef p_tint\n"));

    let (vertex, fragment) = validation_sources(&paint);
    assert!(vertex.starts_with("#version 300 es\n"));
    assert!(fragment.starts_with("#version 300 es\nprecision highp float;\n"));
    assert!(fragment.contains(&format!(
        "uniform vec4 u_paint_blocks[{CANVAS_PAINT_VECTORS}];"
    )));
    assert!(fragment.contains("o_color = ipp_paint_0("));
}

#[test]
fn the_canvas_program_is_the_static_source_until_a_paint_is_admitted() {
    let base = crate::services::render::embedded_shader!("shaders/surface_gui.frag");
    assert!(
        matches!(canvas_fragment_source(std::iter::empty()), Cow::Borrowed(text) if text == base)
    );
    assert_eq!(base.matches(PAINT_PLACEHOLDER).count(), 1);

    let first = source("return color;", &[]);
    let third = source("return vec4(p_level);", &[("level", Kind::F32)]);
    let generated = canvas_fragment_source([(1, &first), (3, &third)].into_iter());
    assert!(!generated.contains(PAINT_PLACEHOLDER));
    assert!(generated.contains("#define IPP_CANVAS_PAINTS 1\n"));
    assert!(
        generated.contains(
            "    if (slot == 1) return ipp_paint_1(position, size, color, edge, block);\n"
        )
    );
    assert!(
        generated.contains(
            "    if (slot == 3) return ipp_paint_3(position, size, color, edge, block);\n"
        )
    );
    // The generated dispatch replaces the static one, which stays behind its guard.
    let generated_at = generated.find("#define IPP_CANVAS_PAINTS").unwrap();
    let static_at = generated.find("#ifndef IPP_CANVAS_PAINTS").unwrap();
    assert!(generated_at < static_at);
    assert!(generated.find("vec4 shape_color(").unwrap() > static_at);
}

/// Admit without a device: slot assignment alone.
fn programs() -> CanvasPaintPrograms<crate::PlatformRenderDevice> {
    CanvasPaintPrograms::default()
}

#[test]
fn paints_keep_their_slots_until_a_new_paint_needs_one_drawn_before_this_frame() {
    let mut programs = programs();
    let paint = source("return color;", &[]);
    programs.begin_frame();
    for slot in 1..=CANVAS_PAINT_SLOTS as u32 {
        assert_eq!(programs.admit(key(slot), &paint), Ok(slot));
    }
    assert!(programs.stale);
    // Readmitting is free; a ninth paint drawn in the same frame finds no slot.
    assert_eq!(programs.admit(key(4), &paint), Ok(4));
    assert_eq!(
        programs.admit(key(9), &paint),
        Err(CanvasPaintFallbackReason::SlotLimit)
    );

    // In the next frame the paint drawn least recently gives up its slot.
    programs.begin_frame();
    for slot in 2..=CANVAS_PAINT_SLOTS as u32 {
        assert_eq!(programs.admit(key(slot), &paint), Ok(slot));
    }
    assert_eq!(programs.admit(key(9), &paint), Ok(1));
    assert_eq!(
        programs.admit(key(1), &paint),
        Err(CanvasPaintFallbackReason::SlotLimit)
    );

    // A rejected paint stays rejected.
    programs.rejected.insert(key(20), "conflict".into());
    assert_eq!(
        programs.admit(key(20), &paint),
        Err(CanvasPaintFallbackReason::Program("conflict".into()))
    );
}

#[test]
fn blocks_hold_property_values_in_parameter_order_and_keep_their_place() {
    let paint = source(
        "return color;",
        &[("spacing", Kind::F32), ("tint", Kind::Vec3)],
    );
    let mut blocks = CanvasPaintBlocks::default();
    let mut reports = Vec::new();
    let values = |spacing: f32| {
        [
            ("spacing", DynamicValue::F32(spacing)),
            ("tint", DynamicValue::Vec3([0.1, 0.2, 0.3])),
            // Extra properties are component state the paint does not read.
            ("unused", DynamicValue::I32(7)),
        ]
    };
    let paints = [instance(1, &values(4.0)), instance(2, &values(8.0))];
    blocks.prepare(
        &paints,
        |_| Ok((2, &paint)),
        |paint, reason| reports.push((paint.target.entity, reason)),
    );
    assert!(reports.iter().all(|(_, reason)| reason.is_none()));
    assert_eq!(
        blocks.lanes(target(1)),
        Some(GuiPaintLanes {
            slot: 2,
            block: 0
        })
    );
    assert_eq!(
        blocks.lanes(target(2)),
        Some(GuiPaintLanes {
            slot: 2,
            block: 2
        })
    );
    assert_eq!(
        blocks.values,
        [
            [4.0, 0.0, 0.0, 0.0],
            [0.1, 0.2, 0.3, 0.0],
            [8.0, 0.0, 0.0, 0.0],
            [0.1, 0.2, 0.3, 0.0]
        ]
    );
    assert!(blocks.lanes_changed());
    let revision = blocks.revision;

    // A value change rewrites the block only.
    let paints = [instance(1, &values(4.0)), instance(2, &values(9.0))];
    blocks.prepare(&paints, |_| Ok((2, &paint)), |_, _| {});
    assert!(!blocks.lanes_changed());
    assert_eq!(blocks.values[2], [9.0, 0.0, 0.0, 0.0]);
    assert_ne!(blocks.revision, revision);
    let revision = blocks.revision;
    blocks.prepare(&paints, |_| Ok((2, &paint)), |_, _| {});
    assert_eq!(blocks.revision, revision, "unchanged values upload nothing");

    // A removed instance frees its block without moving the other one.
    blocks.prepare(&paints[1..], |_| Ok((2, &paint)), |_, _| {});
    assert!(blocks.lanes_changed());
    assert_eq!(blocks.lanes(target(1)), None);
    assert_eq!(blocks.lanes(target(2)).unwrap().block, 2);
}

#[test]
fn an_instance_that_cannot_paint_reports_why_and_takes_no_block() {
    let paint = source("return color;", &[("spacing", Kind::F32)]);
    // Each wide instance takes more than half of the array.
    let names: Vec<String> = (0..CANVAS_PAINT_VECTORS / 2 + 1)
        .map(|index| format!("p{index:03}"))
        .collect();
    let wide = CanvasPaintSource {
        body: "return color;".into(),
        parameters: names.iter().map(|name| (name.clone(), Kind::F32)).collect(),
    };
    let wide_values: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), DynamicValue::F32(1.0)))
        .collect();
    let paints = [
        instance(1, &[("spacing", DynamicValue::Vec2([1.0, 2.0]))]),
        instance(2, &[]),
        instance(3, &wide_values),
        instance(4, &wide_values),
        instance(5, &[("spacing", DynamicValue::F32(1.0))]),
    ];
    let mut blocks = CanvasPaintBlocks::default();
    let mut reports = BTreeMap::new();
    blocks.prepare(
        &paints,
        |instance| match instance.target.entity.to_bits() {
            1 | 2 => Ok((1, &paint)),
            3 | 4 => Ok((2, &wide)),
            _ => Err(CanvasPaintFallbackReason::SlotLimit),
        },
        |paint, reason| {
            reports.insert(paint.target.entity.to_bits(), reason);
        },
    );
    let parameter = Some(CanvasPaintFallbackReason::Parameter("spacing".into()));
    assert_eq!(reports[&1], parameter);
    assert_eq!(reports[&2], parameter);
    assert_eq!(reports[&3], None);
    assert_eq!(reports[&4], Some(CanvasPaintFallbackReason::UniformBudget));
    assert_eq!(reports[&5], Some(CanvasPaintFallbackReason::SlotLimit));
    for entity in [1, 2, 4, 5] {
        assert_eq!(blocks.lanes(target(entity)), None, "{entity}");
    }
    assert_eq!(blocks.values.len(), names.len());
    assert_eq!(first_fit(&[(0, 10), (12, 4)], 2), Some(10));
    assert_eq!(first_fit(&[(0, 10), (12, 4)], 3), Some(16));
    assert_eq!(first_fit(&[(0, CANVAS_PAINT_VECTORS as u32)], 1), None);
    assert_eq!(first_fit(&[(0, CANVAS_PAINT_VECTORS as u32)], 0), Some(0));
}
