use super::*;

fn ratios(entries: &[&str]) -> RatioSet {
    RatioSet::parse(&entries.iter().map(ToString::to_string).collect::<Vec<_>>()).0
}

fn tool(image: (u32, u32)) -> CropTool {
    CropTool::new(image, image, None)
}

/// Presses at `from`, drags to `to` (with or without Shift) and releases.
fn drag(tool: &mut CropTool, from: (f64, f64), to: (f64, f64), shift: bool, set: &RatioSet) -> PixelRect {
    let hit = tool.hit_test(from.0, from.1, 6.0);
    tool.press(from.0, from.1, hit);
    tool.drag_to(to.0, to.1, shift, set);
    tool.release();
    tool.rect()
}

#[test]
fn the_tool_starts_with_the_whole_picture_and_the_original_ratio() {
    let set = ratios(&["4:5", "5:4"]);
    let tool = tool((893, 1221));
    assert_eq!(tool.rect(), PixelRect::full((893, 1221)));
    assert_eq!(tool.size_label(), "893 x 1221 px");
    assert_eq!(tool.ratio_label(&set), "O");
    assert!(tool.is_whole_image());
}

#[test]
fn the_ratio_button_reproduces_the_reference_pictures() {
    let set = ratios(&["5:4", "4:5"]);
    let mut tool = tool((893, 1221));
    tool.cycle_ratio(&set, false);
    assert_eq!(
        (tool.ratio_label(&set), tool.size_label()),
        ("4:5", "893 x 1116 px".to_string())
    );
    tool.cycle_ratio(&set, true);
    assert_eq!(
        (tool.ratio_label(&set), tool.size_label()),
        ("5:4", "893 x 714 px".to_string())
    );
    tool.cycle_ratio(&set, true);
    assert_eq!(
        (tool.ratio_label(&set), tool.size_label()),
        ("O", "893 x 1221 px".to_string())
    );
}

#[test]
fn cycling_ratios_fits_into_the_reference_and_never_shrinks_step_by_step() {
    let set = ratios(&["16:9", "1:1", "21:9"]);
    let mut tool = tool((1000, 1000));
    drag(&mut tool, (1000.0, 1000.0), (600.0, 400.0), false, &set);
    let reference = tool.rect();
    assert_eq!(reference, PixelRect::new(0, 0, 600, 400));
    let mut seen = Vec::new();
    for _ in 0..8 {
        tool.cycle_ratio(&set, true);
        seen.push(tool.rect());
        assert!(tool.rect().right() <= 600 && tool.rect().bottom() <= 400);
    }
    // Two rounds give the same rectangles: nothing drifts.
    assert_eq!(seen[..4], seen[4..]);
    assert_eq!(seen[1], PixelRect::new(100, 0, 400, 400));
}

#[test]
fn the_hit_test_finds_corners_edges_inside_and_outside() {
    let tool = tool((400, 300));
    assert_eq!(tool.hit_test(2.0, 3.0, 6.0), Hit::Handle(Handle::TopLeft));
    assert_eq!(tool.hit_test(398.0, 298.0, 6.0), Hit::Handle(Handle::BottomRight));
    assert_eq!(tool.hit_test(200.0, 4.0, 6.0), Hit::Handle(Handle::Top));
    assert_eq!(tool.hit_test(120.0, 299.0, 6.0), Hit::Handle(Handle::Bottom));
    assert_eq!(tool.hit_test(395.0, 150.0, 6.0), Hit::Handle(Handle::Right));
    assert_eq!(tool.hit_test(1.0, 100.0, 6.0), Hit::Handle(Handle::Left));
    assert_eq!(tool.hit_test(200.0, 150.0, 6.0), Hit::Inside);
    let mut small = self::tool((400, 300));
    let set = ratios(&[]);
    drag(&mut small, (400.0, 300.0), (10.0, 10.0), false, &set);
    assert_eq!(small.rect(), PixelRect::new(0, 0, 10, 10));
    // A rectangle too small for its handles is moved from inside.
    assert_eq!(small.hit_test(5.0, 5.0, 6.0), Hit::Inside);
    assert_eq!(small.hit_test(300.0, 200.0, 6.0), Hit::Outside);
}

#[test]
fn moving_keeps_the_size_and_stays_inside_the_picture() {
    let set = ratios(&[]);
    let mut tool = tool((400, 300));
    drag(&mut tool, (400.0, 300.0), (200.0, 150.0), false, &set);
    assert_eq!(tool.rect(), PixelRect::new(0, 0, 200, 150));
    drag(&mut tool, (100.0, 75.0), (150.3, 95.6), false, &set);
    assert_eq!(tool.rect(), PixelRect::new(50, 21, 200, 150));
    drag(&mut tool, (100.0, 100.0), (900.0, 900.0), false, &set);
    assert_eq!(tool.rect(), PixelRect::new(200, 150, 200, 150));
    drag(&mut tool, (300.0, 200.0), (-900.0, -900.0), false, &set);
    assert_eq!(tool.rect(), PixelRect::new(0, 0, 200, 150));
}

#[test]
fn a_free_resize_follows_the_pointer_and_keeps_the_minimum_size() {
    let set = ratios(&["16:9"]);
    let mut tool = tool((400, 300));
    let rect = drag(&mut tool, (0.0, 0.0), (100.4, 50.6), false, &set);
    assert_eq!(rect, PixelRect::new(100, 51, 300, 249));
    assert_eq!(tool.ratio_label(&set), "Free");
    // The handle cannot pass the opposite edge.
    let rect = drag(&mut tool, (100.0, 51.0), (1000.0, 1000.0), false, &set);
    assert_eq!(
        rect,
        PixelRect::new(400 - MIN_CROP_SIZE, 300 - MIN_CROP_SIZE, MIN_CROP_SIZE, MIN_CROP_SIZE)
    );
    // The pointer outside the picture puts the edge on the picture's border.
    let mut tool = self::tool((400, 300));
    let rect = drag(&mut tool, (400.0, 150.0), (900.0, 10.0), false, &set);
    assert_eq!(rect, PixelRect::full((400, 300)));
}

#[test]
fn shift_on_a_corner_keeps_the_ratio_and_the_opposite_corner() {
    let set = ratios(&["16:9"]);
    let mut tool = tool((1920, 1080));
    tool.cycle_ratio(&set, true);
    assert_eq!(tool.ratio_label(&set), "16:9");
    let rect = drag(&mut tool, (1920.0, 1080.0), (1000.0, 300.0), true, &set);
    assert_eq!((rect.x, rect.y), (0, 0));
    assert_eq!(rect.size(), (1000, 563));
    assert_eq!(tool.ratio_label(&set), "16:9");
    // Pulled outside, it stops at the picture's edge with the same proportions.
    let rect = drag(&mut tool, (1000.0, 563.0), (5000.0, 5000.0), true, &set);
    assert_eq!(rect, PixelRect::full((1920, 1080)));
    // The top left corner keeps the bottom right one; the larger of the two distances to the pointer decides the size.
    let rect = drag(&mut tool, (0.0, 0.0), (960.0, 100.0), true, &set);
    assert_eq!((rect.right(), rect.bottom()), (1920, 1080));
    assert_eq!(rect.size(), (1742, 980));
    let rect = drag(&mut tool, (178.0, 100.0), (960.0, 900.0), true, &set);
    assert_eq!(rect.size(), (960, 540));
}

#[test]
fn shift_without_a_matching_choice_keeps_the_rectangles_own_proportions() {
    let set = ratios(&["16:9"]);
    let mut tool = tool((1000, 1000));
    drag(&mut tool, (1000.0, 1000.0), (400.0, 200.0), false, &set);
    assert_eq!(tool.ratio_label(&set), "Free");
    let rect = drag(&mut tool, (400.0, 200.0), (800.0, 250.0), true, &set);
    assert_eq!(rect.size(), (800, 400));
}

#[test]
fn shift_on_an_edge_grows_the_other_axis_around_its_middle() {
    let set = ratios(&["1:1"]);
    let mut tool = tool((1000, 1000));
    tool.cycle_ratio(&set, true);
    drag(&mut tool, (1000.0, 1000.0), (600.0, 600.0), true, &set);
    assert_eq!(tool.rect(), PixelRect::new(0, 0, 600, 600));
    drag(&mut tool, (300.0, 300.0), (500.0, 500.0), false, &set);
    assert_eq!(tool.rect(), PixelRect::new(200, 200, 600, 600));
    let rect = drag(&mut tool, (800.0, 500.0), (700.0, 520.0), true, &set);
    assert_eq!(rect, PixelRect::new(200, 250, 500, 500));
    // The other axis is pushed back inside the picture when it would leave it.
    let rect = drag(&mut tool, (500.0, 750.0), (500.0, 1000.0), true, &set);
    assert_eq!(rect.size(), (750, 750));
    assert!(rect.right() <= 1000 && rect.bottom() <= 1000);
}

#[test]
fn a_drag_outside_draws_a_new_rectangle_in_any_direction() {
    let set = ratios(&["4:3"]);
    let mut tool = tool((1000, 800));
    drag(&mut tool, (1000.0, 800.0), (500.0, 400.0), false, &set);
    let rect = drag(&mut tool, (900.0, 700.0), (700.4, 600.2), false, &set);
    assert_eq!(rect, PixelRect::new(700, 600, 200, 100));
    let rect = drag(&mut tool, (600.0, 500.0), (900.0, 790.0), false, &set);
    assert_eq!(rect, PixelRect::new(600, 500, 300, 290));
    tool.cycle_ratio(&set, true);
    // Up and to the left with Shift: the room above (100 px) limits the 4:3 rectangle.
    let rect = drag(&mut tool, (950.0, 100.0), (550.0, 50.0), true, &set);
    assert_eq!((rect.right(), rect.bottom()), (950, 100));
    assert_eq!(rect.size(), (133, 100));
}

#[test]
fn the_grid_keeps_the_top_left_corner_on_block_lines() {
    let set = ratios(&["1:1"]);
    let mut tool = CropTool::new((640, 480), (640, 480), Some((16, 16)));
    let rect = drag(&mut tool, (0.0, 0.0), (37.0, 21.0), false, &set);
    assert_eq!((rect.x, rect.y), (32, 16));
    assert_eq!((rect.right(), rect.bottom()), (640, 480));
    let rect = drag(&mut tool, (300.0, 300.0), (309.0, 305.0), false, &set);
    assert_eq!((rect.x % 16, rect.y % 16), (0, 0));
    assert_eq!(rect.size(), (608, 464));
    tool.cycle_ratio(&set, true);
    assert_eq!((tool.rect().x % 16, tool.rect().y % 16), (0, 0));
}

#[test]
fn a_picture_smaller_than_the_minimum_can_still_be_cropped() {
    let set = ratios(&["1:1"]);
    let mut tool = tool((5, 3));
    let rect = drag(&mut tool, (5.0, 3.0), (0.0, 0.0), false, &set);
    assert_eq!(rect, PixelRect::full((5, 3)));
    let rect = drag(&mut tool, (5.0, 3.0), (0.0, 0.0), true, &set);
    assert!(rect.fits_in((5, 3)));
}

#[test]
fn every_result_lies_inside_the_picture() {
    let set = ratios(&["16:9", "9:16", "1:1", "21:9"]);
    let points = [-50.0, 0.0, 3.3, 77.7, 199.5, 250.0, 333.0, 1000.0];
    for grid in [None, Some((8, 8)), Some((16, 16))] {
        for &(sx, sy) in &[(0.0, 0.0), (333.0, 250.0), (166.0, 125.0), (333.0, 0.0), (10.0, 125.0)] {
            for &x in &points {
                for &y in &points {
                    for shift in [false, true] {
                        let mut tool = CropTool::new((333, 250), (333, 250), grid);
                        tool.cycle_ratio(&set, shift);
                        let rect = drag(&mut tool, (sx, sy), (x, y), shift, &set);
                        assert!(
                            rect.fits_in((333, 250)),
                            "{rect:?} from {sx},{sy} to {x},{y} shift {shift} grid {grid:?}"
                        );
                    }
                }
            }
        }
    }
}
