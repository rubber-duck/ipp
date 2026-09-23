//! Device-level Surface cache target oracle shared by the native Surface and
//! GUI runners: create, resize, repaint a translucent box (with glyph atlas
//! population nested inside the repaint in GUI builds), composite front and
//! mirrored views over an opaque background, then delete. Expected pixels are
//! derived from linear premultiplied blending, independently of the device.

pub(crate) fn run(
    context: &super::egl::Context,
    device: &mut ipp_render_gl::GlesRenderDevice,
    evidence: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use ipp_render_gl::{RenderDevice, RenderError};

    const WIDTH: u32 = super::world::WIDTH;
    const HEIGHT: u32 = super::world::HEIGHT;

    // Content is 2 x 1 metres at 32 texels per metre.
    let size = [2.0f32, 1.0];
    let limit = device.surface_cache_limit();
    if !(64..=2048).contains(&limit) {
        return Err(format!("unexpected Surface cache limit {limit}").into());
    }
    for (width, height) in [(0, 8), (limit + 1, 8)] {
        match device.create_surface_cache_target(width, height) {
            Err(RenderError::RenderDevice(_)) => {}
            Err(error) => return Err(format!("bounded size reported {error:?}").into()),
            Ok(target) => {
                device.delete_surface_cache_target(target);
                return Err(format!("{width}x{height} cache target was accepted").into());
            }
        }
    }

    let program = device.create_program(
        include_str!("../../src/services/render/shaders/surface_bitmap.vert"),
        include_str!("../../src/services/render/shaders/surface_cache.frag"),
    )?;
    let bitmap_program = device.create_program(
        include_str!("../../src/services/render/shaders/surface_bitmap.vert"),
        include_str!("../../src/services/render/shaders/surface_bitmap.frag"),
    )?;
    let white = device.create_texture(1, 1, &[255; 4])?;
    let mut target = device.create_surface_cache_target(16, 8)?;
    device.resize_surface_cache_target(&mut target, 64, 32)?;

    // Repaint before the frame, as the service pre-pass does. The content
    // mapping keeps texture row zero at the content top.
    let content = [
        1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0,
    ];
    device.begin_surface_cache_target(&target)?;
    if device.begin_surface_cache_target(&target).is_ok() {
        return Err("nested Surface cache targets were accepted".into());
    }
    if device
        .draw_surface_cache(&program, &target, &content, &size)
        .is_ok()
    {
        return Err("the bound Surface cache target was sampled".into());
    }

    // Glyph atlas population nested inside a repaint must return to the cache
    // target and its viewport; otherwise the box lands elsewhere or nowhere.
    #[cfg(feature = "gui")]
    {
        let page = device.create_glyph_atlas_page(256, 256)?;
        device.begin_glyph_atlas_page(&page)?;
        if device.begin_surface_cache_target(&target).is_ok() {
            return Err("a Surface cache target began inside atlas population".into());
        }
        device.end_glyph_atlas_page()?;
        device.delete_glyph_atlas_page(page);
    }

    device.set_surface_double_sided(true)?;
    device.draw_surface_bitmap(
        &bitmap_program,
        &white,
        &content,
        &[0.25, 0.25, 0.75, 0.5],
        &[0.0, 0.0, size[0], size[1]],
        &[1.0, 0.0, 0.0, 0.5],
    )?;
    device.set_surface_double_sided(false)?;
    device.end_surface_cache_target()?;

    // Content [0, 2] x [0, 1] covers pixels x 32..288 and rows 72..168.
    let front = [
        0.8, 0.0, 0.0, 0.0, 0.0, -0.8, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.8, 0.4, 0.0, 1.0,
    ];
    let mirrored = [
        -0.8, 0.0, 0.0, 0.0, 0.0, -0.8, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.8, 0.4, 0.0, 1.0,
    ];
    let mut captures = Vec::new();
    for mvp in [&front, &mirrored] {
        device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 1.0, 1.0])?;
        device.set_surface_double_sided(true)?;
        device.draw_surface_cache(&program, &target, mvp, &size)?;
        device.set_surface_double_sided(false)?;
        device.end_frame()?;
        captures.push(context.capture()?);
    }
    device.delete_surface_cache_target(target);
    device.delete_texture(white);
    device.delete_program(program);
    device.delete_program(bitmap_program);

    let pixel = |pixels: &[u8], x: u32, y: u32| -> [u8; 4] {
        let offset = ((y * WIDTH + x) * 4) as usize;
        pixels[offset..offset + 4].try_into().unwrap()
    };

    // Premultiplied composition of half-opaque red over blue is the linear
    // midpoint, sRGB 188. Applying opacity twice would leave red near 137.
    let blended = |value: [u8; 4]| {
        (180..=196).contains(&value[0]) && value[1] <= 8 && (180..=196).contains(&value[2])
    };
    let background = |value: [u8; 4]| value[0] <= 8 && value[1] <= 8 && value[2] >= 247;
    let mut report = String::new();
    for (name, pixels, painted, clear) in [
        ("front", &captures[0], 112, 208),
        ("mirrored", &captures[1], 208, 112),
    ] {
        let box_pixel = pixel(pixels, painted, 120);
        let clear_pixel = pixel(pixels, clear, 120);
        let outside = pixel(pixels, 10, 120);
        report.push_str(&format!(
            "{name}: box={box_pixel:?} transparent={clear_pixel:?} outside={outside:?}\n"
        ));
        if !blended(box_pixel) || !background(clear_pixel) || !background(outside) {
            std::fs::write(evidence.join("surface-cache-probe.txt"), &report)?;
            return Err(format!("{name} Surface cache composite mismatch: {report}").into());
        }
    }

    // Filtering across the box edge moves monotonically from the blend to the
    // background, so no dark premultiplication fringe appears.
    let edge: Vec<_> = (150..=170).map(|x| pixel(&captures[0], x, 120)).collect();
    if edge
        .windows(2)
        .any(|pair| pair[1][0] > pair[0][0] || pair[1][2] < pair[0][2])
    {
        return Err(format!("Surface cache edge is not monotonic: {edge:?}").into());
    }
    report.push_str(&format!("edge={edge:?}\n"));
    std::fs::write(evidence.join("surface-cache-front.rgba"), &captures[0])?;
    std::fs::write(evidence.join("surface-cache-mirrored.rgba"), &captures[1])?;
    std::fs::write(evidence.join("surface-cache-probe.txt"), report)?;
    Ok(())
}
