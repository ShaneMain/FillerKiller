import { useLocation } from "react-router-dom";

/**
 * Whether `next` is a safe in-app return path: a single leading `/` (not the
 * protocol-relative `//`), no backslashes, bounded length. The server's
 * `safe_next` is the authoritative check — this mirrors it as defense in depth
 * so the client never forwards or follows an off-site redirect target.
 */
export function isSafeNext(next: string): boolean {
  return (
    next.startsWith("/") &&
    !next.startsWith("//") &&
    !next.includes("\\") &&
    next.length <= 512
  );
}

/** The `/login` href that returns the user to the current page after sign-in. */
export function useLoginHref(): string {
  const loc = useLocation();
  const from = loc.pathname + loc.search;
  return from && from !== "/login" && isSafeNext(from)
    ? `/login?next=${encodeURIComponent(from)}`
    : "/login";
}
