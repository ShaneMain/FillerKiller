//! Filler scoring and skip-guide derivation.
//!
//! Single source of truth in code for the math in the design notes
//!. The threshold constants MUST match that document —
//! changing them is a spec change. Pure functions only: no I/O, no DB.

// The scoring/aggregate API is consumed by the catalog + vote endpoints; the
// skip-guide types (build_skip_guide, SkipGuide, ...) are implemented and tested
// but not yet wired to an endpoint, so the binary sees those as dead. Remove
// this once the skip-guide endpoint lands.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Minimum total votes before we show a confident label at all.
pub const MIN_VOTES: i64 = 5;
/// fillerScore strictly below this → Canon.
pub const CANON_BELOW: f64 = 0.4;
/// fillerScore strictly above this → Filler.
pub const FILLER_ABOVE: f64 = 0.6;

/// A user's vote. Wire format is `FILLER`/`CANON` (request body + responses);
/// the same strings are the Postgres `vote_value` enum labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VoteValue {
    Filler,
    Canon,
}

impl VoteValue {
    /// The Postgres `vote_value` label for this vote.
    pub fn as_db(&self) -> &'static str {
        match self {
            VoteValue::Filler => "FILLER",
            VoteValue::Canon => "CANON",
        }
    }

    /// Parse a Postgres `vote_value` label back into a `VoteValue`.
    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "FILLER" => Some(VoteValue::Filler),
            "CANON" => Some(VoteValue::Canon),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EpisodeStatus {
    Canon,
    Filler,
    Contested,
    NotEnoughVotes,
}

/// `filler / total`, or `None` when there are no votes.
pub fn filler_score(filler_votes: i64, canon_votes: i64) -> Option<f64> {
    let total = filler_votes + canon_votes;
    if total <= 0 {
        return None;
    }
    Some(filler_votes as f64 / total as f64)
}

/// Derive the displayed status, applying the confidence floor (`MIN_VOTES`)
/// before the score thresholds.
pub fn status(filler_votes: i64, canon_votes: i64) -> EpisodeStatus {
    let total = filler_votes + canon_votes;
    if total < MIN_VOTES {
        return EpisodeStatus::NotEnoughVotes;
    }
    let s = filler_votes as f64 / total as f64;
    if s < CANON_BELOW {
        EpisodeStatus::Canon
    } else if s > FILLER_ABOVE {
        EpisodeStatus::Filler
    } else {
        EpisodeStatus::Contested
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
    pub canon_below: f64,
    pub filler_above: f64,
    pub min_votes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipGuide {
    pub watch: Vec<SkipGuideEntry>,
    pub skipped: Vec<SkipGuideEntry>,
    pub thresholds: Thresholds,
}

/// Build a show's skip guide from its scored episodes.
///
/// Safe default: when unsure (Contested / NotEnoughVotes), keep the episode in
/// the watch list, because wrongly skipping canon is worse than wrongly watching
/// filler. `contested` overrides that bias. Specials (season 0) are excluded
/// from the watch order unless `include_specials` is set.
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
    let mut skipped = Vec::new();

    for ep in ordered {
        if ep.season_number == 0 && !include_specials {
            continue;
        }

        let st = status(ep.filler_votes, ep.canon_votes);
        let entry = SkipGuideEntry {
            episode_id: ep.episode_id.clone(),
            season_number: ep.season_number,
            episode_number: ep.episode_number,
            name: ep.name.clone(),
            status: st,
        };

        let skip = match st {
            EpisodeStatus::Filler => true,
            EpisodeStatus::Canon => false,
            // Contested or NotEnoughVotes
            _ => contested == ContestedHandling::Filler,
        };

        if skip {
            skipped.push(entry);
        } else {
            watch.push(entry);
        }
    }

    SkipGuide {
        watch,
        skipped,
        thresholds: Thresholds {
            canon_below: CANON_BELOW,
            filler_above: FILLER_ABOVE,
            min_votes: MIN_VOTES,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filler_score_none_with_no_votes() {
        assert_eq!(filler_score(0, 0), None);
    }

    #[test]
    fn filler_score_fraction() {
        assert_eq!(filler_score(12, 88), Some(0.12));
        assert_eq!(filler_score(1, 1), Some(0.5));
        assert_eq!(filler_score(3, 0), Some(1.0));
    }

    #[test]
    fn status_below_min_votes_is_not_enough() {
        // 4 unanimous filler votes still isn't enough to label.
        assert_eq!(status(4, 0), EpisodeStatus::NotEnoughVotes);
        assert_eq!(status(0, 4), EpisodeStatus::NotEnoughVotes);
        assert_eq!(status(0, 0), EpisodeStatus::NotEnoughVotes);
    }

    #[test]
    fn status_canon_below_threshold() {
        assert_eq!(status(1, 9), EpisodeStatus::Canon); // 0.10
        assert_eq!(status(12, 88), EpisodeStatus::Canon); // 0.12
    }

    #[test]
    fn status_filler_above_threshold() {
        assert_eq!(status(9, 1), EpisodeStatus::Filler); // 0.90
        assert_eq!(status(7, 3), EpisodeStatus::Filler); // 0.70
    }

    #[test]
    fn status_contested_band_inclusive() {
        assert_eq!(status(5, 5), EpisodeStatus::Contested); // 0.50
        assert_eq!(status(4, 6), EpisodeStatus::Contested); // 0.40 boundary
        assert_eq!(status(6, 4), EpisodeStatus::Contested); // 0.60 boundary
    }

    #[test]
    fn status_exactly_min_votes_is_enough() {
        assert_eq!(MIN_VOTES, 5);
        assert_eq!(status(5, 0), EpisodeStatus::Filler);
    }

    fn ep(season: i32, episode: i32, filler: i64, canon: i64) -> ScoredEpisode {
        ScoredEpisode {
            episode_id: format!("s{season}e{episode}"),
            season_number: season,
            episode_number: episode,
            name: None,
            filler_votes: filler,
            canon_votes: canon,
        }
    }

    fn ids(entries: &[SkipGuideEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.episode_id.as_str()).collect()
    }

    #[test]
    fn skip_guide_canon_watched_filler_skipped_ordered() {
        let guide = build_skip_guide(
            &[ep(1, 2, 9, 1), ep(1, 1, 1, 9), ep(1, 3, 0, 10)],
            ContestedHandling::Canon,
            false,
        );
        assert_eq!(ids(&guide.watch), ["s1e1", "s1e3"]);
        assert_eq!(ids(&guide.skipped), ["s1e2"]);
    }

    #[test]
    fn skip_guide_contested_defaults_to_watch() {
        let eps = [ep(1, 1, 5, 5), ep(1, 2, 1, 1)]; // contested, not-enough
        let guide = build_skip_guide(&eps, ContestedHandling::Canon, false);
        assert_eq!(guide.watch.len(), 2);
        assert_eq!(guide.skipped.len(), 0);
    }

    #[test]
    fn skip_guide_contested_filler_skips_borderline() {
        let eps = [ep(1, 1, 5, 5), ep(1, 2, 1, 1)];
        let guide = build_skip_guide(&eps, ContestedHandling::Filler, false);
        assert_eq!(guide.watch.len(), 0);
        assert_eq!(guide.skipped.len(), 2);
    }

    #[test]
    fn skip_guide_specials_excluded_by_default() {
        let eps = [ep(0, 1, 1, 9), ep(1, 1, 1, 9)];
        assert_eq!(
            build_skip_guide(&eps, ContestedHandling::Canon, false)
                .watch
                .len(),
            1
        );
        assert_eq!(
            build_skip_guide(&eps, ContestedHandling::Canon, true)
                .watch
                .len(),
            2
        );
    }

    #[test]
    fn skip_guide_serializes_camelcase_per_spec() {
        // Wire contract is camelCase; vote/status enums are
        // SCREAMING_SNAKE_CASE. Lock both so they can't silently drift.
        let guide = build_skip_guide(&[ep(1, 1, 9, 1)], ContestedHandling::Canon, false);
        let json = serde_json::to_string(&guide).unwrap();
        assert!(json.contains("\"episodeId\""), "{json}");
        assert!(json.contains("\"seasonNumber\""), "{json}");
        assert!(json.contains("\"canonBelow\""), "{json}");
        assert!(json.contains("\"minVotes\""), "{json}");
        assert!(json.contains("\"FILLER\""), "{json}");
        assert!(!json.contains("episode_id"), "{json}");
    }

    #[test]
    fn skip_guide_sorts_across_seasons() {
        let guide = build_skip_guide(
            &[ep(2, 1, 0, 10), ep(1, 10, 0, 10), ep(1, 2, 0, 10)],
            ContestedHandling::Canon,
            false,
        );
        assert_eq!(ids(&guide.watch), ["s1e2", "s1e10", "s2e1"]);
    }
}
