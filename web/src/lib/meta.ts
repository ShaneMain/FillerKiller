// Lightweight per-page document metadata for an SPA: sets the tab/title and the
// meta description as the user navigates. This is a client-side SPA with no SSR,
// so static crawlers see only the shell in index.html; modern JS-executing
// crawlers (and the in-tab title/history) get these per-page values. Full
// prerender/SSR remains a known follow-up for richer link unfurling.
import { useEffect } from "react";

export const SITE_NAME = "FillerKiller";
export const DEFAULT_TITLE = "FillerKiller — skip the filler";
export const DEFAULT_DESCRIPTION =
  "Crowd-sourced guide to which TV episodes are filler, worth it, or canon — so you can skip the fluff and watch what matters.";

function setMetaDescription(content: string) {
  let el = document.head.querySelector<HTMLMetaElement>('meta[name="description"]');
  if (!el) {
    el = document.createElement("meta");
    el.setAttribute("name", "description");
    document.head.appendChild(el);
  }
  el.setAttribute("content", content);
}

/**
 * Set the page title (suffixed with the site name) and, optionally, the meta
 * description. Pass `undefined` for the title to use the site default.
 */
export function usePageMeta(title?: string, description?: string) {
  useEffect(() => {
    document.title = title ? `${title} · ${SITE_NAME}` : DEFAULT_TITLE;
    setMetaDescription(description ?? DEFAULT_DESCRIPTION);
  }, [title, description]);
}
