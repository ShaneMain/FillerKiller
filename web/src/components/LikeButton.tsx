import { useState } from "react";
import { Link } from "react-router-dom";
import { likeGuide, unlikeGuide } from "../lib/api";
import { useLoginHref } from "../lib/loginNav";

/**
 * Heart toggle for a published guide. Signed-out users see a link to sign in.
 */
export function LikeButton({
  guideId,
  initialCount,
  initialLiked,
  signedIn,
}: {
  guideId: string;
  initialCount: number;
  initialLiked: boolean;
  signedIn: boolean;
}) {
  const [count, setCount] = useState(initialCount);
  const [liked, setLiked] = useState(initialLiked);
  const [busy, setBusy] = useState(false);
  const loginHref = useLoginHref();

  if (!signedIn) {
    return (
      <Link
        to={loginHref}
        title="Sign in to like"
        className="inline-flex items-center gap-1 rounded-md border border-zinc-700 px-2.5 py-1 text-sm text-zinc-400 hover:bg-zinc-800"
      >
        ♡ {count}
      </Link>
    );
  }

  async function toggle() {
    if (busy) return;
    setBusy(true);
    try {
      const res = liked ? await unlikeGuide(guideId) : await likeGuide(guideId);
      setCount(res.likeCount);
      setLiked(res.myLike);
    } catch {
      /* leave state as-is on failure */
    } finally {
      setBusy(false);
    }
  }

  return (
    <button
      onClick={() => void toggle()}
      disabled={busy}
      aria-pressed={liked}
      aria-label={liked ? "Unlike this guide" : "Like this guide"}
      className={`inline-flex items-center gap-1 rounded-md border px-2.5 py-1 text-sm transition ${
        liked
          ? "border-rose-700 bg-rose-950/40 text-rose-300"
          : "border-zinc-700 text-zinc-300 hover:bg-zinc-800"
      }`}
    >
      {liked ? "♥" : "♡"} {count}
    </button>
  );
}
