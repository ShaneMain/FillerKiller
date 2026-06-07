//! Filler scoring and skip-guide derivation.
//!
//! Single source of truth in code for the math in the design notes
//!. The constants MUST match that document — changing them
//! is a spec change. Pure functions only: no I/O, no DB.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EpisodeStatus {
    Canon,
    WorthWatching,
    Filler,
    Contested,
    NotEnoughVotes,
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

/// How to treat Contested / NotEnoughVotes episodes in the skip guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestedHandling {
    /// Keep in the watch list (safe default).
    Canon,
    /// Put in the skipped list (aggressive binge).
    Filler,
    /// Keep in watch but flagged. Used by the skip-guide endpoint's
    /// `contested=show` option (wired up next).
    Show,
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
}

/// Build a show's skip guide from its scored episodes.
///
/// Canon → watch, Worth Watching → optional, Filler → skipped. When unsure
/// (Contested / NotEnoughVotes) the safe default keeps the episode in the watch
/// list — wrongly skipping canon is worse than wrongly watching filler;
/// `contested` overrides that bias. Specials (season 0) are excluded unless
/// `include_specials` is set.
pub fn build_skip_guide(
    episodes: &[ScoredEpisode],
    contested: ContestedHandling,
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

    for ep in ordered {
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

        match st {
            EpisodeStatus::Filler => skipped.push(entry),
            EpisodeStatus::WorthWatching => optional.push(entry),
            EpisodeStatus::Canon => watch.push(entry),
            // Contested or NotEnoughVotes
            _ => {
                if contested == ContestedHandling::Filler {
                    skipped.push(entry);
                } else {
                    watch.push(entry);
                }
            }
        }
    }

    SkipGuide {
        watch,
        optional,
        skipped,
        thresholds: Thresholds {
            min_votes: MIN_VOTES,
            contested_margin: CONTESTED_MARGIN,
        },
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
        }
    }

    fn ids(entries: &[SkipGuideEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.episode_id.as_str()).collect()
    }

    #[test]
    fn skip_guide_sorts_into_watch_optional_skipped() {
        let guide = build_skip_guide(
            &[
                ep(1, 1, 0, 0, 10), // canon -> watch
                ep(1, 2, 10, 0, 0), // filler -> skipped
                ep(1, 3, 0, 10, 0), // worth watching -> optional
            ],
            ContestedHandling::Canon,
            false,
        );
        assert_eq!(ids(&guide.watch), ["s1e1"]);
        assert_eq!(ids(&guide.optional), ["s1e3"]);
        assert_eq!(ids(&guide.skipped), ["s1e2"]);
    }

    #[test]
    fn skip_guide_contested_defaults_to_watch() {
        let eps = [ep(1, 1, 5, 5, 0), ep(1, 2, 1, 1, 0)]; // contested, not-enough
        let guide = build_skip_guide(&eps, ContestedHandling::Canon, false);
        assert_eq!(guide.watch.len(), 2);
        assert_eq!(guide.skipped.len(), 0);
    }

    #[test]
    fn skip_guide_contested_filler_skips_borderline() {
        let eps = [ep(1, 1, 5, 5, 0), ep(1, 2, 1, 1, 0)];
        let guide = build_skip_guide(&eps, ContestedHandling::Filler, false);
        assert_eq!(guide.skipped.len(), 2);
        assert_eq!(guide.watch.len(), 0);
    }

    #[test]
    fn skip_guide_specials_excluded_by_default() {
        let eps = [ep(0, 1, 0, 0, 10), ep(1, 1, 0, 0, 10)];
        assert_eq!(build_skip_guide(&eps, ContestedHandling::Canon, false).watch.len(), 1);
        assert_eq!(build_skip_guide(&eps, ContestedHandling::Canon, true).watch.len(), 2);
    }

    #[test]
    fn skip_guide_serializes_camelcase_per_spec() {
        let guide = build_skip_guide(&[ep(1, 1, 10, 0, 0)], ContestedHandling::Canon, false);
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
}
