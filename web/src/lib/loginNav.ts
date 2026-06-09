import { useLocation } from "react-router-dom";

/** The `/login` href that returns the user to the current page after sign-in. */
export function useLoginHref(): string {
  const loc = useLocation();
  const from = loc.pathname + loc.search;
  return from && from !== "/login" ? `/login?next=${encodeURIComponent(from)}` : "/login";
}
