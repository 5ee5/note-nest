import { useEffect, useRef, useState } from "react";
import { Folder, FileText, Plus, Search, Star } from "lucide-react";
import { toApiError, type BrokenLink, type NoteRef } from "../lib/api";
import { selectNotes, viewLabel, type SortKey } from "../lib/notes";
import { formatDate } from "../lib/format";
import { createNote, openNote, restoreFromTrash } from "../lib/actions";
import { cn } from "../lib/cn";
import { useSearchResults } from "../hooks/useSearchResults";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";
import { Highlighted } from "./Highlighted";

const SORTS: { value: SortKey; label: string }[] = [
  { value: "updated", label: "Last modified" },
  { value: "created", label: "Created" },
  { value: "title", label: "Title" },
];

function TrashList() {
  const trash = useNotes((s) => s.trash);
  if (trash.length === 0) {
    return <p className="px-3 py-2 text-sm text-fg-muted">Trash is empty.</p>;
  }
  return (
    <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
      {trash.map((item) => (
        <li key={item.id} className="flex items-center gap-2 rounded-md px-2 py-2">
          {item.isDir ? (
            <Folder aria-hidden className="size-4 shrink-0 text-fg-muted" />
          ) : (
            <FileText aria-hidden className="size-4 shrink-0 text-fg-muted" />
          )}
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm">{item.originalPath}</div>
            <div className="text-xs text-fg-muted">Deleted {formatDate(item.trashedAt)}</div>
          </div>
          <button
            type="button"
            onClick={() => void restoreFromTrash(item.id)}
            className="rounded-md border border-border px-2 py-1 text-xs hover:bg-surface-hover"
          >
            Restore
          </button>
        </li>
      ))}
    </ul>
  );
}

/** Loads something from the backend whenever the note list changes (async, so setState is fine). */
function useBackendList<T>(load: () => Promise<T[]>): { items: T[] | null; error: string | null } {
  const notes = useNotes((s) => s.notes);
  const [state, setState] = useState<{ items: T[] | null; error: string | null }>({
    items: null,
    error: null,
  });
  useEffect(() => {
    let cancelled = false;
    load()
      .then((items) => !cancelled && setState({ items, error: null }))
      .catch((e) => !cancelled && setState({ items: null, error: toApiError(e).message }));
    return () => {
      cancelled = true;
    };
    // `load` is a fresh closure each render; the data only depends on the notes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [notes]);
  return state;
}

function ListMessage({ children }: { children: React.ReactNode }) {
  return <p className="px-3 py-2 text-sm text-fg-muted">{children}</p>;
}

/** Notes that are not linked to or from any other note. */
function OrphanList() {
  const { items, error } = useBackendList<NoteRef>(() => useNotes.getState().api.listOrphans());
  if (error) return <ListMessage>{error}</ListMessage>;
  if (!items) return <ListMessage>Loading…</ListMessage>;
  if (items.length === 0)
    return <ListMessage>Every note is connected to another note.</ListMessage>;
  return (
    <ul aria-label="Orphaned notes" className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
      {items.map((n) => (
        <li key={n.id}>
          <button
            type="button"
            onClick={() => void openNote(n.id)}
            className="w-full rounded-md px-2 py-2 text-left hover:bg-surface-hover"
          >
            <div className="truncate text-sm font-medium">{n.title}</div>
            <div className="truncate text-xs text-fg-muted">{n.path}</div>
          </button>
        </li>
      ))}
    </ul>
  );
}

/** Links that point at no note, grouped by the note they are written in. */
function BrokenList() {
  const { items, error } = useBackendList<BrokenLink>(() =>
    useNotes.getState().api.listBrokenLinks(),
  );
  if (error) return <ListMessage>{error}</ListMessage>;
  if (!items) return <ListMessage>Loading…</ListMessage>;
  if (items.length === 0) return <ListMessage>No broken links.</ListMessage>;
  const bySource = new Map<string, BrokenLink[]>();
  for (const l of items) bySource.set(l.source.id, [...(bySource.get(l.source.id) ?? []), l]);
  return (
    <ul aria-label="Broken links" className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
      {[...bySource.values()].map((links) => (
        <li key={links[0]!.source.id} className="px-2 py-2">
          <button
            type="button"
            onClick={() => void openNote(links[0]!.source.id)}
            className="w-full rounded-md text-left hover:bg-surface-hover"
          >
            <div className="truncate text-sm font-medium">{links[0]!.source.title}</div>
            <div className="truncate text-xs text-fg-muted">{links[0]!.source.path}</div>
          </button>
          <ul className="mt-1 space-y-0.5">
            {links.map((l, i) => (
              <li key={`${i}:${l.line}:${l.targetText}`} className="text-xs text-fg-muted">
                <span className="text-fg">[[{l.targetText}]]</span> on line {l.line}
              </li>
            ))}
          </ul>
        </li>
      ))}
    </ul>
  );
}

export function NoteExplorer() {
  const notes = useNotes((s) => s.notes);
  const status = useNotes((s) => s.status);
  const error = useNotes((s) => s.error);
  const isSample = useNotes((s) => s.api.isSample);
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

  const searching = query.trim() !== "";
  const search = useSearchResults(query);
  const visible = selectNotes(notes, view, sort, "");

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

      {view.kind === "trash" ? (
        <TrashList />
      ) : view.kind === "orphans" ? (
        <OrphanList />
      ) : view.kind === "broken" ? (
        <BrokenList />
      ) : (
        <>
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

          {isSample && (
            <p className="mx-3 mb-2 rounded-md bg-surface px-2 py-1 text-xs text-fg-muted">
              Showing sample data. Nothing is saved to disk.
            </p>
          )}

          {error && (
            <p role="alert" className="mx-3 mb-2 rounded-md border border-border px-2 py-1 text-xs">
              {error}
            </p>
          )}

          {searching ? (
            <ul aria-label="Search results" className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
              {search.error && (
                <li role="alert" className="px-2 py-2 text-sm">
                  {search.error}
                </li>
              )}
              {search.hits === null && !search.error && (
                <li className="px-2 py-2 text-sm text-fg-muted">Searching…</li>
              )}
              {search.hits?.length === 0 && !search.pending && (
                <li className="px-2 py-2 text-sm text-fg-muted">No notes match your search.</li>
              )}
              {search.hits?.map((hit) => (
                <li key={hit.id}>
                  <button
                    type="button"
                    aria-current={hit.id === selectedNoteId ? "true" : undefined}
                    onClick={() => void openNote(hit.id)}
                    className={cn(
                      "w-full rounded-md px-2 py-2 text-left",
                      hit.id === selectedNoteId ? "bg-surface-active" : "hover:bg-surface-hover",
                    )}
                  >
                    <div className="truncate text-sm font-medium">
                      <Highlighted parts={hit.titleParts} />
                      {hit.archived && <span className="ml-2 text-xs text-fg-muted">archived</span>}
                    </div>
                    {hit.snippet.length > 0 && (
                      <div className="line-clamp-2 text-xs text-fg-muted">
                        <Highlighted parts={hit.snippet} />
                      </div>
                    )}
                    <div className="truncate text-xs text-fg-muted">{hit.path}</div>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
              {status === "loading" && (
                <li className="px-2 py-2 text-sm text-fg-muted">Loading…</li>
              )}
              {status === "ready" && visible.length === 0 && (
                <li className="px-2 py-2 text-sm text-fg-muted">No notes here yet.</li>
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
                      <div className="flex items-center gap-1 truncate text-sm font-medium">
                        {note.pinned && (
                          <Star aria-label="Pinned" className="size-3 shrink-0 fill-current" />
                        )}
                        <span className="truncate">{note.title}</span>
                      </div>
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
          )}
        </>
      )}
    </section>
  );
}
