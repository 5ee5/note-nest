import { useEffect, useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import { toApiError } from "../lib/api";
import { openExternal } from "../lib/openExternal";
import { useNotes } from "../store/notes";

/** Delay after the last change before the preview re-renders. */
export const PREVIEW_DEBOUNCE_MS = 200;
/** Above this many characters the preview is rendered only on request. */
export const LARGE_PREVIEW_CHARS = 2_000_000;

interface Props {
  noteId: string;
  /** The note's full text, including frontmatter (the backend strips it). */
  content: string;
}

/**
 * Rendered Markdown. The HTML comes from the backend already sanitized (see
 * crates/notenest-core/src/markdown.rs); this component never builds HTML itself.
 * Links never navigate the webview: http, https and mailto open externally, all
 * others do nothing.
 */
export function MarkdownPreview({ noteId, content }: Props) {
  const large = content.length > LARGE_PREVIEW_CHARS;
  const [html, setHtml] = useState("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [renderedText, setRenderedText] = useState<string | null>(null);
  const latest = useRef(0);
  const first = useRef(true);

  const render = async (text: string) => {
    const ticket = ++latest.current;
    try {
      const out = await useNotes.getState().api.renderMarkdown(noteId, text);
      if (ticket !== latest.current) return; // a newer render superseded this one
      setHtml(out.html);
      setWarnings(out.warnings);
      setError(null);
      setRenderedText(text);
    } catch (e) {
      if (ticket !== latest.current) return;
      setError(toApiError(e).message);
    }
  };

  useEffect(() => {
    if (large) {
      latest.current++; // drop anything in flight
      return;
    }
    const delay = first.current ? 0 : PREVIEW_DEBOUNCE_MS;
    first.current = false;
    const timer = setTimeout(() => void render(content), delay);
    return () => clearTimeout(timer);
    // `render` only reads refs, state setters and stable props.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [content, noteId, large]);

  // Derived, not stored: the shown HTML is out of date when the text has moved on.
  const stale = renderedText !== content;

  const onClick = (e: React.MouseEvent) => {
    const anchor = (e.target as HTMLElement).closest("a");
    if (!anchor) return;
    e.preventDefault(); // the webview must never navigate away from the app
    const href = anchor.getAttribute("href") ?? "";
    void openExternal(href);
  };

  return (
    <section aria-label="Rendered preview" className="flex h-full min-h-0 flex-col">
      {large && (
        <div role="status" className="mx-7 mt-3 rounded-md border border-border px-3 py-2 text-xs">
          This note is large, so the preview is rendered on request.
          <button
            type="button"
            onClick={() => void render(content)}
            className="ml-2 inline-flex items-center gap-1 rounded border border-border px-2 py-0.5 hover:bg-surface-hover"
          >
            <RefreshCw aria-hidden className="size-3" />
            {stale ? "Render preview" : "Refresh"}
          </button>
        </div>
      )}
      {error && (
        <p role="alert" className="mx-7 mt-3 rounded-md border border-border px-3 py-2 text-xs">
          Preview unavailable: {error}
        </p>
      )}
      {warnings.length > 0 && (
        <details className="mx-7 mt-3 text-xs text-fg-muted">
          <summary>{warnings.length} image(s) could not be shown</summary>
          <ul className="mt-1 list-disc pl-5">
            {warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        </details>
      )}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div
          className="markdown-preview mx-auto max-w-3xl px-7 pt-3 pb-24"
          onClick={onClick}
          // Sanitized by the backend (ammonia allowlist); see markdown.rs.
          dangerouslySetInnerHTML={{ __html: html }}
        />
      </div>
    </section>
  );
}
