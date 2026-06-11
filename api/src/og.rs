//! Dynamic Open Graph cards.
//!
//! Show and skip-guide pages point their `og:image` at `/og/shows/{slug}.png`,
//! which renders a 1200×630 PNG: the show's poster plus a stat card ("X% filler
//! — skip N of M episodes") instead of the bare TMDB poster. The card is built
//! as an SVG string and rasterized with `resvg`; fonts are vendored (Inter, OFL
//! — see `assets/fonts/`) so rendering never depends on system fonts. Pure
//! functions except the poster fetch; the route handler lives in `main.rs`.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Context;
use base64::Engine;
use resvg::{tiny_skia, usvg};
use usvg::fontdb;

use crate::models::EpisodeItem;
use crate::scoring::EpisodeStatus;
use crate::seo::html_escape;

/// Card dimensions — the standard large Open Graph size.
pub const WIDTH: u32 = 1200;
pub const HEIGHT: u32 = 630;

// Brand palette, mirroring the SPA's Tailwind classes: zinc-950 background,
// emerald = canon, sky = worth watching, rose = filler / the "Killer" wordmark.
const BG_TOP: &str = "#0a0a0f";
const BG_BOTTOM: &str = "#18181b";
const TEXT: &str = "#f4f4f5"; // zinc-100
const TEXT_DIM: &str = "#d4d4d8"; // zinc-300
const TEXT_MUTED: &str = "#a1a1aa"; // zinc-400
const TEXT_FAINT: &str = "#71717a"; // zinc-500
const BORDER: &str = "#27272a"; // zinc-800
const CANON: &str = "#34d399"; // emerald-400
const WORTH: &str = "#38bdf8"; // sky-400
const FILLER: &str = "#fb7185"; // rose-400
const KILLER: &str = "#f43f5e"; // rose-500 (wordmark)

static INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.otf");
static INTER_BOLD: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.otf");

/// The vendored font database, built once. resvg resolves text against this
/// only — no system font lookup, so output is identical across environments.
fn font_db() -> Arc<fontdb::Database> {
    static DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_font_data(INTER_REGULAR.to_vec());
        db.load_font_data(INTER_BOLD.to_vec());
        Arc::new(db)
    })
    .clone()
}

/// Per-show episode-status counts feeding the stat card. Derived from the same
/// scored episodes the skip guide uses (specials excluded, like its default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OgStats {
    pub canon: u32,
    pub worth_watching: u32,
    pub filler: u32,
    /// Contested + not-enough-votes — episodes without a confident label yet.
    pub undecided: u32,
}

impl OgStats {
    pub fn total(&self) -> u32 {
        self.canon + self.worth_watching + self.filler + self.undecided
    }

    /// Whole-number filler percentage; 0 when there are no episodes (no
    /// division by zero).
    pub fn filler_pct(&self) -> u32 {
        match self.total() {
            0 => 0,
            total => ((self.filler as f64 / total as f64) * 100.0).round() as u32,
        }
    }
}

/// Tally episode statuses for the card. Season 0 (specials) is excluded to
/// match the skip guide's default view.
pub fn stats_from_episodes(episodes: &[EpisodeItem]) -> OgStats {
    let mut stats = OgStats::default();
    for e in episodes.iter().filter(|e| e.season_number != 0) {
        match e.score.status {
            EpisodeStatus::Canon => stats.canon += 1,
            EpisodeStatus::WorthWatching => stats.worth_watching += 1,
            EpisodeStatus::Filler => stats.filler += 1,
            EpisodeStatus::Contested | EpisodeStatus::NotEnoughVotes => stats.undecided += 1,
        }
    }
    stats
}

/// A fetched poster image, ready to embed as a data URI.
pub struct Poster {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// Fetch the show poster for embedding. Strictly best-effort: any failure
/// (timeout, non-2xx, oversized body) logs and returns `None`, and the card
/// degrades to the text-only layout — the endpoint itself never fails on TMDB.
pub async fn fetch_poster(http: &reqwest::Client, url: &str) -> Option<Poster> {
    let res = http
        .get(url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| tracing::warn!("og poster fetch failed: {e}"))
        .ok()?;
    let bytes = res
        .bytes()
        .await
        .map_err(|e| tracing::warn!("og poster body read failed: {e}"))
        .ok()?;
    // TMDB w342 posters are tens of KB; anything huge is wrong — skip it.
    if bytes.is_empty() || bytes.len() > 2 * 1024 * 1024 {
        tracing::warn!("og poster has unreasonable size ({} bytes); skipping", bytes.len());
        return None;
    }
    // Sniff the magic bytes — TMDB serves JPEG, but don't trust the extension.
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    Some(Poster {
        bytes: bytes.to_vec(),
        mime,
    })
}

/// Rough text width in px for layout (Inter averages ~0.56em per glyph). Only
/// used to choose wrap points and breakdown-row spacing, so precision doesn't
/// matter — SVG text renders at its true width regardless.
fn est_width(text: &str, font_size: f32) -> f32 {
    text.chars().count() as f32 * font_size * 0.56
}

/// Greedy word-wrap into at most `max_lines` lines of at most `max_chars`
/// chars, ellipsizing on overflow (including single words longer than a line).
fn wrap_ellipsize(text: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    let max_chars = max_chars.max(2);
    let mut lines: Vec<String> = vec![String::new()];
    let mut truncated = false;
    for word in text.split_whitespace() {
        let cur = lines.last_mut().expect("at least one line");
        let sep = usize::from(!cur.is_empty());
        if cur.chars().count() + sep + word.chars().count() <= max_chars {
            if sep == 1 {
                cur.push(' ');
            }
            cur.push_str(word);
        } else if cur.is_empty() {
            // A single word longer than a whole line: keep it on this line
            // (hard-cut below) rather than leaving the line blank.
            cur.push_str(word);
        } else if lines.len() < max_lines {
            lines.push(word.to_string());
        } else {
            truncated = true;
            break;
        }
    }
    // Hard-cut any line still over budget (a single overlong word).
    let last_idx = lines.len() - 1;
    for (i, line) in lines.iter_mut().enumerate() {
        if line.chars().count() > max_chars {
            *line = line.chars().take(max_chars - 1).collect();
            line.push('…');
            truncated = i == last_idx || truncated;
        }
    }
    if truncated {
        let last = lines.last_mut().expect("at least one line");
        if !last.ends_with('…') {
            // Make room for the ellipsis within the budget.
            while last.chars().count() >= max_chars {
                last.pop();
            }
            *last = format!("{}…", last.trim_end());
        }
    }
    lines
}

/// Lay out the breakdown row (label + dot color per status) to fit `avail`
/// pixels: drop the optional "undecided" item first, then shrink the font.
/// Returns the items and the font size to draw them at.
fn breakdown_layout(stats: &OgStats, avail: f32) -> (Vec<(String, &'static str)>, f32) {
    const DOT_AND_GAP: f32 = 24.0 + 36.0; // dot + label inset, then inter-item gap
    let mut items: Vec<(String, &'static str)> = vec![
        (format!("{} canon", stats.canon), CANON),
        (format!("{} worth it", stats.worth_watching), WORTH),
        (format!("{} filler", stats.filler), FILLER),
    ];
    if stats.undecided > 0 {
        items.push((format!("{} undecided", stats.undecided), TEXT_FAINT));
    }
    let row_width = |items: &[(String, &'static str)], size: f32| {
        items
            .iter()
            .map(|(label, _)| DOT_AND_GAP + est_width(label, size))
            .sum::<f32>()
            - 36.0
    };
    let mut size = 26.0;
    if row_width(&items, size) > avail && items.len() == 4 {
        items.pop(); // least important; the three core counts stay
    }
    let w = row_width(&items, size);
    if w > avail {
        size = (size * avail / w).max(16.0);
    }
    (items, size)
}

fn plural(n: u32, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// Build the card SVG. `poster` embeds as a rounded-corner image on the left;
/// without one the text takes the full width. All dynamic text is XML-escaped.
pub fn og_svg(name: &str, stats: &OgStats, poster: Option<&Poster>) -> String {
    let right_edge = (WIDTH - 64) as f32;
    let text_x = if poster.is_some() { 460.0 } else { 64.0 };
    let avail = right_edge - text_x;

    let mut s = String::with_capacity(4096);
    s.push_str(&format!(
        "<svg width=\"{WIDTH}\" height=\"{HEIGHT}\" viewBox=\"0 0 {WIDTH} {HEIGHT}\" \
         xmlns=\"http://www.w3.org/2000/svg\">"
    ));
    s.push_str(&format!(
        "<defs>\
         <linearGradient id=\"bg\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\">\
         <stop offset=\"0\" stop-color=\"{BG_TOP}\"/>\
         <stop offset=\"1\" stop-color=\"{BG_BOTTOM}\"/>\
         </linearGradient>\
         <clipPath id=\"poster\"><rect x=\"48\" y=\"48\" width=\"356\" height=\"534\" rx=\"20\"/></clipPath>\
         </defs>"
    ));
    s.push_str(&format!("<rect width=\"{WIDTH}\" height=\"{HEIGHT}\" fill=\"url(#bg)\"/>"));
    // Top accent bar in the three vote colors.
    s.push_str(&format!("<rect x=\"0\" y=\"0\" width=\"400\" height=\"8\" fill=\"{CANON}\"/>"));
    s.push_str(&format!("<rect x=\"400\" y=\"0\" width=\"400\" height=\"8\" fill=\"{WORTH}\"/>"));
    s.push_str(&format!("<rect x=\"800\" y=\"0\" width=\"400\" height=\"8\" fill=\"{KILLER}\"/>"));

    if let Some(p) = poster {
        let data = base64::engine::general_purpose::STANDARD.encode(&p.bytes);
        s.push_str(&format!(
            "<image x=\"48\" y=\"48\" width=\"356\" height=\"534\" \
             preserveAspectRatio=\"xMidYMid slice\" clip-path=\"url(#poster)\" \
             href=\"data:{};base64,{data}\"/>",
            p.mime
        ));
        s.push_str(&format!(
            "<rect x=\"48\" y=\"48\" width=\"356\" height=\"534\" rx=\"20\" \
             fill=\"none\" stroke=\"{BORDER}\" stroke-width=\"2\"/>"
        ));
    }

    // Show name: size steps down for longer names, wraps to two lines max.
    let name_chars = name.chars().count();
    let title_size: f32 = if name_chars <= 18 {
        64.0
    } else if name_chars <= 44 {
        52.0
    } else {
        44.0
    };
    let max_chars = (avail / (title_size * 0.56)) as usize;
    let title_lines = wrap_ellipsize(name, max_chars, 2);
    let mut y = 150.0;
    for line in &title_lines {
        s.push_str(&format!(
            "<text x=\"{text_x}\" y=\"{y}\" font-family=\"Inter\" font-weight=\"bold\" \
             font-size=\"{title_size}\" fill=\"{TEXT}\">{}</text>",
            html_escape(line)
        ));
        y += title_size * 1.2;
    }

    let total = stats.total();
    if total == 0 {
        // Zero-episode show: a clean placeholder instead of "0% filler".
        s.push_str(&format!(
            "<text x=\"{text_x}\" y=\"400\" font-family=\"Inter\" font-size=\"40\" \
             fill=\"{TEXT_MUTED}\">No episodes tracked yet</text>"
        ));
    } else {
        s.push_str(&format!(
            "<text x=\"{text_x}\" y=\"408\" font-family=\"Inter\" font-weight=\"bold\" \
             font-size=\"88\" fill=\"{FILLER}\">{}% filler</text>",
            stats.filler_pct()
        ));
        s.push_str(&format!(
            "<text x=\"{text_x}\" y=\"466\" font-family=\"Inter\" font-size=\"34\" \
             fill=\"{TEXT_DIM}\">Skip {} of {} {}</text>",
            stats.filler,
            total,
            plural(total, "episode")
        ));

        // Breakdown row: colored dot + count per status, fitted to the column.
        let (items, row_size) = breakdown_layout(stats, avail);
        let row_y = 524.0;
        let mut x = text_x;
        for (label, color) in items {
            s.push_str(&format!(
                "<circle cx=\"{:.0}\" cy=\"{:.0}\" r=\"7\" fill=\"{color}\"/>",
                x + 7.0,
                row_y - 9.0
            ));
            s.push_str(&format!(
                "<text x=\"{:.0}\" y=\"{row_y}\" font-family=\"Inter\" font-size=\"{row_size}\" \
                 fill=\"{TEXT_DIM}\">{}</text>",
                x + 24.0,
                html_escape(&label)
            ));
            x += 24.0 + est_width(&label, row_size) + 36.0;
        }
    }

    // Wordmark, bottom-right, matching the SPA's white-Filler / rose-Killer mark.
    s.push_str(&format!(
        "<text x=\"{right_edge}\" y=\"580\" text-anchor=\"end\" font-family=\"Inter\" \
         font-weight=\"bold\" font-size=\"30\">\
         <tspan fill=\"{TEXT}\">filler</tspan>\
         <tspan fill=\"{KILLER}\">killer</tspan>\
         <tspan fill=\"{TEXT_FAINT}\">.app</tspan>\
         </text>"
    ));
    s.push_str("</svg>");
    s
}

/// Rasterize the card SVG to a 1200×630 PNG using the vendored fonts.
pub fn render_png(svg: &str) -> anyhow::Result<Vec<u8>> {
    let opt = usvg::Options {
        fontdb: font_db(),
        font_family: "Inter".to_string(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(svg, &opt).context("parse og card svg")?;
    let mut pixmap =
        tiny_skia::Pixmap::new(WIDTH, HEIGHT).context("allocate og card pixmap")?;
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    pixmap.encode_png().context("encode og card png")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::EpisodeScoreView;
    use crate::scoring;
    use uuid::Uuid;

    fn stats() -> OgStats {
        OgStats {
            canon: 100,
            worth_watching: 26,
            filler: 84,
            undecided: 10,
        }
    }

    #[test]
    fn filler_pct_rounds_and_handles_zero_total() {
        assert_eq!(stats().filler_pct(), 38); // 84/220 = 38.18…
        assert_eq!(OgStats::default().filler_pct(), 0); // no division by zero
        let all_filler = OgStats { filler: 3, ..OgStats::default() };
        assert_eq!(all_filler.filler_pct(), 100);
    }

    #[test]
    fn stats_from_episodes_tallies_statuses_and_skips_specials() {
        let ep = |season: i32, f: i64, w: i64, c: i64| EpisodeItem {
            id: Uuid::nil(),
            season_number: season,
            episode_number: 1,
            name: None,
            air_date: None,
            still_path: None,
            tmdb_rating: None,
            tmdb_vote_count: None,
            score: EpisodeScoreView {
                filler_votes: f,
                worth_watching_votes: w,
                canon_votes: c,
                filler_score: scoring::filler_score(f, w, c),
                status: scoring::status(f, w, c),
                my_vote: None,
            },
        };
        let episodes = vec![
            ep(1, 10, 0, 0), // filler
            ep(1, 0, 10, 0), // worth watching
            ep(1, 0, 0, 10), // canon
            ep(1, 5, 5, 0),  // contested -> undecided
            ep(1, 1, 0, 0),  // not enough votes -> undecided
            ep(0, 10, 0, 0), // special: excluded
        ];
        let s = stats_from_episodes(&episodes);
        assert_eq!(
            s,
            OgStats { canon: 1, worth_watching: 1, filler: 1, undecided: 2 }
        );
        assert_eq!(s.total(), 5);
    }

    #[test]
    fn svg_escapes_name_and_renders_stat_lines() {
        let svg = og_svg("Fate & Stay <Night>", &stats(), None);
        assert!(svg.contains("Fate &amp; Stay &lt;Night&gt;"), "{svg}");
        assert!(!svg.contains("Fate & Stay"), "{svg}");
        assert!(svg.contains(">38% filler</text>"), "{svg}");
        assert!(svg.contains("Skip 84 of 220 episodes"), "{svg}");
        assert!(svg.contains("100 canon"), "{svg}");
        assert!(svg.contains("26 worth it"), "{svg}");
        assert!(svg.contains("84 filler"), "{svg}");
        assert!(svg.contains("10 undecided"), "{svg}");
        // Wordmark tspans.
        assert!(svg.contains(">filler</tspan>"), "{svg}");
        assert!(svg.contains(">killer</tspan>"), "{svg}");
        assert!(svg.contains(">.app</tspan>"), "{svg}");
    }

    #[test]
    fn svg_zero_episode_show_has_placeholder_not_percent() {
        let svg = og_svg("Brand New Show", &OgStats::default(), None);
        assert!(svg.contains("No episodes tracked yet"), "{svg}");
        assert!(!svg.contains("% filler"), "{svg}");
        assert!(!svg.contains("NaN"), "{svg}");
    }

    #[test]
    fn svg_embeds_poster_as_data_uri_only_when_present() {
        let poster = Poster { bytes: vec![1, 2, 3], mime: "image/jpeg" };
        let with = og_svg("Naruto", &stats(), Some(&poster));
        assert!(with.contains("data:image/jpeg;base64,AQID"), "{with}");
        let without = og_svg("Naruto", &stats(), None);
        assert!(!without.contains("<image"), "{without}");
    }

    #[test]
    fn svg_wraps_and_ellipsizes_long_names() {
        let long = "The Melancholy of Haruhi Suzumiya and Other Extremely Long Anime Titles \
                    That Never End No Matter How Far You Scroll Through the Season Listing";
        let svg = og_svg(long, &stats(), None);
        assert!(svg.contains('…'), "{svg}");
        // Two title lines (font-size 44 for very long names).
        assert_eq!(svg.matches("font-size=\"44\"").count(), 2, "{svg}");
    }

    #[test]
    fn breakdown_row_fits_the_column() {
        // With a poster the text column is ~676px: the optional "undecided"
        // item is dropped rather than overflowing the card.
        let (items, size) = breakdown_layout(&stats(), 676.0);
        assert_eq!(items.len(), 3);
        assert_eq!(size, 26.0);
        // The full-width text-only layout keeps all four.
        let (items, _) = breakdown_layout(&stats(), 1072.0);
        assert_eq!(items.len(), 4);
        // Absurd counts shrink the font instead of overflowing.
        let huge = OgStats { canon: 99999, worth_watching: 99999, filler: 99999, undecided: 0 };
        let (_, size) = breakdown_layout(&huge, 400.0);
        assert!(size < 26.0);
    }

    #[test]
    fn wrap_ellipsize_handles_overlong_single_word() {
        let lines = wrap_ellipsize("Supercalifragilisticexpialidocious", 10, 2);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with('…'));
        assert!(lines[0].chars().count() <= 10);
    }

    #[test]
    fn render_png_with_poster_data_uri_succeeds() {
        // A real (tiny) PNG via tiny-skia's own encoder exercises the embedded
        // <image> data-URI decode path end to end.
        let bytes = tiny_skia::Pixmap::new(2, 3).unwrap().encode_png().unwrap();
        let poster = Poster { bytes, mime: "image/png" };
        let png = render_png(&og_svg("Naruto", &stats(), Some(&poster))).expect("render");
        assert_eq!(&png[..4], &[0x89, 0x50, 0x4e, 0x47]);
    }

    #[test]
    fn render_png_smoke_text_only_card_has_right_dimensions() {
        let png = render_png(&og_svg("Cowboy Bebop", &stats(), None)).expect("render");
        // PNG signature + IHDR width/height (big-endian at offsets 16 and 20).
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert_eq!((width, height), (WIDTH, HEIGHT));
    }
}
