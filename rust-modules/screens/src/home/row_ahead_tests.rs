use super::*;

fn style() -> RowStyle {
    RowStyle::HOME
}

#[test]
fn the_rows_below_the_screen_come_first_then_the_one_above() {
    let rows: Vec<usize> = candidate_rows(Some((3, 4)), 10).collect();
    assert_eq!(rows, vec![5, 6, 2]);
}

#[test]
fn the_billboard_warms_the_rows_under_the_peeking_first_row() {
    let rows: Vec<usize> = candidate_rows(None, 10).collect();
    assert_eq!(rows, vec![1, 2]);
}

#[test]
fn no_row_past_the_last_is_named() {
    let rows: Vec<usize> = candidate_rows(Some((8, 9)), 10).collect();
    assert_eq!(rows, vec![7]);
    assert_eq!(candidate_rows(Some((0, 0)), 1).count(), 0);
}

#[test]
fn a_row_at_its_start_paints_from_its_first_card() {
    let s = style();
    let run = painted(&s, 0.0, 40);
    assert_eq!(run.start, 0);
    assert!(run.end > 1 && run.end < 40, "{run:?}");
}

#[test]
fn a_scrolled_row_paints_the_cards_its_scroll_shows() {
    let s = style();
    let pitch = s.w + s.gap;
    // card 5's left edge at the panel's left edge: card 4 is wholly off screen
    let run = painted(&s, pitch * 5.0 + s.margin_x, 40);
    assert_eq!(run.start, 5);
    // a row scrolled by whole cards still shows the end of the one before in its margin
    assert_eq!(painted(&s, pitch * 5.0, 40).start, if s.margin_x > s.gap { 4 } else { 5 });
}

#[test]
fn a_short_row_paints_no_card_it_does_not_have() {
    let s = style();
    assert_eq!(painted(&s, 0.0, 3), 0..3);
    assert_eq!(painted(&s, 0.0, 0), 0..0);
}

#[test]
fn rest_needs_still_targets_for_the_settle_frames() {
    let mut r = Rest::default();
    let still = (1.0, 0.0, 1.0);
    assert!(!r.observe(still, (0.0, 0.0, 0.0)));
    assert!(!r.observe(still, (0.0, 0.0, 0.0)));
    assert!(r.observe(still, (0.0, 0.0, 0.0)));
    // a moved scroll target restarts the count
    assert!(!r.observe(still, (0.0, 0.0, 300.0)));
}
