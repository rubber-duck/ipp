//! A second, non-sRGB attachment records the last contributing Canvas layer.
//! Color still blends normally; the ownership output has alpha one, so both
//! straight and premultiplied blending replace its value without mixing IDs.

pub(super) fn fragment(source: &str) -> std::borrow::Cow<'_, str> {
    if !source.contains("in vec2 v_surface_position;") || !source.contains("out vec4 o_color;") {
        return source.into();
    }

    let source = source.replace(
        "out vec4 o_color;",
        "layout(location = 0) out vec4 o_color;\nlayout(location = 1) out vec4 o_ownership;\nuniform highp float u_surface_owner;",
    ).replace("void main()", "void ipp_surface_color()");
    format!("{source}\nvoid main() {{\n    ipp_surface_color();\n    if (o_color.a <= 0.0) discard;\n    vec3 id = mod(floor(u_surface_owner / vec3(1.0, 256.0, 65536.0)), 256.0);\n    o_ownership = vec4(id / 255.0, 1.0);\n}}\n").into()
}
