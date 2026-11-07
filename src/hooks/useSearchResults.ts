import { useEffect, useState } from "react";
import { toApiError, type SearchHit } from "../lib/api";
import { useNotes } from "../store/notes";

/** Delay after the last keystroke before the search runs. */
export const SEARCH_DEBOUNCE_MS = 150;

export interface SearchState {
  /** Results of the latest completed search, or null when there is no query. */
  hits: SearchHit[] | null;
  /** A search for the current text is still running (earlier hits may be shown meanwhile). */
  pending: boolean;
  error: string | null;
}

/**
 * Backend full-text search for the search box. Debounced, and answers that arrive for an
 * outdated query are dropped. Searches again when the note list changes, so results do
 * not go stale after an edit.
 */
export function useSearchResults(query: string): SearchState {
  const q = query.trim();
  const notes = useNotes((s) => s.notes);
  const [done, setDone] = useState<{ q: string; hits: SearchHit[]; error: string | null } | null>(
    null,
  );

  useEffect(() => {
    if (q === "") return;
    let cancelled = false;
    const timer = setTimeout(() => {
      useNotes
        .getState()
        .api.searchNotes(q, 50)
        .then((hits) => !cancelled && setDone({ q, hits, error: null }))
        .catch((e) => !cancelled && setDone({ q, hits: [], error: toApiError(e).message }));
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [q, notes]);

  if (q === "") return { hits: null, pending: false, error: null };
  return { hits: done?.hits ?? null, pending: done?.q !== q, error: done?.error ?? null };
}
