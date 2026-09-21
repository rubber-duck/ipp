//! Replaceable spatial acceleration of finalized, component-owned geometry.

mod bvh;
mod index;
mod query;

#[cfg(test)]
mod spatial_tests;

pub use index::{GeometryPreparedBounds, GeometrySpatialBackend, GeometrySpatialIndex};
pub use query::{GeometryQueryResults, GeometryQueryScratch};

type BoxBounds = [[f64; 3]; 2];

fn union(a: BoxBounds, b: BoxBounds) -> BoxBounds {
    [
        std::array::from_fn(|i| a[0][i].min(b[0][i])),
        std::array::from_fn(|i| a[1][i].max(b[1][i])),
    ]
}

fn area(b: BoxBounds) -> f64 {
    let d: [f64; 3] = std::array::from_fn(|i| b[1][i] - b[0][i]);
    d[0] * d[1] + d[1] * d[2] + d[2] * d[0]
}
