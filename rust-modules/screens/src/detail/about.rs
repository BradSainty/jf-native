//! The Detail page's About footer.
//!
//! Only the synopsis card and Languages column are controls. Information and Accessibility remain
//! readable content, not dead focus stops.
//!
//! **JF Native draws the whole footer in one visual language** (the owner's consistency pass): every
//! block is a bordered navy PLATE (`theme::INFO_TABLE_FILL` / `INFO_TABLE_RULE`), and every fact in
//! one is a ROW — a bold label on the left, its value right-aligned, rows ruled apart. The blocks sit
//! on one grid between the page margins, on whole pixels, and **every plate in a row is as tall as
//! the tallest** — the rows' contents differ, the boxes do not:
//!
//! ```text
//! About
//! [ synopsis (MORE)               ] [ Genres / Director / Writers / Studio ]
//! [ Released / Run Time ] [ Original Audio / Audio ] [ CC / SDH / AD       ]
//! ```
//!
//! No heading floats above a plate: each row names its own fact, and a plate with nothing to say
//! names its topic beside a dash. With no credits, the synopsis takes row 1 alone.

use std::ffi::CString;

use plx_data::metadata::Detail;
use plx_machine::machine::{GroupId, Measure};
use plx_ui::consts::{MARGIN_X, SCR_W};
use plx_ui::text_lift::{draw_focused, TextLift, CENTRE};
use plx_ui::text_view::TextView;
use plx_ui::{theme, Painter, Rect};

pub const ABOUT_ELEM_RANGE_START: u32 = 4 * super::SECTION_BLOCK;
pub const ABOUT_ELEM_RANGE_END: u32 = 5 * super::SECTION_BLOCK;
pub const ABOUT_GROUP: GroupId = GroupId(5);
pub const CARD_ELEM: u32 = ABOUT_ELEM_RANGE_START;
pub const LANGUAGES_ELEM: u32 = ABOUT_ELEM_RANGE_START + 1;

/// How many audio tracks the Languages column names. A summary, not the whole list: the column is
/// a control (see [`locate`]) whose MORE opens the tracks panel, which lists EVERY audio track in
/// the server's order (`tracks_panel::audio_rows`), so a track past the summary is one press away.
const AUDIO_SUMMARY: usize = 8;

// ---- the grid ------------------------------------------------------------------------------
/// The first row of plates, below the section's "About" heading.
const PLATE_Y: f32 = 50.0;
const GRID_W: f32 = SCR_W - 2.0 * MARGIN_X;
/// Air between two plates, across and down — the design system's region gap.
const GAP: f32 = theme::space::LG;
/// Row 1's two plates (the synopsis card, the credits table).
const HALF_W: f32 = (GRID_W - GAP) / 2.0;

/// Row 2's column `col` (0 Information, 1 Languages, 2 Accessibility): its left edge and width.
/// A third of the grid is not a whole pixel, and a hairline border on a fractional edge reads
/// soft beside the integer ones, so the edges are rounded and the remainder lands in a width.
fn third(col: usize) -> (f32, f32) {
    let edge = |i: usize| (i as f32 * (GRID_W + GAP) / 3.0).round();
    (MARGIN_X + edge(col), edge(col + 1) - edge(col) - GAP)
}

// ---- the plate and its rows ----------------------------------------------------------------
/// Every plate's corner, and the `ui::text_lift` radius of the two that lift on focus.
const RADIUS: f32 = 14.0;
const PAD_X: f32 = 24.0;
const PAD_Y: f32 = 16.0;
const LEAD: f32 = 30.0;

// ---- the synopsis card ---------------------------------------------------------------------
/// Air between the synopsis' last line and the MORE pinned on it — the same air the Person bio
/// and the Collection summary keep before theirs.
const MORE_GAP: f32 = theme::space::LG;
/// The About card's synopsis measure beside the credits table. The card inset is a row's, so
/// the synopsis' first line sits level with the table's first row.
#[cfg(test)]
const SYNOPSIS_W: f32 = HALF_W - 2.0 * PAD_X;

/// The About card's synopsis: ONE builder for the height `card_rect` reserves and the block
/// `draw` paints, so the two cannot disagree about the wrap.
fn synopsis<'a>(summary: &'a str, measure: &'a dyn Measure) -> TextView<'a> {
    TextView::new(summary, theme::size::CAPTION, theme::TEXT_HEADING)
        .with_measure(measure)
        .leading(LEAD)
        .max_lines(5)
        .fade_for_more(MORE_GAP)
        .mark_always()
}

pub fn locate(key: u32, tracks_available: bool) -> Option<usize> {
    match key {
        CARD_ELEM => Some(0),
        LANGUAGES_ELEM if tracks_available => Some(1),
        _ => None,
    }
}

/// One fact of a plate: its label, its value, and how many lines the value may wrap to.
type Fact<'a> = (&'a str, &'a str, usize);

/// A plate's label column: its widest label plus the air before the values, so no label (a long
/// one is "Regions of Origin") runs into its value — and never more than half the plate's measure.
fn label_w(facts: &[Fact<'_>], w: f32, measure: &dyn Measure) -> f32 {
    let widest = facts
        .iter()
        .map(|(label, _, _)| measure.width_str(label, theme::size::CAPTION, true))
        .fold(0.0, f32::max);
    (widest.ceil() + PAD_X).min((w - 2.0 * PAD_X) / 2.0)
}

fn value_view<'a>(value: &'a str, lines: usize, measure: &'a dyn Measure) -> TextView<'a> {
    TextView::new(value, theme::size::CAPTION, theme::TEXT_SECONDARY)
        .with_measure(measure)
        .leading(LEAD)
        .max_lines(lines)
        .h(plx_ui::label::HAlign::Right)
}

fn fact_h((_, value, lines): Fact<'_>, value_w: f32, measure: &dyn Measure) -> f32 {
    2.0 * PAD_Y + value_view(value, lines, measure).measure_h(value_w).max(LEAD)
}

fn facts_h(facts: &[Fact<'_>], w: f32, measure: &dyn Measure) -> f32 {
    let value_w = w - 2.0 * PAD_X - label_w(facts, w, measure);
    facts.iter().map(|f| fact_h(*f, value_w, measure)).sum()
}

/// The plate itself: the rule colour behind, the fill inset by a hairline — its border.
fn plate(p: Painter, r: Rect) {
    p.rect(r, RADIUS, theme::INFO_TABLE_RULE, theme::INFO_TABLE_RULE, 0.0);
    p.rect(r.inset(1.0), RADIUS - 1.0, theme::INFO_TABLE_FILL, theme::INFO_TABLE_FILL, 0.0);
}

/// A hairline rule across plate `r` at `y`, between two of its rows.
fn rule(p: Painter, r: Rect, y: f32) {
    p.rect(Rect::new(r.x + 1.0, y, r.w - 2.0, 1.0), 0.0, theme::INFO_TABLE_RULE, theme::INFO_TABLE_RULE, 0.0);
}

/// `facts` as rows from the top of plate `r`; returns the rows' height.
fn draw_facts(p: Painter, r: Rect, facts: &[Fact<'_>], measure: &dyn Measure) -> f32 {
    let label_w = label_w(facts, r.w, measure);
    let value_w = r.w - 2.0 * PAD_X - label_w;
    let mut y = r.y;
    for (i, fact) in facts.iter().enumerate() {
        let (label, value, lines) = *fact;
        if i > 0 {
            rule(p, r, y);
        }
        TextView::new(label, theme::size::CAPTION, theme::TEXT_PRIMARY)
            .bold()
            .with_measure(measure)
            .leading(LEAD)
            .max_lines(1)
            .draw(p, Rect::new(r.x + PAD_X, y + PAD_Y, label_w, 0.0));
        value_view(value, lines, measure).draw(p, Rect::new(r.x + PAD_X + label_w, y + PAD_Y, value_w, 0.0));
        y += fact_h(*fact, value_w, measure);
    }
    y - r.y
}

/// A plate with nothing to say still stands, naming its topic beside one dash — so a row of three
/// never has a hole, and the empty one still says what it is about.
fn nothing(topic: &str) -> Fact<'_> {
    (topic, "\u{2014}", 1)
}

/// The Languages plate's MORE row, below its facts: the affordance of a pressable plate.
const MORE_ROW_H: f32 = 2.0 * PAD_Y + LEAD;

/// Where the Languages plate's MORE is drawn (`Painter::text`'s `y`): in the plate's foot row.
fn more_y(plate: Rect) -> f32 {
    plate.y + plate.h - PAD_Y - theme::size::CAPTION as f32
}

pub struct Rows {
    dirty: bool,
    identity: Option<(plx_plex::plex::ServerId, String)>,
    info: Vec<(&'static str, String)>,
    orig_audio: Option<String>,
    audio_list: String,
    access: Vec<(&'static str, &'static str)>,
    /// The credits table's rows: label, value, and how many lines the value may wrap to.
    credits: Vec<(&'static str, String, usize)>,
}

/// Every name in `crew` whose job is `job`, once each, in the server's order.
fn crew_named(d: &Detail, job: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for credit in d.crew.iter().filter(|credit| credit.role.contains(job)) {
        if !out.contains(&credit.tag) {
            out.push(credit.tag.clone());
        }
    }
    out
}

impl Rows {
    pub fn new() -> Self {
        Self {
            dirty: true,
            identity: None,
            info: Vec::new(),
            orig_audio: None,
            audio_list: String::new(),
            access: Vec::new(),
            credits: Vec::new(),
        }
    }

    fn build_credits(&mut self, d: &Detail) {
        use plx_platform::i18n::msg;
        self.credits.clear();
        if !d.genres.is_empty() {
            self.credits.push((msg::browse_detail_info_genres(), d.genres.join(", "), 1));
        }
        let directors = if d.directors.is_empty() { crew_named(d, "Director") } else { d.directors.clone() };
        if !directors.is_empty() {
            let label = if directors.len() == 1 { msg::browse_crew_director() } else { msg::browse_detail_info_directors() };
            self.credits.push((label, directors.join(", "), 1));
        }
        let writers = crew_named(d, "Writer");
        if !writers.is_empty() {
            let label = if writers.len() == 1 { msg::browse_crew_writer() } else { msg::browse_detail_info_writers() };
            self.credits.push((label, writers.join(", "), 2));
        }
        if !d.studio.is_empty() {
            self.credits.push((msg::browse_detail_info_studio(), d.studio.clone(), 1));
        }
    }

    fn credit_facts(&self) -> Vec<Fact<'_>> {
        self.credits.iter().map(|(label, value, lines)| (*label, value.as_str(), *lines)).collect()
    }

    fn info_facts(&self) -> Vec<Fact<'_>> {
        let facts: Vec<Fact<'_>> = self.info.iter().map(|(label, value)| (*label, value.as_str(), 2)).collect();
        if facts.is_empty() { vec![nothing(plx_platform::i18n::msg::browse_detail_information())] } else { facts }
    }

    fn language_facts(&self) -> Vec<Fact<'_>> {
        use plx_platform::i18n::msg;
        let mut facts: Vec<Fact<'_>> = Vec::new();
        if let Some(orig) = &self.orig_audio {
            facts.push((msg::browse_detail_original_audio(), orig.as_str(), 1));
        }
        if !self.audio_list.is_empty() {
            facts.push((msg::browse_detail_audio(), self.audio_list.as_str(), 4));
        }
        if facts.is_empty() { vec![nothing(msg::browse_detail_languages())] } else { facts }
    }

    fn access_facts(&self) -> Vec<Fact<'_>> {
        let facts: Vec<Fact<'_>> = self.access.iter().map(|(label, desc)| (*label, *desc, 2)).collect();
        if facts.is_empty() { vec![nothing(plx_platform::i18n::msg::browse_detail_accessibility())] } else { facts }
    }

    /// The synopsis card's width: half the grid beside a credits table, the whole of it alone.
    fn card_w(&self) -> f32 {
        if self.credits.is_empty() { GRID_W } else { HALF_W }
    }

    /// Row 1's height: the taller of the card's own content and the credits table's rows.
    fn row1_h(&self, d: &Detail, measure: &dyn Measure) -> f32 {
        let card = 2.0 * PAD_Y + synopsis(&d.summary, measure).measure_h(self.card_w() - 2.0 * PAD_X).max(LEAD);
        if self.credits.is_empty() {
            card
        } else {
            card.max(facts_h(&self.credit_facts(), HALF_W, measure))
        }
    }

    /// Where the credits table stands for an About section whose heading is at `top`, or `None`
    /// when the item names none of its rows.
    pub fn credits_rect(&self, d: &Detail, top: f32, measure: &dyn Measure) -> Option<Rect> {
        (!self.credits.is_empty())
            .then(|| Rect::new(MARGIN_X + HALF_W + GAP, top + PLATE_Y, HALF_W, self.row1_h(d, measure)))
    }

    pub fn update(&mut self, d: &Detail) {
        if !self.dirty
            && self
                .identity
                .as_ref()
                .is_some_and(|(sid, rk)| *sid == d.sid && rk == &d.rk)
        {
            return;
        }
        self.dirty = false;
        self.identity = Some((d.sid, d.rk.clone()));
        self.info.clear();
        self.build_credits(d);
        // the year alone, as the hero's facts row reads it (JF Native)
        let released = if d.year > 0 { d.year.to_string() } else { d.aired.get(..4).unwrap_or("").to_string() };
        if !released.is_empty() {
            self.info.push((plx_platform::i18n::msg::browse_detail_released(), released));
        }
        // A show's or season's `duration` is the agent's nominal per-episode figure, no episode's
        // (#814): only a leaf has a runtime to print.
        if plx_data::metadata::has_runtime(&d.kind) && !d.is_show && d.dur_ms > 0 {
            self.info.push((
                plx_platform::i18n::msg::browse_detail_runtime(),
                plx_ui::fmt::duration(d.dur_ms),
            ));
        }
        self.info.push((
            plx_platform::i18n::msg::browse_detail_rated(),
            if d.rating.is_empty() {
                plx_platform::i18n::msg::browse_detail_unrated().into()
            } else {
                d.rating.clone()
            },
        ));
        if !d.countries.is_empty() {
            self.info
                .push((plx_platform::i18n::msg::browse_detail_origins(), d.countries.join(", ")));
        }
        self.orig_audio = d.audio.first().map(|a| {
            if a.lang.is_empty() {
                plx_platform::i18n::msg::browse_detail_unknown().into()
            } else {
                a.lang.clone()
            }
        });
        self.audio_list = d
            .audio
            .iter()
            .take(AUDIO_SUMMARY)
            .map(|a| {
                let lang = if a.lang.is_empty() {
                    plx_platform::i18n::msg::browse_detail_unknown()
                } else {
                    &a.lang
                };
                format!("{} ({})", lang, a.codec.to_uppercase())
            })
            .collect::<Vec<_>>()
            .join(", ");
        self.access.clear();
        if !d.subs.is_empty() {
            self.access.push((
                plx_platform::i18n::msg::widgets_badge_cc(),
                plx_platform::i18n::msg::browse_detail_closed_captions(),
            ));
        }
        if d.subs.iter().any(|s| s.sdh) {
            self.access.push((plx_platform::i18n::msg::widgets_badge_sdh(), plx_platform::i18n::msg::browse_detail_sdh()));
        }
        if d.audio.iter().any(|a| a.ad) {
            self.access.push((
                plx_platform::i18n::msg::widgets_badge_ad(),
                plx_platform::i18n::msg::browse_detail_audio_description(),
            ));
        }
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub fn card_rect(&self, d: &Detail, top: f32, measure: &dyn Measure) -> Rect {
        Rect::new(MARGIN_X, top + PLATE_Y, self.card_w(), self.row1_h(d, measure))
    }

    /// The top of row 2: under row 1's plates, which share one height.
    fn row2_top(&self, d: &Detail, top: f32, measure: &dyn Measure) -> f32 {
        top + PLATE_Y + self.row1_h(d, measure) + GAP
    }

    /// Row 2's height: the tallest of its three plates' rows — the Languages plate's including
    /// the MORE row the track sheet's affordance takes when there is a sheet to open (`tracks`).
    fn row2_h(&self, tracks: bool, measure: &dyn Measure) -> f32 {
        let more = if tracks { MORE_ROW_H } else { 0.0 };
        facts_h(&self.info_facts(), third(0).1, measure)
            .max(facts_h(&self.language_facts(), third(1).1, measure) + more)
            .max(facts_h(&self.access_facts(), third(2).1, measure))
    }

    /// Row 2's plate `col` (0 Information, 1 Languages, 2 Accessibility).
    fn column(&self, d: &Detail, top: f32, col: usize, tracks: bool, measure: &dyn Measure) -> Rect {
        let (x, w) = third(col);
        Rect::new(x, self.row2_top(d, top, measure), w, self.row2_h(tracks, measure))
    }

    /// The Languages plate (the only row-2 plate that is a focus stop, and only with `tracks`).
    pub fn languages_rect(&self, d: &Detail, top: f32, tracks: bool, measure: &dyn Measure) -> Rect {
        self.column(d, top, 1, tracks, measure)
    }

    /// `tracks` is the PAGE's answer to "is there a file for the track sheet to describe"
    /// (`DetailScreen::tracks_available`), passed in rather than asked of the sheet: it decides
    /// whether the Languages column carries a MORE affordance, and it has to be the same bit
    /// `about::locate` gates the element on or the column reads as pressable and is not.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        p: Painter,
        d: &Detail,
        top: f32,
        tracks: bool,
        card_lift: &TextLift,
        lang_lift: &TextLift,
        measure: &dyn plx_machine::machine::Measure,
    ) {
        text_at(
            p,
            MARGIN_X,
            top,
            theme::size::HEADLINE,
            theme::TEXT_PRIMARY,
            1,
            plx_platform::i18n::msg::browse_detail_about(),
        );
        let card = self.card_rect(d, top, measure);
        // The card's content grows with its plate inside `draw_focused`'s closure only; the
        // plates drawn after it use the caller's own `p` and cannot inherit the lift. (No title:
        // the page's own logo names the film a screen above, and no other plate has one.)
        draw_focused(p, card, RADIUS, card_lift, CENTRE, |p| {
            plate(p, card);
            let text = synopsis(&d.summary, measure);
            let text_w = card.w - 2.0 * PAD_X;
            let text_h = text.measure_h(text_w).max(LEAD);
            text.draw(p, Rect::new(card.x + PAD_X, card.y + PAD_Y, text_w, 0.0));
            // MORE reads on, so it sits ON the synopsis' last line (which fades under it) rather
            // than at the foot of a card stretched to the credits table's height
            p.text(
                plx_ui::text_view::more_mark().as_ptr(),
                card.x + card.w - PAD_X,
                card.y + PAD_Y + text_h - theme::size::CAPTION as f32,
                theme::size::CAPTION,
                theme::TEXT_TERTIARY,
                2,
                1,
            );
        });
        if let Some(table) = self.credits_rect(d, top, measure) {
            plate(p, table);
            draw_facts(p, table, &self.credit_facts(), measure);
        }

        let info = self.info_facts();
        let info_plate = self.column(d, top, 0, tracks, measure);
        plate(p, info_plate);
        draw_facts(p, info_plate, &info, measure);

        let langs = self.language_facts();
        let lang_plate = self.languages_rect(d, top, tracks, measure);
        draw_focused(p, lang_plate, RADIUS, lang_lift, CENTRE, |p| {
            plate(p, lang_plate);
            draw_facts(p, lang_plate, &langs, measure);
            if tracks {
                // MORE keeps its own row at the FOOT of the plate, wherever the facts end
                rule(p, lang_plate, lang_plate.y + lang_plate.h - MORE_ROW_H);
                p.text(
                    plx_ui::text_view::more_mark().as_ptr(),
                    lang_plate.x + lang_plate.w - PAD_X,
                    more_y(lang_plate),
                    theme::size::CAPTION,
                    theme::TEXT_TERTIARY,
                    2,
                    1,
                );
            }
        });

        let access = self.access_facts();
        let access_plate = self.column(d, top, 2, tracks, measure);
        plate(p, access_plate);
        draw_facts(p, access_plate, &access, measure);
    }
}

fn text_at(p: Painter, x: f32, y: f32, size: i32, color: [f32; 4], bold: i32, text: &str) -> f32 {
    CString::new(text)
        .ok()
        .map(|s| p.text(s.as_ptr(), x, y, size, color, 0, bold))
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file with twelve audio tracks: the column names the first eight and is a control, and the
    /// tracks panel it opens has a row for each of the twelve, the last one last.
    #[test]
    fn the_audio_summary_stops_at_eight_and_the_tracks_panel_lists_all_twelve() {
        use plx_data::metadata::Stream;
        let audio: Vec<Stream> = (0..12)
            .map(|i| Stream { lang: format!("Lang{i}"), codec: "ac3".into(), ..Default::default() })
            .collect();
        let d = Detail { part: "/library/parts/1/file.mkv".into(), audio, ..Default::default() };
        let mut rows = Rows::new();
        rows.update(&d);
        assert_eq!(rows.audio_list.split(", ").count(), AUDIO_SUMMARY);
        assert!(rows.audio_list.contains("Lang7") && !rows.audio_list.contains("Lang8"));
        assert_eq!(locate(LANGUAGES_ELEM, d.has_own_file()), Some(1), "the column opens the tracks panel");
        let panel = crate::registry::tracks_panel_audio_rows(&d.audio);
        assert_eq!(panel.len(), 12, "the panel lists every track");
        assert_eq!(panel[11].name, "Lang11");
    }

    fn crew(name: &str, job: &str) -> plx_data::metadata::Cast {
        plx_data::metadata::Cast { tag: name.into(), role: job.into(), thumb: String::new(), id: 0, tag_key: String::new() }
    }

    /// The credits table (JF Native): Genres, Director, Writers, Studio, each only when the item
    /// names it, the job's label singular or plural by count; and at its tallest it still ends
    /// above the Information/Languages columns, so it needs no room of its own in the flow.
    #[test]
    fn the_credits_table_names_the_crew_and_stays_above_the_columns() {
        let measure = plx_ui::fixture::FixtureMeasure;
        let d = Detail {
            genres: vec!["Animation".into(), "Family".into()],
            directors: vec!["Andrew Stanton".into()],
            crew: vec![
                crew("Andrew Stanton", "DirectorWriter"),
                crew("David Reynolds", "Writer"),
                crew("Bob Peterson", "Writer"),
            ],
            studio: "Pixar".into(),
            ..Default::default()
        };
        let mut rows = Rows::new();
        rows.update(&d);
        let got: Vec<(&str, &str)> = rows.credits.iter().map(|(l, v, _)| (*l, v.as_str())).collect();
        use plx_platform::i18n::msg;
        assert_eq!(got, vec![
            (msg::browse_detail_info_genres(), "Animation, Family"),
            (msg::browse_crew_director(), "Andrew Stanton"),
            (msg::browse_detail_info_writers(), "Andrew Stanton, David Reynolds, Bob Peterson"),
            (msg::browse_detail_info_studio(), "Pixar"),
        ]);
        // the tallest table: every value long enough to use every line it may
        let long = "Someone Withaverylongname, ".repeat(40);
        let mut tall = Rows::new();
        tall.update(&Detail {
            genres: vec![long.clone()],
            directors: vec![long.clone(), long.clone()],
            crew: vec![crew(&long, "Writer"), crew("Another", "Writer")],
            studio: long.clone(),
            ..Default::default()
        });
        // a short synopsis, so the long table is the taller of row 1's two
        let d_tall = Detail { summary: "word ".repeat(3), ..Default::default() };
        let table = tall.credits_rect(&d_tall, 0.0, &measure).expect("a table");
        assert!((table.x + table.w - (SCR_W - MARGIN_X)).abs() < 0.01, "the table ends on the right margin");
        assert_eq!((table.y, table.w), (PLATE_Y, HALF_W), "row 1's second plate, beside the card");
        assert!(table.h >= facts_h(&tall.credit_facts(), HALF_W, &measure), "every row fits the plate");
        // row 2 waits for row 1: the long table pushes the columns down
        assert!(tall.row2_top(&d_tall, 0.0, &measure) >= table.y + table.h + GAP);
        assert!(Rows::new().credits_rect(&d_tall, 0.0, &measure).is_none(), "nothing named: no table");
    }

    /// One grid: row 1's two plates share an edge pair and ONE height, row 2's three plates share
    /// one top and ONE height, all between the page margins on whole pixels, with the region gap
    /// between every pair.
    #[test]
    fn the_footer_is_one_grid_between_the_margins() {
        let measure = plx_ui::fixture::FixtureMeasure;
        let mut d = movie_with_audio();
        d.directors = vec!["Andrew Stanton".into()];
        d.studio = "Pixar".into();
        let mut rows = Rows::new();
        rows.update(&d);
        let card = rows.card_rect(&d, 0.0, &measure);
        let table = rows.credits_rect(&d, 0.0, &measure).expect("a table");
        assert_eq!((card.x, card.w), (MARGIN_X, HALF_W));
        assert_eq!((table.y, table.h), (card.y, card.h), "row 1's plates are one height");
        assert!((table.x - (card.x + card.w + GAP)).abs() < 0.01);
        for tracks in [true, false] {
            let lang = rows.languages_rect(&d, 0.0, tracks, &measure);
            let info = rows.column(&d, 0.0, 0, tracks, &measure);
            let access = rows.column(&d, 0.0, 2, tracks, &measure);
            assert_eq!(info.x, MARGIN_X);
            assert_eq!((info.y, info.h), (lang.y, lang.h), "row 2's plates are one height");
            assert_eq!((access.y, access.h), (lang.y, lang.h), "row 2's plates are one height");
            assert_eq!(lang.x - (info.x + info.w), GAP);
            assert_eq!(access.x - (lang.x + lang.w), GAP);
            assert_eq!(access.x + access.w, SCR_W - MARGIN_X);
            for r in [info, lang, access] {
                assert_eq!((r.x.fract(), r.w.fract()), (0.0, 0.0), "whole pixels: {r:?}");
            }
            assert_eq!(info.y, card.y + card.h + GAP, "row 2 sits a region gap under row 1");
        }
        // no credits: the synopsis takes row 1 alone rather than leaving its right half empty
        let mut bare = Rows::new();
        let d = movie_with_audio();
        bare.update(&d);
        let card = bare.card_rect(&d, 0.0, &measure);
        assert_eq!((card.x, card.w), (MARGIN_X, GRID_W));
    }

    /// A plate's label column is as wide as its widest label, so a long label never runs into its
    /// value; an empty plate names its topic.
    #[test]
    fn labels_fit_their_column_and_an_empty_plate_names_its_topic() {
        let measure = plx_ui::fixture::FixtureMeasure;
        let facts = [("Released", "1995", 1), ("Regions of Origin", "United States of America", 2)];
        let w = third(0).1;
        let widest = measure.width_str("Regions of Origin", theme::size::CAPTION, true);
        assert!(label_w(&facts, w, &measure) >= widest + PAD_X - 0.01);
        assert!(label_w(&facts, w, &measure) <= (w - 2.0 * PAD_X) / 2.0 + 0.01, "a label never takes most of the plate");
        let rows = Rows::new();
        use plx_platform::i18n::msg;
        assert_eq!(rows.access_facts()[0].0, msg::browse_detail_accessibility());
        assert_eq!(rows.info_facts()[0].0, msg::browse_detail_information());
        assert_eq!(rows.language_facts()[0].0, msg::browse_detail_languages());
    }

    /// Issue 15: the About card's MORE is pinned on the synopsis' last line whether or not the
    /// text was truncated, so a synopsis that fits in exactly its five lines with a long last
    /// line runs under MORE unless that line fades too.
    #[test]
    fn an_untruncated_last_line_reaching_under_more_fades() {
        let measure = plx_ui::fixture::FixtureMeasure;
        let fits = |n: usize| {
            let text = "word ".repeat(n);
            (!synopsis(&text, &measure).truncates(SYNOPSIS_W)).then_some(text)
        };
        let longest = (1..2000).map_while(fits).last().expect("some synopsis fits");
        let view = synopsis(&longest, &measure);
        let mark = measure.width(plx_ui::text_view::more_mark(), theme::size::CAPTION, true);
        assert!(!view.truncates(SYNOPSIS_W));
        assert!(view.last_line_w(SYNOPSIS_W) > SYNOPSIS_W - (mark + MORE_GAP),
            "precondition: the last line ends inside MORE's rect and its air");
        assert!(view.last_line_fades(SYNOPSIS_W), "text running under MORE must fade");
        let short = synopsis("A short synopsis.", &measure);
        assert!(!short.last_line_fades(SYNOPSIS_W), "a line clear of MORE stays plain");
    }

    /// The Languages plate's MORE is not the About card's shape: it has a row of its own below the
    /// facts, so even the longest audio list never reaches under it — and only a plate with a track
    /// sheet to open carries that row.
    #[test]
    fn the_languages_more_has_its_own_row_below_the_audio_list() {
        let measure = plx_ui::fixture::FixtureMeasure;
        let d = Detail::default();
        let mut rows = Rows::new();
        rows.audio_list = "English (AAC), ".repeat(60);
        let facts = facts_h(&rows.language_facts(), third(1).1, &measure);
        let plate = rows.languages_rect(&d, 0.0, true, &measure);
        assert!(more_y(plate) >= plate.y + facts,
            "MORE ({}) is below the last audio row ({})", more_y(plate), plate.y + facts);
        assert_eq!(rows.languages_rect(&d, 0.0, false, &measure).h, facts, "no sheet: no MORE row");
    }

    #[test]
    fn only_the_card_and_available_languages_are_focusable() {
        assert_eq!(locate(CARD_ELEM, false), Some(0));
        assert_eq!(locate(LANGUAGES_ELEM, false), None);
        assert_eq!(locate(LANGUAGES_ELEM, true), Some(1));
        assert_eq!(locate(ABOUT_ELEM_RANGE_START + 2, true), None);
    }

    #[test]
    fn about_focus_only_ever_lands_on_the_card_and_a_clickable_languages_column() {
        let without_tracks: Vec<_> = (ABOUT_ELEM_RANGE_START..ABOUT_ELEM_RANGE_END)
            .filter_map(|key| locate(key, false).map(|_| key))
            .collect();
        let with_tracks: Vec<_> = (ABOUT_ELEM_RANGE_START..ABOUT_ELEM_RANGE_END)
            .filter_map(|key| locate(key, true).map(|_| key))
            .collect();
        assert_eq!(without_tracks, vec![CARD_ELEM]);
        assert_eq!(with_tracks, vec![CARD_ELEM, LANGUAGES_ELEM]);
    }

    #[test]
    fn a_same_identity_metadata_landing_invalidates_cached_about_rows() {
        let mut rows = Rows::new();
        let first = Detail {
            sid: plx_plex::plex::ServerId::UNSET,
            rk: "movie".into(),
            rating: "PG".into(),
            dur_ms: 60_000,
            ..Default::default()
        };
        rows.update(&first);
        assert!(rows
            .info
            .iter()
            .any(|(label, value)| *label == "Rated" && value == "PG"));

        let second = Detail {
            rating: "R".into(),
            dur_ms: 120_000,
            ..first
        };
        rows.update(&second);
        assert!(
            rows.info
                .iter()
                .any(|(label, value)| *label == "Rated" && value == "PG"),
            "without a landing invalidation, repeated reads remain O(1)"
        );
        rows.invalidate();
        rows.update(&second);
        assert!(rows
            .info
            .iter()
            .any(|(label, value)| *label == "Rated" && value == "R"));
        assert!(rows
            .info
            .iter()
            .any(|(label, value)| *label == "Run Time" && value == "2m"));
    }

    fn movie_with_audio() -> Detail {
        Detail {
            sid: plx_plex::plex::ServerId::UNSET,
            rk: "movie".into(),
            rating: "PG".into(),
            summary: "word ".repeat(40),
            audio: vec![plx_data::metadata::Stream {
                lang: "en".into(),
                codec: "ac3".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn focused(elem_card: bool) -> (TextLift, TextLift) {
        let (mut card, mut lang) = (TextLift::new(), TextLift::new());
        for _ in 0..240 {
            card.step(elem_card, 1.0 / 60.0);
            lang.step(!elem_card, 1.0 / 60.0);
        }
        (card, lang)
    }

    fn census(rows: &Rows, d: &Detail, card: &TextLift, lang: &TextLift) -> Vec<(u64, Rect)> {
        let measure = plx_ui::fixture::FixtureMeasure;
        plx_ui::draw_census::capture(|| rows.draw(Painter::recording(), d, 0.0, true, card, lang, &measure))
    }

    /// A focused About card grows its own content and nothing else: the Information column, drawn
    /// after it, stays exactly where it does at rest.
    #[test]
    fn a_focused_about_card_does_not_scale_its_siblings() {
        let _g = plx_base::testlock::serial();
        let mut rows = Rows::new();
        let d = movie_with_audio();
        rows.update(&d);
        let at_rest = census(&rows, &d, &TextLift::new(), &TextLift::new());
        let (card, lang) = focused(true);
        assert!(card.scale() > 1.0, "precondition: the card is lifted");
        let lifted = census(&rows, &d, &card, &lang);

        // the Information plate
        let info = rows.column(&d, 0.0, 0, true, &plx_ui::fixture::FixtureMeasure);
        let at = |c: &[(u64, Rect)]| c.iter().find(|(_, r)| *r == info).map(|(_, r)| *r);
        assert!(at(&at_rest).is_some(), "precondition: the Information plate is in the census");
        assert_eq!(at(&lifted), at(&at_rest), "a sibling must not inherit the card's scale");
        // And the card's own content did move, so the test above is not vacuous.
        assert_ne!(lifted, at_rest);
    }

    /// Focus moving off a lifted block leaves nothing behind once it settles: the draw is exactly
    /// the never-focused draw, for either block.
    #[test]
    fn leaving_a_lifted_block_leaves_no_plate_or_shadow_once_settled() {
        let _g = plx_base::testlock::serial();
        let mut rows = Rows::new();
        let d = movie_with_audio();
        rows.update(&d);
        let at_rest = census(&rows, &d, &TextLift::new(), &TextLift::new());

        for on_card in [true, false] {
            let (mut card, mut lang) = focused(on_card);
            assert_ne!(census(&rows, &d, &card, &lang), at_rest, "precondition: focus draws something");
            for _ in 0..240 {
                card.step(false, 1.0 / 60.0);
                lang.step(false, 1.0 / 60.0);
            }
            assert_eq!(census(&rows, &d, &card, &lang), at_rest);
        }
    }

    /// #814: a show's `duration` is the metadata agent's nominal per-episode figure, so About
    /// prints no Runtime for a show or a season, and still prints a movie's or episode's.
    #[test]
    fn a_show_or_season_has_no_runtime_row_and_a_leaf_keeps_its_own() {
        let runtime = plx_platform::i18n::msg::browse_detail_runtime();
        let has = |d: &Detail| {
            let mut rows = Rows::new();
            rows.update(d);
            rows.info.iter().any(|(label, _)| *label == runtime)
        };
        let episode = plx_data::metadata::Episode { dur_ms: 45 * 60_000, ..Default::default() };
        let show = Detail { kind: "show".into(), is_show: true, dur_ms: 6_000_000, ..Default::default() };
        let season = Detail {
            kind: "season".into(),
            dur_ms: 6_000_000,
            episodes: vec![episode].into(),
            ..Default::default()
        };
        assert!(!has(&show), "a show's nominal duration is no episode's");
        assert!(!has(&season));
        for kind in ["movie", "episode"] {
            let leaf = Detail { kind: kind.into(), dur_ms: 6_000_000, ..Default::default() };
            assert!(has(&leaf), "{kind}");
        }
    }

}
