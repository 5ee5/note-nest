import type { NoteSummary } from "./api/types";

export type View =
  | { kind: "all" }
  | { kind: "favorites" }
  | { kind: "recent" }
  | { kind: "daily" }
  | { kind: "archived" }
  | { kind: "trash" }
  | { kind: "orphans" }
  | { kind: "broken" }
  | { kind: "tag"; value: string }
  | { kind: "folder"; value: string };

export type SortKey = "updated" | "created" | "title";

const DAILY = /^\d{4}-\d{2}-\d{2}$/;

export function isDailyNote(note: NoteSummary): boolean {
  return note.folder === "Journal" && DAILY.test(note.title);
}

export function viewLabel(view: View): string {
  switch (view.kind) {
    case "all":
      return "All notes";
    case "favorites":
      return "Favorites";
    case "recent":
      return "Recently opened";
    case "daily":
      return "Daily notes";
    case "archived":
      return "Archive";
    case "trash":
      return "Trash";
    case "orphans":
      return "Orphaned notes";
    case "broken":
      return "Broken links";
    case "tag":
      return `#${view.value}`;
    case "folder":
      return view.value;
  }
}

function inView(note: NoteSummary, view: View): boolean {
  // Archived notes only appear in the Archive view.
  if (view.kind === "archived") return note.archived;
  if (note.archived) return false;
  switch (view.kind) {
    case "all":
      return true;
    case "trash":
    case "orphans":
    case "broken":
      return false; // these views list something other than notes

    case "favorites":
      return note.pinned;
    case "recent":
      return note.lastOpenedAt !== null;
    case "daily":
      return isDailyNote(note);
    case "tag":
      return note.tags.some((t) => t === view.value || t.startsWith(`${view.value}/`));
    case "folder":
      return note.folder === view.value || note.folder.startsWith(`${view.value}/`);
  }
}

function matchesQuery(note: NoteSummary, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (q === "") return true;
  return (
    note.title.toLowerCase().includes(q) ||
    note.preview.toLowerCase().includes(q) ||
    note.tags.some((t) => t.toLowerCase().includes(q))
  );
}

function compare(a: NoteSummary, b: NoteSummary, sort: SortKey, view: View): number {
  if (view.kind === "recent") {
    return (b.lastOpenedAt ?? "").localeCompare(a.lastOpenedAt ?? "");
  }
  switch (sort) {
    case "title":
      return a.title.localeCompare(b.title);
    case "created":
      return b.createdAt.localeCompare(a.createdAt);
    case "updated":
      return b.updatedAt.localeCompare(a.updatedAt);
  }
}

/** Pure filter + sort used by the note explorer. Never mutates its input. */
export function selectNotes(
  notes: readonly NoteSummary[],
  view: View,
  sort: SortKey,
  query: string,
): NoteSummary[] {
  const list = notes
    .filter((n) => inView(n, view) && matchesQuery(n, query))
    .sort((a, b) => compare(a, b, sort, view));
  return view.kind === "recent" ? list.slice(0, RECENT_LIMIT) : list;
}

/** "Recently opened" shows only the latest notes. */
export const RECENT_LIMIT = 20;

export function allTags(notes: readonly NoteSummary[]): string[] {
  return [...new Set(notes.flatMap((n) => n.tags))].sort();
}
