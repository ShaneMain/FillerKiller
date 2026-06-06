import { test } from "node:test";
import assert from "node:assert/strict";
import {
  fillerScore,
  status,
  buildSkipGuide,
  MIN_VOTES,
  type ScoredEpisode,
} from "./scoring.ts";

test("fillerScore: null with no votes", () => {
  assert.equal(fillerScore(0, 0), null);
});

test("fillerScore: fraction of filler votes", () => {
  assert.equal(fillerScore(12, 88), 0.12);
  assert.equal(fillerScore(1, 1), 0.5);
  assert.equal(fillerScore(3, 0), 1);
});

test("status: below MIN_VOTES is NOT_ENOUGH_VOTES regardless of score", () => {
  // 4 unanimous filler votes still isn't enough to label.
  assert.equal(status(4, 0), "NOT_ENOUGH_VOTES");
  assert.equal(status(0, 4), "NOT_ENOUGH_VOTES");
  assert.equal(status(0, 0), "NOT_ENOUGH_VOTES");
});

test("status: CANON when score below 0.40 with enough votes", () => {
  assert.equal(status(1, 9), "CANON"); // 0.10
  assert.equal(status(12, 88), "CANON"); // 0.12
});

test("status: FILLER when score above 0.60 with enough votes", () => {
  assert.equal(status(9, 1), "FILLER"); // 0.90
  assert.equal(status(7, 3), "FILLER"); // 0.70
});

test("status: CONTESTED in the [0.40, 0.60] band", () => {
  assert.equal(status(5, 5), "CONTESTED"); // 0.50
  assert.equal(status(4, 6), "CONTESTED"); // 0.40 (boundary, not < 0.40)
  assert.equal(status(6, 4), "CONTESTED"); // 0.60 (boundary, not > 0.60)
});

test("status: exactly MIN_VOTES is enough", () => {
  assert.equal(MIN_VOTES, 5);
  assert.equal(status(5, 0), "FILLER");
});

const ep = (
  seasonNumber: number,
  episodeNumber: number,
  fillerVotes: number,
  canonVotes: number,
): ScoredEpisode => ({
  episodeId: `s${seasonNumber}e${episodeNumber}`,
  seasonNumber,
  episodeNumber,
  name: null,
  fillerVotes,
  canonVotes,
});

test("buildSkipGuide: canon watched, filler skipped, ordered", () => {
  const guide = buildSkipGuide([
    ep(1, 2, 9, 1), // filler
    ep(1, 1, 1, 9), // canon
    ep(1, 3, 0, 10), // canon
  ]);
  assert.deepEqual(
    guide.watch.map((e) => e.episodeId),
    ["s1e1", "s1e3"],
  );
  assert.deepEqual(
    guide.skipped.map((e) => e.episodeId),
    ["s1e2"],
  );
});

test("buildSkipGuide: contested defaults to watch (safe)", () => {
  const eps = [ep(1, 1, 5, 5), ep(1, 2, 1, 1)]; // contested, not-enough
  const guide = buildSkipGuide(eps);
  assert.equal(guide.watch.length, 2);
  assert.equal(guide.skipped.length, 0);
});

test("buildSkipGuide: contested=filler skips borderline episodes", () => {
  const eps = [ep(1, 1, 5, 5), ep(1, 2, 1, 1)];
  const guide = buildSkipGuide(eps, "filler");
  assert.equal(guide.watch.length, 0);
  assert.equal(guide.skipped.length, 2);
});

test("buildSkipGuide: specials excluded by default, included on opt-in", () => {
  const eps = [ep(0, 1, 1, 9), ep(1, 1, 1, 9)];
  assert.equal(buildSkipGuide(eps).watch.length, 1);
  assert.equal(buildSkipGuide(eps, "canon", true).watch.length, 2);
});

test("buildSkipGuide: sorts across seasons", () => {
  const guide = buildSkipGuide([ep(2, 1, 0, 10), ep(1, 10, 0, 10), ep(1, 2, 0, 10)]);
  assert.deepEqual(
    guide.watch.map((e) => e.episodeId),
    ["s1e2", "s1e10", "s2e1"],
  );
});
