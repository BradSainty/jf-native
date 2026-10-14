//! JF Native: the Home rows' AT-REST poster lookahead. While Home rests, make the posters of the
//! rows just below the ones on screen resident, so scrolling down finds them loaded instead of
//! popping in card by card.
//!
//! A shelf asks for a card's art when the card is first drawn, and its own warm-ahead
//! (`Shelf::warm_ahead`) looks only along the focused row. A row below the screen therefore
//! started every fetch at the moment it scrolled in. This is the Library grid's lookahead
//! (`library/lookahead.rs`) turned on its side, under the same rules, each of which keeps the
//! page able to go idle:
//! - only a page at rest asks ([`Rest`]): the dive and the vertical scroll have both settled and
//!   their targets have held still for [`SETTLE_FRAMES`] frames, which lets the cards on screen
//!   claim their own art first;
//! - only a quiet source is asked (`tex::source_idle`), so a lookahead never competes with a
//!   visible card;
//! - at most one card per frame, nearest first, and the walk ends at the first card the source
//!   accepted or refused;
//! - the set is at most [`AHEAD_SET_MAX`] cards, what the source's lookahead store is sized
//!   against.
//!
//! The cards warmed in a row are the ones that row would paint at its current horizontal scroll,
//! through the key the card's draw resolves (`widgets::card_art_request`), so a later draw is a
//! hit. The hubs store holds the cards of two rows either side of the screen
//! (`cards::hold_band`), which is why [`ROWS_AHEAD`] is two: a row further down has no cards yet.
use std::ops::Range;

use plx_ui::cards::{CardSource, RowStyle};
use plx_ui::consts::SCR_W;
use plx_ui::tex::{Warm, AHEAD_SET_MAX};

use super::{Cx, HomeScreen};
use crate::registry::HomeLike;

/// Rows below the lowest one on screen kept warm. One row above the highest is kept as well.
pub(super) const ROWS_AHEAD: usize = 2;
/// Frames the dive and scroll targets must hold still before Home counts as at rest.
const SETTLE_FRAMES: u8 = 2;

/// Whether Home is at rest. Render history of the page, not logical state: it is never
/// canonised, restored or compared.
#[derive(Default)]
pub(super) struct Rest {
    targets: Option<(f32, f32)>,
    still: u8,
}

impl Rest {
    /// One call per prepared frame with the dive and the vertical scroll, each as
    /// `(pos, vel, target)`. A moved target restarts the count; the first sight is not a move.
    pub(super) fn observe(&mut self, snap: (f32, f32, f32), scroll: (f32, f32, f32)) -> bool {
        let targets = (snap.2, scroll.2);
        match self.targets {
            Some(last) if last != targets => self.still = 0,
            Some(_) => self.still = self.still.saturating_add(1),
            None => {}
        }
        self.targets = Some(targets);
        self.still >= SETTLE_FRAMES
            && plx_machine::idle::settled(snap.0, snap.2, snap.1)
            && plx_machine::idle::settled(scroll.0, scroll.2, scroll.1)
    }
}

/// The rows to keep warm, nearest first: [`ROWS_AHEAD`] below `on_screen`'s last row, then the
/// one above its first, of `rows` rows. With nothing on screen (the billboard, where only the
/// first row peeks in) the first row counts as the one on screen.
pub(super) fn candidate_rows(on_screen: Option<(usize, usize)>, rows: usize) -> impl Iterator<Item = usize> {
    let (first, last) = on_screen.unwrap_or((0, 0));
    let below = (1..=ROWS_AHEAD).map(move |d| last.checked_add(d));
    below.chain(std::iter::once(first.checked_sub(1))).flatten().filter(move |&row| row < rows)
}

/// The cards a row of `len` cards in `style` paints when scrolled to `scroll_x`: every card any
/// of which shows.
pub(super) fn painted(style: &RowStyle, scroll_x: f32, len: usize) -> Range<usize> {
    let pitch = style.w + style.gap;
    if pitch <= 0.0 {
        return 0..0;
    }
    let first = ((scroll_x - style.margin_x + style.gap) / pitch).floor().max(0.0) as usize;
    let past = ((scroll_x + SCR_W - style.margin_x) / pitch).ceil().max(0.0) as usize;
    first.min(len)..past.min(len)
}

impl HomeScreen {
    /// Called every prepared frame. Once Home has rested and the poster source is quiet it warms
    /// ONE not-yet-held poster of the rows just off screen, nearest row first.
    pub(super) fn prepare_row_ahead<H: HomeLike>(&mut self, cx: &Cx<'_, H>) {
        let resting = self.row_ahead.observe(
            (self.snap.pos, self.snap.vel, self.snap_target),
            (self.grid.scroll_y.pos, self.grid.scroll_y.vel, self.grid.scroll_target),
        );
        if !resting || self.covered || self.rows.is_empty() || !plx_ui::tex::source_idle() {
            return;
        }
        let on_screen = if self.snap.pos <= 0.5 {
            None
        } else {
            (0..self.grid.shelves.len())
                .filter(|&row| self.shelf_on_screen(row))
                .fold(None, |band: Option<(usize, usize)>, row| {
                    Some(band.map_or((row, row), |(lo, hi)| (lo.min(row), hi.max(row))))
                })
        };
        let view = H::hubs(cx);
        let mut asked = 0;
        for row in candidate_rows(on_screen, self.grid.shelves.len()) {
            let shelf = &self.grid.shelves[row];
            let src = self.cards(view, row);
            for i in painted(shelf.style(), shelf.scroll(), CardSource::<H>::len(&src)) {
                if asked == AHEAD_SET_MAX {
                    return;
                }
                asked += 1;
                if !CardSource::<H>::loaded(&src, i) {
                    continue;
                }
                let art = CardSource::<H>::art(&src, i);
                let Some((srv, path, w, h)) = plx_ui::widgets::card_art_request(&art) else { continue };
                if plx_ui::tex::warm_ahead_on(srv, path, w, h, false) != Warm::Known {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "row_ahead_tests.rs"]
mod tests;
