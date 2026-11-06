import { useEffect, useRef } from "react";
import { Plus, Search } from "lucide-react";
import { selectNotes, viewLabel, type SortKey } from "../lib/notes";
import { formatDate } from "../lib/format";
import { createNote, openNote } from "../lib/actions";
import { cn } from "../lib/cn";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

const SORTS: { value: SortKey; label: string }[] = [
  { value: "updated", label: "Last modified" },
  { value: "created", label: "Created" },
  { value: "title", label: "Title" },
];

export function NoteExplorer() {
  const notes = useNotes((s) => s.notes);
  const status = useNotes((s) => s.status);
  const error = useNotes((s) => s.error);
  const { view, sort, query, selectedNoteId, searchFocusTick } = useUi();
  const setSort = useUi((s) => s.setSort);
  const setQuery = useUi((s) => s.setQuery);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (searchFocusTick > 0) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [searchFocusTick]);

  const visible = selectNotes(notes, view, sort, query);

  return (
    <section aria-label="Note explorer" className="flex h-full flex-col bg-bg">
      <header className="flex items-center justify-between px-3 pt-3 pb-2">
        <h2 className="truncate text-sm font-semibold">{viewLabel(view)}</h2>
        <button
          type="button"
          aria-label="New note"
          title="New note"
          onClick={() => void createNote()}
          className="rounded-md p-1 text-fg-muted hover:bg-surface-hover"
        >
          <Plus aria-hidden className="size-4" />
        </button>
      </header>

      <div className="space-y-2 px-3 pb-2">
        <label className="relative block">
          <span className="sr-only">Search notes</span>
          <Search
            aria-hidden
            className="pointer-events-none absolute top-1/2 left-2 size-4 -translate-y-1/2 text-fg-muted"
          />
          <input
            ref={inputRef}
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search notes"
            className="w-full rounded-md border border-border bg-surface py-1.5 pr-2 pl-8 text-sm placeholder:text-fg-muted"
          />
        </label>
        <label className="flex items-center gap-2 text-xs text-fg-muted">
          Sort by
          <select
            value={sort}
            onChange={(e) => setSort(e.target.value as SortKey)}
            className="rounded-md border border-border bg-surface px-1.5 py-1 text-xs text-fg"
          >
            {SORTS.map((s) => (
              <option key={s.value} value={s.value}>
                {s.label}
              </option>
            ))}
          </select>
        </label>
      </div>

      <p className="mx-3 mb-2 rounded-md bg-surface px-2 py-1 text-xs text-fg-muted">
        Showing sample data. Nothing is saved to disk yet.
      </p>

      {error && (
        <p role="alert" className="mx-3 mb-2 rounded-md border border-border px-2 py-1 text-xs">
          {error}
        </p>
      )}

      <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {status === "loading" && <li className="px-2 py-2 text-sm text-fg-muted">Loading…</li>}
        {status === "ready" && visible.length === 0 && (
          <li className="px-2 py-2 text-sm text-fg-muted">
            {query ? "No notes match your search." : "No notes here yet."}
          </li>
        )}
        {visible.map((note) => {
          const selected = note.id === selectedNoteId;
          return (
            <li key={note.id}>
              <button
                type="button"
                aria-current={selected ? "true" : undefined}
                onClick={() => void openNote(note.id)}
                className={cn(
                  "w-full rounded-md px-2 py-2 text-left",
                  selected ? "bg-surface-active" : "hover:bg-surface-hover",
                )}
              >
                <div className="truncate text-sm font-medium">{note.title}</div>
                {note.preview && (
                  <div className="truncate text-xs text-fg-muted">{note.preview}</div>
                )}
                <div className="mt-0.5 flex gap-2 truncate text-xs text-fg-muted">
                  <span>{formatDate(note.updatedAt)}</span>
                  {note.folder && <span className="truncate">{note.folder}</span>}
                </div>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
