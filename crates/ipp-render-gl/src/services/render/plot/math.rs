//! Local matrix math shared by plot placement and label layout.

pub(super) fn point(model: [f32; 16], local: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        model[12 + row]
            + (0..3)
                .map(|col| model[col * 4 + row] * local[col])
                .sum::<f32>()
    })
}
