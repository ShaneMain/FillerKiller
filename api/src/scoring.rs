//! Filler scoring and skip-guide derivation.
//!
//! Single source of truth in code for the filler-scoring math. These constants
//! define the public voting behavior — change them deliberately. Pure functions
//! only: no I/O, no DB.

use serde::{Deserialize, Serialize};

/// Minimum total votes before we show a confident label at all.
pub const MIN_VOTES: i64 = 5;
/// If the plurality lead over the runner-up is within this fraction of the total,
/// the episode is CONTESTED rather than labelled.
pub const CONTESTED_MARGIN: f64 = 0.10;

/// A user's vote. Wire format is `FILLER`/`WORTH_WATCHING`/`CANON` (request body
/// + responses); the same strings are the Postgres `vote_value` enum labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VoteValue {
    Filler,
    WorthWatching,
    Canon,
}

impl VoteValue {
    /// The Postgres `vote_value` label for this vote.
    pub fn as_db(&self) -> &'static str {
        match self {
            VoteValue::Filler => "FILLER",
            VoteValue::WorthWatching => "WORTH_WATCHING",
            VoteValue::Canon => "CANON",
        }
    }

    /// Parse a Postgres `vote_value` label back into a `VoteValue`.
    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "FILLER" => Some(VoteValue::Filler),
            "WORTH_WATCHING" => Some(VoteValue::WorthWatching),
            "CANON" => Some(VoteValue::Canon),
            _ => None,
        }
    }
}

/// An optional reason tag a voter can attach to their vote. Tags are value-scoped;
/// only certain tags are valid for each `VoteValue`. See [`valid_reasons`].
///
/// Wire format: kebab-case strings matching the Postgres CHECK constraint values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoteReason {
    // FILLER tags
    Recap,
    SideStory,
    FunButSkippable,
    // WORTH_WATCHING tags
    SelfContainedGem,
    CharacterMoment,
    Worldbuilding,
    // CANON tags
    MajorPlot,
    CharacterDevelopment,
    ArcSetup,
}

impl VoteReason {
    /// The DB/wire string for this reason (kebab-case, matches the CHECK constraint).
    pub fn as_str(&self) -> &'static str {
        match self {
            VoteReason::Recap => "recap",
            VoteReason::SideStory => "side-story",
            VoteReason::FunButSkippable => "fun-but-skippable",
            VoteReason::SelfContainedGem => "self-contained-gem",
            VoteReason::CharacterMoment => "character-moment",
            VoteReason::Worldbuilding => "worldbuilding",
            VoteReason::MajorPlot => "major-plot",
            VoteReason::CharacterDevelopment => "character-development",
            VoteReason::ArcSetup => "arc-setup",
        }
    }

    /// Parse a DB/wire reason string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "recap" => Some(VoteReason::Recap),
            "side-story" => Some(VoteReason::SideStory),
            "fun-but-skippable" => Some(VoteReason::FunButSkippable),
            "self-contained-gem" => Some(VoteReason::SelfContainedGem),
            "character-moment" => Some(VoteReason::CharacterMoment),
            "worldbuilding" => Some(VoteReason::Worldbuilding),
            "major-plot" => Some(VoteReason::MajorPlot),
            "character-development" => Some(VoteReason::CharacterDevelopment),
            "arc-setup" => Some(VoteReason::ArcSetup),
            _ => None,
        }
    }

    /// All three reasons valid for a given vote value, in display order.
    pub fn valid_for(value: VoteValue) -> &'static [VoteReason] {
        match value {
            VoteValue::Filler => &[
                VoteReason::Recap,
                VoteReason::SideStory,
                VoteReason::FunButSkippable,
            ],
            VoteValue::WorthWatching => &[
                VoteReason::SelfContainedGem,
                VoteReason::CharacterMoment,
                VoteReason::Worldbuilding,
            ],
            VoteValue::Canon => &[
                VoteReason::MajorPlot,
                VoteReason::CharacterDevelopment,
                VoteReason::ArcSetup,
            ],
        }
    }

    /// Return `true` if this reason tag is valid for `value`.
    pub fn is_valid_for(&self, value: VoteValue) -> bool {
        Self::valid_for(value).contains(self)
    }

    /// Human-readable label (used in tests to verify the full tag matrix).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn label(&self) -> &'static str {
        match self {
            VoteReason::Recap => "Recap episode",
            VoteReason::SideStory => "Side story, no plot",
            VoteReason::FunButSkippable => "Fun but skippable",
            VoteReason::SelfContainedGem => "Self-contained gem",
            VoteReason::CharacterMoment => "Great character moment",
            VoteReason::Worldbuilding => "Worldbuilding",
            VoteReason::MajorPlot => "Major plot",
            VoteReason::CharacterDevelopment => "Character development",
            VoteReason::ArcSetup => "Sets up a later arc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EpisodeStatus {
    Canon,
    WorthWatching,
    Filler,
    Contested,
    NotEnoughVotes,
}

/// Minimum TMDB rating at which `Binge` mode rescues a WorthWatching episode
/// into the watch list ("the good standalone episodes survive the cut").
pub const BINGE_RESCUE_RATING: f64 = 7.0;

/// The four skip-guide modes governing how episodes are partitioned.
///
/// # Binge rating threshold
/// `Binge` mode rescues WorthWatching episodes with a TMDB rating of at least
/// [`BINGE_RESCUE_RATING`] into the watch list. Episodes rated below it, or with
/// no rating at all, are skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GuideMode {
    /// Skip ONLY confirmed Filler. WorthWatching joins watch. Unsure stays watch.
    /// `optional` is always empty.
    Completionist,
    /// Default: Canon → watch, WorthWatching → optional, Filler → skipped.
    /// Unsure (Contested/NotEnoughVotes) stays watch.
    Standard,
    /// Watch = Canon + Unsure only. WorthWatching moves to skipped (not optional).
    /// Filler skipped. Optional list is always empty.
    CanonOnly,
    /// Like CanonOnly, but WorthWatching episodes with TMDB rating >= 7.0 are
    /// rescued into watch. WorthWatching with rating < 7.0 or no rating → skipped.
    /// Unsure stays watch (never skip what might be canon).
    Binge,
}

/// Pure-filler fraction: `filler / total`, or `None` when there are no votes.
pub fn filler_score(filler: i64, worth_watching: i64, canon: i64) -> Option<f64> {
    let total = filler + worth_watching + canon;
    if total <= 0 {
        return None;
    }
    Some(filler as f64 / total as f64)
}

/// Derive the displayed status by plurality, applying the confidence floor
/// (`MIN_VOTES`) and the contested margin.
pub fn status(filler: i64, worth_watching: i64, canon: i64) -> EpisodeStatus {
    let total = filler + worth_watching + canon;
    if total < MIN_VOTES {
        return EpisodeStatus::NotEnoughVotes;
    }

    // Highest count wins; a near-tie with the runner-up is CONTESTED.
    let mut ranked = [
        (EpisodeStatus::Filler, filler),
        (EpisodeStatus::WorthWatching, worth_watching),
        (EpisodeStatus::Canon, canon),
    ];
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));

    let lead = (ranked[0].1 - ranked[1].1) as f64 / total as f64;
    if lead <= CONTESTED_MARGIN {
        EpisodeStatus::Contested
    } else {
        ranked[0].0
    }
}

/// Map a status to the `vote_value` DB string of the plurality winner, or
/// `None` when the episode is contested / has too few votes (no clear leader
/// whose reason tags are worth surfacing).
pub fn plurality_value(status: EpisodeStatus) -> Option<&'static str> {
    match status {
        EpisodeStatus::Filler => Some("FILLER"),
        EpisodeStatus::WorthWatching => Some("WORTH_WATCHING"),
        EpisodeStatus::Canon => Some("CANON"),
        EpisodeStatus::Contested | EpisodeStatus::NotEnoughVotes => None,
    }
}

/// An episode plus its raw vote counts, the input to the skip guide.
#[derive(Debug, Clone)]
pub struct ScoredEpisode {
    pub episode_id: String,
    pub season_number: i32,
    pub episode_number: i32,
    pub name: Option<String>,
    pub filler_votes: i64,
    pub worth_watching_votes: i64,
    pub canon_votes: i64,
    /// TMDB's own audience rating (0–10); used by `Binge` mode to rescue
    /// WorthWatching episodes with a high enough rating into the watch list.
    pub tmdb_rating: Option<f64>,
    /// Episode runtime in minutes; used for time-saved calculation.
    pub runtime_minutes: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipGuideEntry {
    pub episode_id: String,
    pub season_number: i32,
    pub episode_number: i32,
    pub name: Option<String>,
    pub status: EpisodeStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thresholds {
    pub min_votes: i64,
    pub contested_margin: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipGuide {
    pub watch: Vec<SkipGuideEntry>,
    pub optional: Vec<SkipGuideEntry>,
    pub skipped: Vec<SkipGuideEntry>,
    pub thresholds: Thresholds,
    pub mode: GuideMode,
    pub valid_modes: Vec<&'static str>,
    pub minutes_skipped: Option<i64>,
}

/// Compute the total time (minutes) saved by skipping `skipped_episodes`.
///
/// For each skipped episode, the runtime used is:
/// 1. The episode's own `runtime_minutes` if present.
/// 2. Otherwise, the show-average of all episodes with a known runtime (computed
///    from the full episode set passed in `all_episodes`).
/// 3. If the show has NO known runtimes at all, returns `None` (UI omits the display).
///
/// All averages and sums are integer-rounded (nearest minute).
pub fn minutes_skipped(
    skipped: &[&ScoredEpisode],
    all_episodes: &[ScoredEpisode],
) -> Option<i64> {
    if skipped.is_empty() {
        return Some(0);
    }

    // Compute show-average runtime from all episodes with known runtimes.
    let known: Vec<i32> = all_episodes
        .iter()
        .filter_map(|e| e.runtime_minutes)
        .collect();

    if known.is_empty() {
        return None;
    }

    let avg_runtime = (known.iter().map(|&r| r as i64).sum::<i64>() as f64
        / known.len() as f64)
        .round() as i64;

    let total: i64 = skipped
        .iter()
        .map(|ep| ep.runtime_minutes.map(|r| r as i64).unwrap_or(avg_runtime))
        .sum();

    Some(total)
}

/// Build a show's skip guide from its scored episodes.
///
/// The `mode` controls how episodes are partitioned:
/// - `Completionist`: WorthWatching → watch; Filler → skipped; Unsure → watch; optional always empty
/// - `Standard`: Canon → watch; WorthWatching → optional; Filler → skipped; Unsure → watch
/// - `CanonOnly`: Canon → watch; WorthWatching → skipped; Filler → skipped; Unsure → watch; optional always empty
/// - `Binge`: Canon → watch; WorthWatching with tmdb_rating >= 7.0 → watch; WorthWatching with rating < 7.0 or None → skipped; Filler → skipped; Unsure → watch; optional always empty
///
/// Specials (season 0) are excluded unless `include_specials` is set.
pub fn build_skip_guide(
    episodes: &[ScoredEpisode],
    mode: GuideMode,
    include_specials: bool,
) -> SkipGuide {
    let mut ordered: Vec<&ScoredEpisode> = episodes.iter().collect();
    ordered.sort_by(|a, b| {
        a.season_number
            .cmp(&b.season_number)
            .then(a.episode_number.cmp(&b.episode_number))
    });

    let mut watch = Vec::new();
    let mut optional = Vec::new();
    let mut skipped = Vec::new();
    // Episodes behind the `skipped` entries, kept for the time-saved math so it
    // is derived from the same partition pass and can never disagree with it.
    let mut skipped_refs: Vec<&ScoredEpisode> = Vec::new();

    for ep in &ordered {
        if ep.season_number == 0 && !include_specials {
            continue;
        }

        let st = status(ep.filler_votes, ep.worth_watching_votes, ep.canon_votes);
        let entry = SkipGuideEntry {
            episode_id: ep.episode_id.clone(),
            season_number: ep.season_number,
            episode_number: ep.episode_number,
            name: ep.name.clone(),
            status: st,
        };

        let mut skip = |entry: SkipGuideEntry| {
            skipped.push(entry);
            skipped_refs.push(*ep);
        };

        match mode {
            GuideMode::Completionist => match st {
                EpisodeStatus::Filler => skip(entry),
                // WorthWatching, Canon, Contested, NotEnoughVotes → all watch
                _ => watch.push(entry),
            },
            GuideMode::Standard => match st {
                EpisodeStatus::Filler => skip(entry),
                EpisodeStatus::WorthWatching => optional.push(entry),
                EpisodeStatus::Canon => watch.push(entry),
                // Contested or NotEnoughVotes → watch (safe default)
                _ => watch.push(entry),
            },
            GuideMode::CanonOnly => match st {
                EpisodeStatus::Canon => watch.push(entry),
                // Unsure stays watch
                EpisodeStatus::Contested | EpisodeStatus::NotEnoughVotes => watch.push(entry),
                // WorthWatching and Filler → skipped; optional always empty
                _ => skip(entry),
            },
            GuideMode::Binge => match st {
                EpisodeStatus::Canon => watch.push(entry),
                EpisodeStatus::WorthWatching => {
                    // Rescue into watch if rated well enough; otherwise skip.
                    if ep.tmdb_rating.map(|r| r >= BINGE_RESCUE_RATING).unwrap_or(false) {
                        watch.push(entry);
                    } else {
                        skip(entry);
                    }
                }
                // Unsure → watch (never skip what might be canon)
                EpisodeStatus::Contested | EpisodeStatus::NotEnoughVotes => watch.push(entry),
                // Filler → skipped
                EpisodeStatus::Filler => skip(entry),
            },
        }
    }

    let mins = minutes_skipped(&skipped_refs, episodes);

    SkipGuide {
        watch,
        optional,
        skipped,
        thresholds: Thresholds {
            min_votes: MIN_VOTES,
            contested_margin: CONTESTED_MARGIN,
        },
        mode,
        valid_modes: vec!["completionist", "standard", "canon-only", "binge"],
        minutes_skipped: mins,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filler_score_none_with_no_votes() {
        assert_eq!(filler_score(0, 0, 0), None);
    }

    #[test]
    fn filler_score_pure_filler_fraction() {
        assert_eq!(filler_score(12, 20, 68), Some(0.12));
        assert_eq!(filler_score(1, 0, 1), Some(0.5));
        assert_eq!(filler_score(3, 0, 0), Some(1.0));
    }

    #[test]
    fn status_below_min_votes_is_not_enough() {
        assert_eq!(status(4, 0, 0), EpisodeStatus::NotEnoughVotes);
        assert_eq!(status(2, 1, 1), EpisodeStatus::NotEnoughVotes); // n=4
        assert_eq!(status(0, 0, 0), EpisodeStatus::NotEnoughVotes);
    }

    #[test]
    fn status_plurality_winner() {
        assert_eq!(status(8, 1, 1), EpisodeStatus::Filler);
        assert_eq!(status(1, 8, 1), EpisodeStatus::WorthWatching);
        assert_eq!(status(1, 1, 8), EpisodeStatus::Canon);
    }

    #[test]
    fn status_contested_on_tie_or_thin_margin() {
        assert_eq!(status(5, 5, 0), EpisodeStatus::Contested); // tie
        assert_eq!(status(4, 4, 2), EpisodeStatus::Contested); // tie for top
        assert_eq!(status(5, 4, 1), EpisodeStatus::Contested); // lead 1/10 = 0.10 (<= margin)
        assert_eq!(status(6, 4, 0), EpisodeStatus::Filler); // lead 2/10 = 0.20 (> margin)
        assert_eq!(status(3, 3, 3), EpisodeStatus::Contested); // three-way tie, n=9
    }

    #[test]
    fn status_exactly_min_votes_is_enough() {
        assert_eq!(MIN_VOTES, 5);
        assert_eq!(status(5, 0, 0), EpisodeStatus::Filler);
    }

    fn ep(season: i32, episode: i32, f: i64, w: i64, c: i64) -> ScoredEpisode {
        ScoredEpisode {
            episode_id: format!("s{season}e{episode}"),
            season_number: season,
            episode_number: episode,
            name: None,
            filler_votes: f,
            worth_watching_votes: w,
            canon_votes: c,
            tmdb_rating: None,
            runtime_minutes: None,
        }
    }

    fn ep_rated(season: i32, episode: i32, f: i64, w: i64, c: i64, rating: Option<f64>) -> ScoredEpisode {
        ScoredEpisode {
            episode_id: format!("s{season}e{episode}"),
            season_number: season,
            episode_number: episode,
            name: None,
            filler_votes: f,
            worth_watching_votes: w,
            canon_votes: c,
            tmdb_rating: rating,
            runtime_minutes: None,
        }
    }

    fn ep_runtime(season: i32, episode: i32, f: i64, w: i64, c: i64, runtime: Option<i32>) -> ScoredEpisode {
        ScoredEpisode {
            episode_id: format!("s{season}e{episode}"),
            season_number: season,
            episode_number: episode,
            name: None,
            filler_votes: f,
            worth_watching_votes: w,
            canon_votes: c,
            tmdb_rating: None,
            runtime_minutes: runtime,
        }
    }

    fn ids(entries: &[SkipGuideEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.episode_id.as_str()).collect()
    }

    // ---- Standard mode tests (backward-compatible) ----

    #[test]
    fn skip_guide_sorts_into_watch_optional_skipped() {
        let guide = build_skip_guide(
            &[
                ep(1, 1, 0, 0, 10), // canon -> watch
                ep(1, 2, 10, 0, 0), // filler -> skipped
                ep(1, 3, 0, 10, 0), // worth watching -> optional
            ],
            GuideMode::Standard,
            false,
        );
        assert_eq!(ids(&guide.watch), ["s1e1"]);
        assert_eq!(ids(&guide.optional), ["s1e3"]);
        assert_eq!(ids(&guide.skipped), ["s1e2"]);
    }

    #[test]
    fn skip_guide_contested_defaults_to_watch() {
        let eps = [ep(1, 1, 5, 5, 0), ep(1, 2, 1, 1, 0)]; // contested, not-enough
        let guide = build_skip_guide(&eps, GuideMode::Standard, false);
        assert_eq!(guide.watch.len(), 2);
        assert_eq!(guide.skipped.len(), 0);
    }

    #[test]
    fn skip_guide_specials_excluded_by_default() {
        let eps = [ep(0, 1, 0, 0, 10), ep(1, 1, 0, 0, 10)];
        assert_eq!(build_skip_guide(&eps, GuideMode::Standard, false).watch.len(), 1);
        assert_eq!(build_skip_guide(&eps, GuideMode::Standard, true).watch.len(), 2);
    }

    #[test]
    fn skip_guide_serializes_camelcase_per_spec() {
        let guide = build_skip_guide(&[ep(1, 1, 10, 0, 0)], GuideMode::Standard, false);
        let json = serde_json::to_string(&guide).unwrap();
        assert!(json.contains("\"episodeId\""), "{json}");
        assert!(json.contains("\"minVotes\""), "{json}");
        assert!(json.contains("\"contestedMargin\""), "{json}");
        assert!(json.contains("\"FILLER\""), "{json}");
        assert!(!json.contains("episode_id"), "{json}");
    }

    #[test]
    fn vote_value_db_round_trip() {
        for v in [VoteValue::Filler, VoteValue::WorthWatching, VoteValue::Canon] {
            assert_eq!(VoteValue::from_db(v.as_db()), Some(v));
        }
        assert_eq!(VoteValue::from_db("NOPE"), None);
    }

    // ---- Completionist mode ----

    #[test]
    fn completionist_only_skips_confirmed_filler() {
        let guide = build_skip_guide(
            &[
                ep(1, 1, 0, 0, 10),  // canon -> watch
                ep(1, 2, 10, 0, 0),  // filler -> skipped
                ep(1, 3, 0, 10, 0),  // worth watching -> watch (not optional)
                ep(1, 4, 5, 5, 0),   // contested -> watch
                ep(1, 5, 1, 1, 0),   // not enough votes -> watch
            ],
            GuideMode::Completionist,
            false,
        );
        assert_eq!(guide.watch.len(), 4, "canon+ww+contested+nev all watch");
        assert_eq!(guide.optional.len(), 0, "optional always empty in completionist");
        assert_eq!(ids(&guide.skipped), ["s1e2"]);
    }

    // ---- CanonOnly mode ----

    #[test]
    fn canon_only_moves_worth_watching_to_skipped() {
        let guide = build_skip_guide(
            &[
                ep(1, 1, 0, 0, 10),  // canon -> watch
                ep(1, 2, 10, 0, 0),  // filler -> skipped
                ep(1, 3, 0, 10, 0),  // worth watching -> skipped
                ep(1, 4, 5, 5, 0),   // contested -> watch
                ep(1, 5, 1, 1, 0),   // not enough votes -> watch
            ],
            GuideMode::CanonOnly,
            false,
        );
        assert_eq!(ids(&guide.watch), ["s1e1", "s1e4", "s1e5"]);
        assert_eq!(guide.optional.len(), 0, "optional always empty in canon-only");
        assert_eq!(ids(&guide.skipped), ["s1e2", "s1e3"]);
    }

    // ---- Binge mode ----

    #[test]
    fn binge_rescues_worth_watching_at_7_0() {
        let guide = build_skip_guide(
            &[
                ep(1, 1, 0, 0, 10),                              // canon -> watch
                ep_rated(1, 2, 0, 10, 0, Some(7.0)),             // ww + 7.0 -> watch
                ep_rated(1, 3, 0, 10, 0, Some(6.9)),             // ww + 6.9 -> skipped
                ep_rated(1, 4, 0, 10, 0, None),                  // ww + no rating -> skipped
                ep(1, 5, 10, 0, 0),                              // filler -> skipped
                ep(1, 6, 5, 5, 0),                               // contested -> watch
                ep(1, 7, 1, 1, 0),                               // not enough votes -> watch
            ],
            GuideMode::Binge,
            false,
        );
        assert_eq!(ids(&guide.watch), ["s1e1", "s1e2", "s1e6", "s1e7"]);
        assert_eq!(guide.optional.len(), 0, "optional always empty in binge");
        assert_eq!(ids(&guide.skipped), ["s1e3", "s1e4", "s1e5"]);
    }

    #[test]
    fn binge_exactly_7_0_is_watch() {
        let guide = build_skip_guide(
            &[ep_rated(1, 1, 0, 10, 0, Some(7.0))],
            GuideMode::Binge,
            false,
        );
        assert_eq!(guide.watch.len(), 1);
        assert_eq!(guide.skipped.len(), 0);
    }

    #[test]
    fn binge_6_9_is_skipped() {
        let guide = build_skip_guide(
            &[ep_rated(1, 1, 0, 10, 0, Some(6.9))],
            GuideMode::Binge,
            false,
        );
        assert_eq!(guide.watch.len(), 0);
        assert_eq!(guide.skipped.len(), 1);
    }

    #[test]
    fn binge_no_rating_is_skipped() {
        let guide = build_skip_guide(
            &[ep_rated(1, 1, 0, 10, 0, None)],
            GuideMode::Binge,
            false,
        );
        assert_eq!(guide.watch.len(), 0);
        assert_eq!(guide.skipped.len(), 1);
    }

    // ---- minutes_skipped ----

    #[test]
    fn minutes_skipped_uses_own_runtime() {
        let skipped = vec![
            ep_runtime(1, 1, 10, 0, 0, Some(42)),
            ep_runtime(1, 2, 10, 0, 0, Some(24)),
        ];
        let refs: Vec<&ScoredEpisode> = skipped.iter().collect();
        assert_eq!(minutes_skipped(&refs, &skipped), Some(66));
    }

    #[test]
    fn minutes_skipped_falls_back_to_show_average() {
        // All episodes have runtimes except the skipped one
        let all = vec![
            ep_runtime(1, 1, 0, 0, 10, Some(40)),
            ep_runtime(1, 2, 0, 0, 10, Some(60)),
            ep_runtime(1, 3, 10, 0, 0, None), // this one is skipped, no runtime
        ];
        let skipped = [all[2].clone()];
        let refs: Vec<&ScoredEpisode> = skipped.iter().collect();
        // avg of 40+60 = 50
        assert_eq!(minutes_skipped(&refs, &all), Some(50));
    }

    #[test]
    fn minutes_skipped_all_null_returns_none() {
        let all = vec![
            ep_runtime(1, 1, 10, 0, 0, None),
            ep_runtime(1, 2, 10, 0, 0, None),
        ];
        let refs: Vec<&ScoredEpisode> = all.iter().collect();
        assert_eq!(minutes_skipped(&refs, &all), None);
    }

    #[test]
    fn minutes_skipped_empty_skipped_returns_zero() {
        let all = vec![ep_runtime(1, 1, 0, 0, 10, Some(30))];
        assert_eq!(minutes_skipped(&[], &all), Some(0));
    }

    // ---- valid_modes in response ----

    #[test]
    fn skip_guide_includes_valid_modes() {
        let guide = build_skip_guide(&[ep(1, 1, 0, 0, 10)], GuideMode::Standard, false);
        assert_eq!(guide.valid_modes, ["completionist", "standard", "canon-only", "binge"]);
    }

    // ---- VoteReason validation matrix ----

    /// Every FILLER reason is valid only for FILLER, not for the other values.
    #[test]
    fn filler_reasons_valid_only_for_filler() {
        let filler_reasons = [
            VoteReason::Recap,
            VoteReason::SideStory,
            VoteReason::FunButSkippable,
        ];
        for r in &filler_reasons {
            assert!(r.is_valid_for(VoteValue::Filler), "{} should be valid for Filler", r.as_str());
            assert!(!r.is_valid_for(VoteValue::WorthWatching), "{} should not be valid for WorthWatching", r.as_str());
            assert!(!r.is_valid_for(VoteValue::Canon), "{} should not be valid for Canon", r.as_str());
        }
    }

    /// Every WORTH_WATCHING reason is valid only for WORTH_WATCHING.
    #[test]
    fn worth_watching_reasons_valid_only_for_worth_watching() {
        let ww_reasons = [
            VoteReason::SelfContainedGem,
            VoteReason::CharacterMoment,
            VoteReason::Worldbuilding,
        ];
        for r in &ww_reasons {
            assert!(!r.is_valid_for(VoteValue::Filler), "{} should not be valid for Filler", r.as_str());
            assert!(r.is_valid_for(VoteValue::WorthWatching), "{} should be valid for WorthWatching", r.as_str());
            assert!(!r.is_valid_for(VoteValue::Canon), "{} should not be valid for Canon", r.as_str());
        }
    }

    /// Every CANON reason is valid only for CANON.
    #[test]
    fn canon_reasons_valid_only_for_canon() {
        let canon_reasons = [
            VoteReason::MajorPlot,
            VoteReason::CharacterDevelopment,
            VoteReason::ArcSetup,
        ];
        for r in &canon_reasons {
            assert!(!r.is_valid_for(VoteValue::Filler), "{} should not be valid for Filler", r.as_str());
            assert!(!r.is_valid_for(VoteValue::WorthWatching), "{} should not be valid for WorthWatching", r.as_str());
            assert!(r.is_valid_for(VoteValue::Canon), "{} should be valid for Canon", r.as_str());
        }
    }

    /// `valid_for` returns exactly three tags per vote value.
    #[test]
    fn valid_for_returns_three_per_value() {
        assert_eq!(VoteReason::valid_for(VoteValue::Filler).len(), 3);
        assert_eq!(VoteReason::valid_for(VoteValue::WorthWatching).len(), 3);
        assert_eq!(VoteReason::valid_for(VoteValue::Canon).len(), 3);
    }

    /// Wire strings round-trip through `from_str` and `as_str`.
    #[test]
    fn vote_reason_wire_round_trip() {
        let all = [
            VoteReason::Recap,
            VoteReason::SideStory,
            VoteReason::FunButSkippable,
            VoteReason::SelfContainedGem,
            VoteReason::CharacterMoment,
            VoteReason::Worldbuilding,
            VoteReason::MajorPlot,
            VoteReason::CharacterDevelopment,
            VoteReason::ArcSetup,
        ];
        for r in &all {
            let s = r.as_str();
            assert_eq!(VoteReason::from_str(s), Some(*r), "round-trip failed for {s}");
        }
        assert_eq!(VoteReason::from_str("nonsense"), None);
    }

    /// `label()` returns a non-empty string for every reason.
    #[test]
    fn vote_reason_labels_are_non_empty() {
        let all = [
            VoteReason::Recap,
            VoteReason::SideStory,
            VoteReason::FunButSkippable,
            VoteReason::SelfContainedGem,
            VoteReason::CharacterMoment,
            VoteReason::Worldbuilding,
            VoteReason::MajorPlot,
            VoteReason::CharacterDevelopment,
            VoteReason::ArcSetup,
        ];
        for r in &all {
            assert!(!r.label().is_empty(), "{} has empty label", r.as_str());
        }
    }

    /// Serde serializes reasons as kebab-case strings.
    #[test]
    fn vote_reason_serializes_as_kebab_case() {
        let json = serde_json::to_string(&VoteReason::SideStory).unwrap();
        assert_eq!(json, r#""side-story""#);
        let json = serde_json::to_string(&VoteReason::CharacterDevelopment).unwrap();
        assert_eq!(json, r#""character-development""#);
        let json = serde_json::to_string(&VoteReason::FunButSkippable).unwrap();
        assert_eq!(json, r#""fun-but-skippable""#);
    }

    /// Serde deserializes kebab-case strings back to reasons.
    #[test]
    fn vote_reason_deserializes_from_kebab_case() {
        let r: VoteReason = serde_json::from_str(r#""major-plot""#).unwrap();
        assert_eq!(r, VoteReason::MajorPlot);
        let r: VoteReason = serde_json::from_str(r#""self-contained-gem""#).unwrap();
        assert_eq!(r, VoteReason::SelfContainedGem);
    }
}
