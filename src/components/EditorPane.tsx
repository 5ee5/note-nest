import { lazy, Suspense, useState } from "react";
import {
  Archive,
  ArchiveRestore,
  Command as CommandIcon,
  Copy,
  PanelLeft,
  Search,
  Star,
  Trash2,
} from "lucide-react";
import { deleteNote, duplicateNote, renameNote, setArchived, setPinned } from "../lib/actions";
import { modLabel } from "../lib/shortcuts";
import { cn } from "../lib/cn";
import { useEditor, type SaveStatus } from "../store/editor";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

// The editor (CodeMirror and its language packs) is loaded on first use.
const MarkdownEditor = lazy(() => import("./MarkdownEditor"));
const FormatToolbar = lazy(() => import("./FormatToolbar"));

const STATUS_LABEL: Record<SaveStatus, string> = {
  idle: "",
  dirty: "Unsaved changes",
  saving: "Saving…",
  saved: "Saved",
  error: "Save failed",
  conflict: "Changed on disk",
};

function ToolbarButton({
  label,
  onClick,
  pressed,
  children,
}: {
  label: string;
  onClick: () => void;
  pressed?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={pressed}
      onClick={onClick}
      className={cn("rounded-md p-1.5 text-fg-muted hover:bg-surface-hover", pressed && "text-fg")}
    >
      {children}
    </button>
  );
}

/** Editable note title; committing renames the file. */
function TitleInput({ id, title }: { id: string; title: string }) {
  const [value, setValue] = useState(title);
  const commit = async () => {
    const next = value.trim();
    if (next === title) return;
    if (next === "" || !(await renameNote(id, next))) setValue(title);
  };
  return (
    <input
      aria-label="Note title"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onBlur={() => void commit()}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          setValue(title);
          e.currentTarget.blur();
        }
      }}
      className="min-w-0 flex-1 rounded-md bg-transparent px-1 py-0.5 text-xl font-semibold hover:bg-surface-hover focus:bg-surface"
    />
  );
}

export function EditorPane() {
  const notes = useNotes((s) => s.notes);
  const selectedNoteId = useUi((s) => s.selectedNoteId);
  const sidebarCollapsed = useUi((s) => s.sidebarCollapsed);
  const explorerCollapsed = useUi((s) => s.explorerCollapsed);
  const setSidebarCollapsed = useUi((s) => s.setSidebarCollapsed);
  const setExplorerCollapsed = useUi((s) => s.setExplorerCollapsed);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  const { noteId, content, status, message, loading, loadError, revision } = useEditor();
  const edit = useEditor((s) => s.edit);
  const keepMine = useEditor((s) => s.keepMine);
  const loadTheirs = useEditor((s) => s.loadTheirs);
  const flush = useEditor((s) => s.flush);
  const note = notes.find((n) => n.id === (noteId ?? selectedNoteId)) ?? null;
  const mod = modLabel();

  return (
    <main aria-label="Editor" className="flex h-full flex-col bg-bg">
      <div className="flex items-center gap-1 border-b border-border px-2 py-1.5">
        <ToolbarButton
          label="Toggle sidebar"
          pressed={!sidebarCollapsed}
          onClick={() => setSidebarCollapsed(!sidebarCollapsed)}
        >
          <PanelLeft aria-hidden className="size-4" />
        </ToolbarButton>
        <ToolbarButton
          label="Toggle note explorer"
          pressed={!explorerCollapsed}
          onClick={() => setExplorerCollapsed(!explorerCollapsed)}
        >
          <Search aria-hidden className="size-4" />
        </ToolbarButton>
        <div className="ml-auto">
          <ToolbarButton label={`Command palette (${mod}+K)`} onClick={() => setPaletteOpen(true)}>
            <CommandIcon aria-hidden className="size-4" />
          </ToolbarButton>
        </div>
      </div>

      {loadError ? (
        <div className="p-8">
          <p role="alert" className="rounded-md border border-border px-3 py-2 text-sm">
            This note cannot be opened: {loadError}
          </p>
        </div>
      ) : note && noteId ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="flex items-center gap-1 px-6 pt-5">
            <TitleInput key={note.id + note.title} id={note.id} title={note.title} />
            <ToolbarButton
              label={note.pinned ? "Unpin note" : "Pin note"}
              pressed={note.pinned}
              onClick={() => void setPinned(note.id, !note.pinned)}
            >
              <Star aria-hidden className={cn("size-4", note.pinned && "fill-current")} />
            </ToolbarButton>
            <ToolbarButton
              label={note.archived ? "Unarchive note" : "Archive note"}
              onClick={() => void setArchived(note.id, !note.archived)}
            >
              {note.archived ? (
                <ArchiveRestore aria-hidden className="size-4" />
              ) : (
                <Archive aria-hidden className="size-4" />
              )}
            </ToolbarButton>
            <ToolbarButton label="Duplicate note" onClick={() => void duplicateNote(note.id)}>
              <Copy aria-hidden className="size-4" />
            </ToolbarButton>
            <ToolbarButton label="Delete note" onClick={() => void deleteNote(note.id)}>
              <Trash2 aria-hidden className="size-4" />
            </ToolbarButton>
          </div>
          <div className="flex items-center justify-between px-7 pt-1 pb-2 text-xs text-fg-muted">
            <span>{note.path}</span>
            <span role="status" aria-live="polite">
              {STATUS_LABEL[status]}
            </span>
          </div>

          {status === "conflict" && (
            <div role="alert" className="mx-6 mb-2 rounded-md border border-ring px-3 py-2 text-sm">
              <p>
                This note was changed outside NoteNest. Autosave is paused so nothing is
                overwritten.
              </p>
              <div className="mt-2 flex gap-2">
                <button
                  type="button"
                  onClick={() => void keepMine()}
                  className="rounded-md bg-accent px-2.5 py-1 text-xs text-accent-fg"
                >
                  Keep my version
                </button>
                <button
                  type="button"
                  onClick={() => void loadTheirs()}
                  className="rounded-md border border-border px-2.5 py-1 text-xs hover:bg-surface-hover"
                >
                  Load the version on disk
                </button>
              </div>
              <p className="mt-1 text-xs text-fg-muted">
                Either way, the other version is kept so you can get it back.
              </p>
            </div>
          )}
          {status === "error" && (
            <div role="alert" className="mx-6 mb-2 rounded-md border border-ring px-3 py-2 text-sm">
              <p>Could not save: {message}</p>
              <p className="mt-1 text-xs text-fg-muted">
                Your text is still here and a recovery copy was kept.
              </p>
              <button
                type="button"
                onClick={() => void flush()}
                className="mt-2 rounded-md border border-border px-2.5 py-1 text-xs hover:bg-surface-hover"
              >
                Try again
              </button>
            </div>
          )}

          <Suspense fallback={<div className="h-9 border-b border-border" />}>
            <FormatToolbar />
          </Suspense>
          <div className="min-h-0 flex-1">
            <Suspense fallback={<p className="px-7 text-sm text-fg-muted">Loading editor…</p>}>
              <MarkdownEditor
                key={`${noteId}:${revision}`}
                value={content}
                onChange={edit}
                onSave={() => void flush()}
              />
            </Suspense>
          </div>
        </div>
      ) : (
        <div className="p-8">
          <p className="text-sm text-fg-muted">
            {loading ? "Opening…" : `Select a note, or press ${mod}+N to create one.`}
          </p>
        </div>
      )}
    </main>
  );
}
