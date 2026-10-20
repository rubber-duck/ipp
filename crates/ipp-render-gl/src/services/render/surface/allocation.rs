//! Rank required images by visible screen fraction times authored priority.
//! Every image retains a one-texel floor when feasible. In descending priority
//! order choose the largest requested binary quality tier that fits while
//! preserving those floors. Extra memory is never used to exceed pixel demand.
//! Planning is independent of the child-first order needed to render outputs.

use ipp_core::OutputRef;
use std::collections::BTreeMap;

pub(in crate::services::render) struct ImageRequest {
    pub output: OutputRef,
    pub size: [u32; 2],
    pub count: usize,
    pub score: f64,
}

impl ImageRequest {
    fn bytes(&self, size: [u32; 2]) -> usize {
        self.count
            .saturating_mul(size[0] as usize)
            .saturating_mul(size[1] as usize)
            .saturating_mul(4)
    }
}

pub(in crate::services::render) fn plan(
    mut requests: Vec<ImageRequest>,
    budget: usize,
) -> BTreeMap<OutputRef, usize> {
    requests.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.output.cmp(&b.output))
    });
    let mut floors = requests.iter().map(|r| r.bytes([1, 1])).sum::<usize>();
    let mut remaining = budget;
    let mut allocations = BTreeMap::new();
    for request in requests {
        floors = floors.saturating_sub(request.bytes([1, 1]));
        let available = remaining.saturating_sub(floors);
        let mut size = request.size;
        while request.bytes(size) > available && size != [1, 1] {
            size = size.map(|v| (v / 2).max(1));
        }
        let bytes = if request.bytes(size) <= available {
            request.bytes(size)
        } else {
            0
        };
        allocations.insert(request.output, bytes);
        remaining -= bytes;
    }
    allocations
}

#[cfg(test)]
#[path = "allocation_tests.rs"]
mod tests;
