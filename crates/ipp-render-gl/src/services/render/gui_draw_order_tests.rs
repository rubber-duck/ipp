use super::*;

const DOMAIN: [f32; 4] = [0.0, 0.0, 400.0, 300.0];

/// Builds painter-ordered items with consecutive storage positions per kind.
#[derive(Default)]
struct Items {
    items: Vec<GuiDrawItem>,
    shapes: usize,
    glyphs: usize,
}

impl Items {
    fn shape(&mut self, bounds: [f32; 4]) -> &mut Self {
        self.shape_records(1, Some(bounds))
    }

    fn shape_records(&mut self, count: usize, bounds: Option<[f32; 4]>) -> &mut Self {
        self.items.push(GuiDrawItem::Shape {
            first: self.shapes,
            count,
            bounds,
        });
        self.shapes += count;
        self
    }

    fn text(&mut self, page: usize, bounds: [f32; 4]) -> &mut Self {
        self.items.push(GuiDrawItem::Glyphs {
            first: self.glyphs,
            count: 3,
            page,
            bounds: Some(bounds),
        });
        self.glyphs += 3;
        self
    }

    /// Leave `count` empty records before the next shape, as between storage slots.
    fn shape_gap(&mut self, count: usize) -> &mut Self {
        self.shapes += count;
        self
    }

    fn plan(&self) -> Vec<GuiDraw> {
        let mut order = GuiDrawOrder::default();
        order.begin(DOMAIN);
        for item in &self.items {
            order.push(*item);
        }
        order.finish().to_vec()
    }
}

fn shapes(first: usize, end: usize, records: usize) -> GuiDraw {
    GuiDraw {
        kind: GuiRecordKind::Shape,
        first,
        end,
        page: None,
        records,
    }
}

fn glyphs(page: usize, first: usize, end: usize, records: usize) -> GuiDraw {
    GuiDraw {
        kind: GuiRecordKind::Glyph,
        first,
        end,
        page: Some(page),
        records,
    }
}

#[test]
fn a_panel_of_interleaved_backgrounds_and_labels_is_two_draws() {
    let mut items = Items::default();
    items
        .shape([0.0, 0.0, 200.0, 200.0])
        .text(0, [10.0, 10.0, 120.0, 26.0])
        .shape([10.0, 40.0, 110.0, 64.0])
        .text(0, [20.0, 44.0, 90.0, 60.0])
        .shape([10.0, 80.0, 30.0, 100.0])
        .text(0, [40.0, 82.0, 140.0, 98.0]);

    assert_eq!(items.plan(), [shapes(0, 3, 3), glyphs(0, 0, 9, 9)]);
}

#[test]
fn a_later_shape_over_earlier_text_cuts_the_run() {
    let mut items = Items::default();
    items
        .shape([0.0, 0.0, 200.0, 200.0])
        .text(0, [10.0, 10.0, 120.0, 26.0])
        // A dialog over the label, then its own title.
        .shape([0.0, 0.0, 150.0, 100.0])
        .text(0, [20.0, 20.0, 100.0, 36.0])
        .shape([20.0, 60.0, 80.0, 80.0]);

    assert_eq!(
        items.plan(),
        [
            shapes(0, 1, 1),
            glyphs(0, 0, 3, 3),
            shapes(1, 3, 2),
            glyphs(0, 3, 6, 3),
        ]
    );
}

#[test]
fn a_cut_keeps_shapes_already_in_the_segment_before_the_overlap() {
    // Boxes of one batch: the first misses the label, the second covers it.
    let mut items = Items::default();
    items
        .text(0, [10.0, 10.0, 60.0, 20.0])
        .shape([100.0, 0.0, 120.0, 20.0])
        .shape([0.0, 0.0, 80.0, 30.0]);

    assert_eq!(
        items.plan(),
        [shapes(0, 1, 1), glyphs(0, 0, 3, 3), shapes(1, 2, 1)]
    );
}

#[test]
fn shapes_overlapping_text_cut_and_shapes_touching_it_do_not() {
    // Glyph bounds already include their antialias margin.
    let label = [10.0, 10.0, 60.0, 20.0];
    let mut near = Items::default();
    near.text(0, label).shape([59.5, 10.0, 80.0, 20.0]);
    assert_eq!(near.plan(), [glyphs(0, 0, 3, 3), shapes(0, 1, 1)]);

    let mut touching = Items::default();
    touching
        .text(0, label)
        .shape([60.0, 10.0, 80.0, 20.0])
        .shape([10.0, 20.0, 60.0, 30.0]);
    assert_eq!(touching.plan(), [shapes(0, 2, 2), glyphs(0, 0, 3, 3)]);
}

#[test]
fn text_after_a_shape_never_cuts() {
    let mut items = Items::default();
    items
        .shape([0.0, 0.0, 100.0, 100.0])
        .text(0, [0.0, 0.0, 100.0, 100.0])
        .text(0, [0.0, 0.0, 100.0, 100.0]);

    assert_eq!(items.plan(), [shapes(0, 1, 1), glyphs(0, 0, 6, 6)]);
}

#[test]
fn atlas_page_changes_split_glyph_draws_but_not_shapes() {
    let mut items = Items::default();
    items
        .shape([0.0, 0.0, 10.0, 10.0])
        .text(0, [20.0, 0.0, 30.0, 10.0])
        .shape([40.0, 0.0, 50.0, 10.0])
        .text(1, [60.0, 0.0, 70.0, 10.0])
        .text(0, [80.0, 0.0, 90.0, 10.0]);

    assert_eq!(
        items.plan(),
        [
            shapes(0, 2, 2),
            glyphs(0, 0, 3, 3),
            glyphs(1, 3, 6, 3),
            glyphs(0, 6, 9, 3),
        ]
    );
}

#[test]
fn draws_span_the_empty_room_between_slots() {
    let mut items = Items::default();
    items
        .shape_records(4, Some([0.0, 0.0, 10.0, 10.0]))
        .shape_gap(5)
        .shape_records(2, Some([20.0, 0.0, 30.0, 10.0]));

    assert_eq!(items.plan(), [shapes(0, 11, 6)]);
}

#[test]
fn shapes_painting_nothing_never_cut() {
    let mut items = Items::default();
    items
        .text(0, [0.0, 0.0, 100.0, 100.0])
        .shape_records(1, None);

    assert_eq!(items.plan(), [shapes(0, 1, 1), glyphs(0, 0, 3, 3)]);
}

#[test]
fn shapes_without_finite_bounds_cut_after_any_text() {
    let mut items = Items::default();
    items
        .text(0, [0.0, 0.0, 10.0, 10.0])
        .shape([f32::NAN, 0.0, 10.0, 10.0]);

    assert_eq!(items.plan(), [glyphs(0, 0, 3, 3), shapes(0, 1, 1)]);
}

#[test]
fn text_outside_the_domain_still_orders_shapes() {
    let mut items = Items::default();
    for index in 0..40 {
        let x = -500.0 - 20.0 * index as f32;
        items.text(0, [x, -400.0, x + 10.0, -390.0]);
    }
    items.shape([-520.0, -400.0, -505.0, -395.0]);

    let draws = items.plan();
    assert_eq!(draws.len(), 2);
    assert_eq!(draws[1], shapes(0, 1, 1));
}

/// Cuts by testing every earlier glyph rectangle of the segment.
fn reference(items: &[GuiDrawItem]) -> Vec<usize> {
    let mut cuts = Vec::new();
    let mut text: Vec<[f32; 4]> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match *item {
            GuiDrawItem::Shape {
                bounds: Some(bounds),
                ..
            } => {
                if text.iter().any(|rect| intersects(*rect, finite(bounds))) {
                    cuts.push(index);
                    text.clear();
                }
            }
            GuiDrawItem::Glyphs {
                bounds: Some(bounds),
                ..
            } => text.push(bounds),
            _ => {}
        }
    }
    cuts
}

/// Item indices of the cuts in `draws` of `items` whose first item is a shape: every
/// later segment begins with the shape that cut it, so each shape draw after the
/// first starts at a cut.
fn segment_starts(items: &[GuiDrawItem], draws: &[GuiDraw]) -> Vec<usize> {
    draws
        .iter()
        .filter(|draw| draw.kind == GuiRecordKind::Shape)
        .skip_while(|draw| draw.first == 0)
        .map(|draw| {
            items
                .iter()
                .position(
                    |item| matches!(item, GuiDrawItem::Shape { first, .. } if *first == draw.first),
                )
                .expect("shape draw starts at an item")
        })
        .collect()
}

#[test]
fn the_grid_cuts_exactly_where_testing_every_rectangle_does() {
    // A dense data grid of cells, each a background then its text, with overlays
    // and stray rectangles that do and do not reach earlier text.
    let mut state = 0x2545_f491_u32;
    let mut random = move |range: f32| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state as f32 / u32::MAX as f32) * range
    };
    let mut items = Items::default();
    for row in 0..40 {
        for column in 0..12 {
            let x = 4.0 + column as f32 * 32.0;
            let y = 4.0 + row as f32 * 7.0;
            items.shape([x, y, x + 30.0, y + 7.0]);
            items.text(0, [x + 3.0, y + 1.0, x + 3.0 + random(24.0), y + 6.0]);
            if random(1.0) < 0.05 {
                // Some reach back over earlier rows' text, others only over cells
                // still to come.
                let (x, y) = (random(400.0) - 20.0, y - random(40.0) + 10.0);
                items.shape([x, y, x + random(60.0), y + random(40.0)]);
            }
        }
    }

    let draws = items.plan();
    let cuts = reference(&items.items);
    assert!(cuts.len() > 4, "the overlays cut the grid: {cuts:?}");
    assert_eq!(segment_starts(&items.items, &draws), cuts);
    let records = |kind| {
        draws
            .iter()
            .filter(|draw| draw.kind == kind)
            .map(|draw| draw.records)
            .sum::<usize>()
    };
    assert_eq!(records(GuiRecordKind::Shape), items.shapes);
    assert_eq!(records(GuiRecordKind::Glyph), items.glyphs);
}
