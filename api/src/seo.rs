//! Server-side SEO rendering.
//!
//! FillerKiller is a client-rendered SPA, so a plain crawler or link unfurler
//! would see only the empty shell. For the SEO-critical routes (show pages and
//! their skip guides) the server injects a per-page `<head>` (title, description,
//! canonical, Open Graph / Twitter) and a crawler-visible content snapshot into
//! the shell — then the React app still hydrates client-side and takes over.
//! This is plain string templating, not a JS SSR runtime, so it stays inside the
//! single Rust container. Also builds the DB-driven sitemap.

use crate::db::ShowCore;
use crate::guides::{Disposition, GuideDetail};
use crate::models::SeasonSummary;
use crate::scoring::{SkipGuide, SkipGuideEntry};

const SITE: &str = "FillerKiller";

// Markers in index.html. The build's default `<head>` lives between HEAD_OPEN
// and HEAD_CLOSE (used as-is for the home page and in dev); the server swaps that
// block for per-page tags. The snapshot is injected into the empty root div.
const HEAD_OPEN: &str = "<!--head-->";
const HEAD_CLOSE: &str = "<!--/head-->";
const ROOT_EMPTY: &str = "<div id=\"root\"></div>";

/// Escape text for safe inclusion in HTML element and attribute contexts.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Trim to at most `max` chars on a word boundary, appending an ellipsis when cut.
fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    if let Some(idx) = out.rfind(' ') {
        out.truncate(idx);
    }
    out.push('…');
    out
}

/// Replace everything from `open` through `close` (inclusive) with `replacement`.
/// Returns the input unchanged if the markers aren't found in order.
fn replace_between(s: &str, open: &str, close: &str, replacement: &str) -> String {
    if let (Some(i), Some(j)) = (s.find(open), s.find(close)) {
        if j >= i {
            let mut out = String::with_capacity(s.len() + replacement.len());
            out.push_str(&s[..i]);
            out.push_str(replacement);
            out.push_str(&s[j + close.len()..]);
            return out;
        }
    }
    s.to_string()
}

struct PageMeta {
    title: String,
    description: String,
    canonical: String,
    image: Option<String>,
}

fn head_tags(m: &PageMeta) -> String {
    let title = html_escape(&m.title);
    let desc = html_escape(&m.description);
    let canonical = html_escape(&m.canonical);
    let mut s = String::new();
    s.push_str(&format!("<title>{title}</title>"));
    s.push_str(&format!("<meta name=\"description\" content=\"{desc}\" />"));
    s.push_str(&format!("<link rel=\"canonical\" href=\"{canonical}\" />"));
    s.push_str("<meta property=\"og:type\" content=\"website\" />");
    s.push_str(&format!("<meta property=\"og:site_name\" content=\"{SITE}\" />"));
    s.push_str(&format!("<meta property=\"og:title\" content=\"{title}\" />"));
    s.push_str(&format!("<meta property=\"og:description\" content=\"{desc}\" />"));
    s.push_str(&format!("<meta property=\"og:url\" content=\"{canonical}\" />"));
    s.push_str("<meta name=\"twitter:card\" content=\"summary_large_image\" />");
    s.push_str(&format!("<meta name=\"twitter:title\" content=\"{title}\" />"));
    s.push_str(&format!("<meta name=\"twitter:description\" content=\"{desc}\" />"));
    if let Some(image) = &m.image {
        let image = html_escape(image);
        s.push_str(&format!("<meta property=\"og:image\" content=\"{image}\" />"));
        s.push_str(&format!("<meta name=\"twitter:image\" content=\"{image}\" />"));
    }
    s
}

/// Inject a per-page head + content snapshot into the SPA shell. The snapshot is
/// wrapped in a `hidden` element: it stays in the HTML source for crawlers/link
/// unfurlers, but never paints for users — so there's no unstyled (and, on mobile,
/// desktop-looking) flash before React mounts and replaces `#root`.
pub fn render_page(template: &str, head: &str, snapshot: &str) -> String {
    let with_head = replace_between(template, HEAD_OPEN, HEAD_CLOSE, head);
    with_head.replacen(
        ROOT_EMPTY,
        &format!("<div id=\"root\"><div hidden>{snapshot}</div></div>"),
        1,
    )
}

/// Render a show detail page with per-show SEO head + snapshot.
pub fn show_page(
    template: &str,
    base: &str,
    core: &ShowCore,
    seasons: &[SeasonSummary],
    image: Option<String>,
) -> String {
    let base = base.trim_end_matches('/');
    let canonical = format!("{base}/shows/{}", core.slug);
    let description = match core.overview.as_deref() {
        Some(o) if !o.trim().is_empty() => truncate(o, 160),
        _ => format!(
            "Crowd-sourced filler guide for {} — see which episodes are filler, worth it, or canon.",
            core.name
        ),
    };
    let head = head_tags(&PageMeta {
        title: format!("{} — {SITE}", core.name),
        description,
        canonical: canonical.clone(),
        // Fall back to the site card so a posterless show still unfurls an image.
        image: image.or_else(|| Some(format!("{base}/og-image.png"))),
    });

    let mut body = String::new();
    body.push_str(&format!("<h1>{}</h1>", html_escape(&core.name)));
    if let Some(o) = core.overview.as_deref() {
        if !o.trim().is_empty() {
            body.push_str(&format!("<p>{}</p>", html_escape(o)));
        }
    }
    if !seasons.is_empty() {
        body.push_str("<h2>Seasons</h2><ul>");
        for s in seasons {
            let label = match s.name.as_deref() {
                Some(n) if !n.trim().is_empty() => html_escape(n),
                _ => format!("Season {}", s.season_number),
            };
            body.push_str(&format!("<li>{label} ({} episodes)</li>", s.episode_count));
        }
        body.push_str("</ul>");
    }
    body.push_str(&format!(
        "<p><a href=\"{}/skip-guide\">View the skip guide for {}</a></p>",
        html_escape(&canonical),
        html_escape(&core.name)
    ));

    render_page(template, &head, &body)
}

/// Render a skip-guide page with the watch/optional/skip lists as real content.
pub fn skip_guide_page(template: &str, base: &str, core: &ShowCore, guide: &SkipGuide) -> String {
    let base = base.trim_end_matches('/');
    let canonical = format!("{base}/shows/{}/skip-guide", core.slug);
    let description = format!(
        "Crowd-sourced binge order for {}: {} to watch, {} optional, {} to skip.",
        core.name,
        guide.watch.len(),
        guide.optional.len(),
        guide.skipped.len()
    );
    let head = head_tags(&PageMeta {
        title: format!("Skip guide — {} — {SITE}", core.name),
        description,
        canonical,
        image: None,
    });

    let mut body = String::new();
    body.push_str(&format!("<h1>Skip guide: {}</h1>", html_escape(&core.name)));
    render_bucket(&mut body, "Watch", &guide.watch);
    render_bucket(&mut body, "Optional — worth it", &guide.optional);
    render_bucket(&mut body, "Skip", &guide.skipped);

    render_page(template, &head, &body)
}

/// Render a user-authored guide's share page: per-guide SEO head + the curated
/// watch/optional/skip lists as crawlable content. Caller passes only published
/// guides.
pub fn guide_page(template: &str, base: &str, guide: &GuideDetail) -> String {
    let base = base.trim_end_matches('/');
    let canonical = format!("{base}/shows/{}/guides/{}", guide.show_slug, guide.id);
    let count = |d: Disposition| guide.entries.iter().filter(|e| e.disposition == d).count();
    let (w, o, s) = (count(Disposition::Watch), count(Disposition::Optional), count(Disposition::Skip));
    let description = match guide.description.as_deref() {
        Some(d) if !d.trim().is_empty() => truncate(d, 160),
        _ => format!("A {} skip guide: {w} to watch, {o} optional, {s} to skip.", guide.show_name),
    };
    let head = head_tags(&PageMeta {
        title: format!("{} — {} skip guide — {SITE}", guide.title, guide.show_name),
        description,
        canonical,
        image: Some(format!("{base}/og-image.png")),
    });

    let mut body = String::new();
    body.push_str(&format!("<h1>{}</h1>", html_escape(&guide.title)));
    body.push_str(&format!(
        "<p>A {} skip guide by {}</p>",
        html_escape(&guide.show_name),
        html_escape(guide.author_name.as_deref().unwrap_or("a former member"))
    ));
    if let Some(d) = guide.description.as_deref() {
        if !d.trim().is_empty() {
            body.push_str(&format!("<p>{}</p>", html_escape(d)));
        }
    }
    for (label, disp) in [
        ("Watch", Disposition::Watch),
        ("Optional — worth it", Disposition::Optional),
        ("Skip", Disposition::Skip),
    ] {
        let items: Vec<_> = guide.entries.iter().filter(|e| e.disposition == disp).collect();
        body.push_str(&format!("<h2>{label} ({})</h2>", items.len()));
        if items.is_empty() {
            body.push_str("<p>None.</p>");
        } else {
            body.push_str("<ul>");
            for e in items {
                let name = e.name.as_deref().unwrap_or("Untitled");
                body.push_str(&format!(
                    "<li>S{}E{} — {}</li>",
                    e.season_number,
                    e.episode_number,
                    html_escape(name)
                ));
            }
            body.push_str("</ul>");
        }
    }

    render_page(template, &head, &body)
}

fn render_bucket(body: &mut String, title: &str, entries: &[SkipGuideEntry]) {
    body.push_str(&format!("<h2>{title} ({})</h2>", entries.len()));
    if entries.is_empty() {
        body.push_str("<p>None.</p>");
        return;
    }
    body.push_str("<ul>");
    for e in entries {
        let name = e.name.as_deref().unwrap_or("Untitled");
        body.push_str(&format!(
            "<li>S{}E{} — {}</li>",
            e.season_number,
            e.episode_number,
            html_escape(name)
        ));
    }
    body.push_str("</ul>");
}

/// Build the sitemap XML from the stable routes plus every show's pages.
pub fn sitemap_xml(base: &str, slugs: &[String]) -> String {
    let base = base.trim_end_matches('/');
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    let mut url = |loc: String, priority: &str| {
        s.push_str(&format!(
            "  <url><loc>{}</loc><priority>{priority}</priority></url>\n",
            html_escape(&loc)
        ));
    };
    url(format!("{base}/"), "1.0");
    for path in ["about", "support", "privacy", "terms"] {
        url(format!("{base}/{path}"), "0.3");
    }
    for slug in slugs {
        url(format!("{base}/shows/{slug}"), "0.7");
        url(format!("{base}/shows/{slug}/skip-guide"), "0.6");
    }
    s.push_str("</urlset>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_metacharacters() {
        assert_eq!(html_escape("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn truncate_keeps_short_and_cuts_long_on_word_boundary() {
        assert_eq!(truncate("short text", 160), "short text");
        let long = "one two three four five";
        let t = truncate(long, 10);
        assert!(t.ends_with('…'));
        assert!(t.chars().count() <= 11);
        assert!(!t.contains("four"));
    }

    #[test]
    fn render_page_swaps_head_and_fills_root() {
        let tmpl = "<head><!--head--><title>Default</title><!--/head--></head>\
                    <body><div id=\"root\"></div></body>";
        let out = render_page(tmpl, "<title>Custom</title>", "<h1>Hi</h1>");
        assert!(out.contains("<title>Custom</title>"));
        assert!(!out.contains("Default"));
        assert!(!out.contains("<!--head-->"));
        assert!(out.contains("<div id=\"root\"><div hidden><h1>Hi</h1></div></div>"));
    }

    #[test]
    fn render_page_is_noop_without_markers() {
        let tmpl = "<head><title>x</title></head><body>no root</body>";
        assert_eq!(render_page(tmpl, "<title>y</title>", "<h1>z</h1>"), tmpl);
    }

    #[test]
    fn guide_page_renders_head_and_escaped_buckets() {
        use crate::guides::{Disposition, GuideDetail, GuideEntryView};
        use uuid::Uuid;
        let guide = GuideDetail {
            id: Uuid::nil(),
            show_id: Uuid::nil(),
            show_slug: "breaking-bad".into(),
            show_name: "Breaking Bad".into(),
            title: "Story only <x>".into(),
            description: Some("Just the essentials".into()),
            author_name: Some("Ann".into()),
            like_count: 3,
            is_published: true,
            my_like: false,
            mine: false,
            entries: vec![
                GuideEntryView { episode_id: Uuid::nil(), season_number: 1, episode_number: 1, name: Some("Pilot".into()), disposition: Disposition::Watch },
                GuideEntryView { episode_id: Uuid::nil(), season_number: 1, episode_number: 2, name: None, disposition: Disposition::Skip },
            ],
        };
        let tmpl = "<head><!--head--><title>D</title><!--/head--></head><body><div id=\"root\"></div></body>";
        let out = guide_page(tmpl, "https://fillerkiller.app", &guide);
        assert!(out.contains("Story only &lt;x&gt; — Breaking Bad skip guide"), "{out}");
        assert!(out.contains("/shows/breaking-bad/guides/00000000-0000-0000-0000-000000000000"));
        assert!(out.contains("<h2>Watch (1)</h2>"));
        assert!(out.contains("<h2>Skip (1)</h2>"));
        assert!(out.contains("S1E1 — Pilot"));
    }

    #[test]
    fn sitemap_lists_static_and_show_routes() {
        let xml = sitemap_xml("https://fillerkiller.app/", &["breaking-bad".into()]);
        assert!(xml.contains("<loc>https://fillerkiller.app/</loc>"));
        assert!(xml.contains("<loc>https://fillerkiller.app/shows/breaking-bad</loc>"));
        assert!(xml.contains("<loc>https://fillerkiller.app/shows/breaking-bad/skip-guide</loc>"));
        assert!(xml.contains("<loc>https://fillerkiller.app/privacy</loc>"));
    }
}
