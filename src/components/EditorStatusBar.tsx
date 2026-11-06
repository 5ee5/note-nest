import { useEffect, useMemo, useState } from "react";
import { noteStats, type TextStats } from "../lib/stats";
import { useEditorView } from "../store/editorView";

const number = new Intl.NumberFormat();
const plural = (n: number, one: string, many: string) =>
  `${number.format(n)} ${n === 1 ? one : many}`;

/** Notes up to this size are counted on every change; larger ones after a pause. */
const LIVE_COUNT_CHARS = 50_000;
const LARGE_COUNT_DELAY_MS = 400;

/** Counts for the note; for large notes they trail the typing instead of blocking it. */
function useNoteStats(content: string): TextStats {
  const small = content.length <= LIVE_COUNT_CHARS;
  const live = useMemo(() => (small ? noteStats(content) : null), [content, small]);
  const [trailing, setTrailing] = useState<TextStats>(() => noteStats(content));
  useEffect(() => {
    if (small) return;
    const timer = setTimeout(() => setTrailing(noteStats(content)), LARGE_COUNT_DELAY_MS);
    return () => clearTimeout(timer);
  }, [content, small]);
  return live ?? trailing;
}

/** Word and character counts for the note, plus the current selection. */
export function EditorStatusBar({ content }: { content: string }) {
  const stats = useNoteStats(content);
  const selection = useEditorView((s) => s.selection);

  return (
    <footer
      aria-label="Document statistics"
      className="flex items-center gap-3 border-t border-border px-7 py-1 text-xs text-fg-muted"
    >
      <span>{plural(stats.words, "word", "words")}</span>
      <span>{plural(stats.characters, "character", "characters")}</span>
      {selection && (
        <span className="ml-auto text-fg">
          {selection.words > 0 ? `${plural(selection.words, "word", "words")}, ` : ""}
          {plural(selection.characters, "character", "characters")} selected
        </span>
      )}
    </footer>
  );
}
