//! Independent completed-frame oracles, without CEF, EGL or launch details.
#[derive(Clone, Copy)]
pub enum FrameState {
    Initial,
    Updated,
    Transparent,
}

pub fn assert_frame(bytes: &[u8], state: FrameState) -> Result<(), String> {
    if bytes.len() != 800 * 600 * 4 {
        return Err("incorrect frame dimensions".into());
    }
    let changed = matches!(state, FrameState::Updated);
    let expected = [
        [220, 38, 38],
        [34, 197, 94],
        if changed {
            [217, 70, 239]
        } else {
            [37, 99, 235]
        },
        [250, 204, 21],
    ];
    for (index, color) in expected.into_iter().enumerate() {
        // Lower-page CSS swatches verify channel order, top-left orientation,
        // scaling, sRGB handling and actual JavaScript content replacement.
        let x = 160 + index * 160;
        for y in 484..492 {
            for dx in 0..8 {
                let offset = (y * 800 + x + dx) * 4;
                if !bytes[offset..offset + 3]
                    .iter()
                    .zip(color)
                    .all(|(a, b)| a.abs_diff(b) <= 6)
                {
                    return Err(format!(
                        "swatch {index}: {:?}, expected {color:?}",
                        &bytes[offset..offset + 4]
                    ));
                }
            }
        }
    }
    // Header is above the swatches, with the neutral exterior outside placement.
    for (x, y, color) in [(100, 110, [23, 37, 84]), (10, 10, [33, 44, 60])] {
        let offset = (y * 800 + x) * 4;
        if !bytes[offset..offset + 3]
            .iter()
            .zip(color)
            .all(|(a, b)| a.abs_diff(b) <= 6)
        {
            return Err(format!("header/background mismatch at {x},{y}"));
        }
    }
    if matches!(state, FrameState::Transparent) {
        // Half-transparent sRGB red must blend in linear light over IPP's
        // independently specified linear background, not over the old web page.
        let background = [0.015_f32, 0.025, 0.045];
        let alpha = 128.0 / 255.0;
        let expected = background
            .into_iter()
            .enumerate()
            .map(|(i, bg)| {
                let linear = if i == 0 {
                    alpha
                } else {
                    0.0
                } + bg * (1.0 - alpha);
                let srgb = if linear <= 0.0031308 {
                    12.92 * linear
                } else {
                    1.055 * linear.powf(1.0 / 2.4) - 0.055
                };
                (srgb * 255.0).round() as u8
            })
            .collect::<Vec<_>>();
        for y in 452..460 {
            for x in 600..620 {
                let offset = (y * 800 + x) * 4;
                if !bytes[offset..offset + 3]
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.abs_diff(*b) <= 5)
                {
                    return Err(format!(
                        "transparent colour {:?}, expected {expected:?}",
                        &bytes[offset..offset + 4]
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn assert_canvas_changed(before: &[u8], after: &[u8]) -> Result<(), String> {
    let mut changed = 0;
    for y in 270..350 {
        for x in 450..650 {
            let offset = (y * 800 + x) * 4;
            if before[offset..offset + 3] != after[offset..offset + 3] {
                changed += 1;
            }
        }
    }
    if changed < 1000 {
        return Err(format!(
            "HTML canvas did not update: only {changed} changed pixels"
        ));
    }
    Ok(())
}
