import { useId, useState } from "react";
import { X } from "lucide-react";
import type { NoteSummary } from "../lib/api";
import { addTag, removeTag } from "../lib/actions";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

/**
 * The note's tags as chips. Tags from the frontmatter can be removed here; tags that
 * only appear as #tag in the text are shown read-only (remove them by editing the text).
 */
export function TagBar({ note }: { note: NoteSummary }) {
  const tagCounts = useNotes((s) => s.tagCounts);
  const setView = useUi((s) => s.setView);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const listId = useId();

  const suggestions = tagCounts.map((t) => t.name).filter((n) => !note.tags.includes(n));

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (draft.trim() === "" || busy) return;
    setBusy(true);
    try {
      if (await addTag(note, draft)) setDraft("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-wrap items-center gap-1.5 px-7 pb-2" aria-label="Tags">
      {note.tags.map((tag) => {
        const inline = note.inlineTags.includes(tag);
        return (
          <span
            key={tag}
            className="inline-flex items-center gap-0.5 rounded-full border border-border bg-surface py-0.5 pr-1 pl-2 text-xs"
            title={inline ? "Written in the text as #tag; edit the text to remove it" : undefined}
          >
            <button
              type="button"
              onClick={() => setView({ kind: "tag", value: tag })}
              className="hover:underline"
              aria-label={`Show notes tagged ${tag}`}
            >
              #{tag}
            </button>
            {inline ? (
              <span className="w-1" />
            ) : (
              <button
                type="button"
                aria-label={`Remove tag ${tag}`}
                onClick={() => void removeTag(note, tag)}
                className="rounded-full p-0.5 text-fg-muted hover:bg-surface-hover"
              >
                <X aria-hidden className="size-3" />
              </button>
            )}
          </span>
        );
      })}
      <form onSubmit={(e) => void submit(e)}>
        <input
          list={listId}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Add tag"
          aria-label="Add tag"
          className="w-24 rounded-full border border-transparent bg-transparent px-2 py-0.5 text-xs placeholder:text-fg-muted hover:border-border focus:border-border focus:bg-surface"
        />
        <datalist id={listId}>
          {suggestions.map((n) => (
            <option key={n} value={n} />
          ))}
        </datalist>
      </form>
    </div>
  );
}
