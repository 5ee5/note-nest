import { useState } from "react";
import { Command } from "cmdk";
import {
  changeEditorMode,
  changeTheme,
  createFolder,
  createNote,
  openDailyNote,
  openNote,
  searchFor,
} from "../lib/actions";
import { THEMES, type Theme } from "../lib/theme";
import { isDailyNote, type View } from "../lib/notes";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

const THEME_LABELS: Record<Theme, string> = {
  light: "Light",
  dark: "Dark",
  system: "System default",
};

const itemClass =
  "flex cursor-pointer items-center rounded-md px-2 py-1.5 text-sm text-fg-muted data-[selected=true]:bg-surface-active data-[selected=true]:text-fg";
const groupClass =
  "[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:text-fg-muted";

export function CommandPalette() {
  const open = useUi((s) => s.paletteOpen);
  const setOpen = useUi((s) => s.setPaletteOpen);
  const openPrompt = useUi((s) => s.openPrompt);
  const setView = useUi((s) => s.setView);
  const setSettingsOpen = useUi((s) => s.setSettingsOpen);
  const sidebarCollapsed = useUi((s) => s.sidebarCollapsed);
  const explorerCollapsed = useUi((s) => s.explorerCollapsed);
  const setSidebarCollapsed = useUi((s) => s.setSidebarCollapsed);
  const setExplorerCollapsed = useUi((s) => s.setExplorerCollapsed);
  const notes = useNotes((s) => s.notes);
  const folders = useNotes((s) => s.folders);
  const tagCounts = useNotes((s) => s.tagCounts);
  const [input, setInput] = useState("");

  /** Runs a command and closes the palette. */
  const run = (fn: () => void | Promise<void>) => () => {
    setOpen(false);
    void fn();
  };
  const go = (view: View) => run(() => setView(view));

  return (
    <Command.Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) setInput("");
        setOpen(o);
      }}
      label="Command palette"
      overlayClassName="fixed inset-0 z-40 bg-overlay"
      contentClassName="fixed top-[20%] left-1/2 z-50 w-[36rem] max-w-[calc(100vw-2rem)] -translate-x-1/2 overflow-hidden rounded-lg border border-border bg-bg shadow-xl"
    >
      <Command.Input
        value={input}
        onValueChange={setInput}
        placeholder="Type a command or search notes…"
        className="w-full border-b border-border bg-transparent px-4 py-3 text-sm outline-none placeholder:text-fg-muted"
      />
      <Command.List className="max-h-80 overflow-y-auto p-2">
        <Command.Empty className="px-2 py-4 text-center text-sm text-fg-muted">
          No results.
        </Command.Empty>

        {input.trim().length >= 2 && (
          <Command.Group heading="Search" className={groupClass}>
            <Command.Item
              forceMount
              value={`search-notes ${input}`}
              className={itemClass}
              onSelect={run(() => searchFor(input.trim()))}
            >
              Search notes for “{input.trim()}”
            </Command.Item>
          </Command.Group>
        )}

        <Command.Group heading="Actions" className={groupClass}>
          <Command.Item className={itemClass} onSelect={run(createNote)}>
            Create note
          </Command.Item>
          <Command.Item
            className={itemClass}
            onSelect={run(() =>
              openPrompt({
                title: "New folder",
                label: "Folder name",
                confirmLabel: "Create",
                onSubmit: async (n) => void (await createFolder(n)),
              }),
            )}
          >
            Create folder
          </Command.Item>
          <Command.Item className={itemClass} onSelect={run(openDailyNote)}>
            Create daily note
          </Command.Item>
          <Command.Item className={itemClass} onSelect={run(() => setSettingsOpen(true))}>
            Open settings
          </Command.Item>
          <Command.Item
            className={itemClass}
            onSelect={run(() => setSidebarCollapsed(!sidebarCollapsed))}
          >
            Toggle sidebar
          </Command.Item>
          <Command.Item
            className={itemClass}
            onSelect={run(() => setExplorerCollapsed(!explorerCollapsed))}
          >
            Toggle note explorer
          </Command.Item>
        </Command.Group>

        <Command.Group heading="View" className={groupClass}>
          <Command.Item className={itemClass} onSelect={run(() => changeEditorMode("edit"))}>
            View: Edit
          </Command.Item>
          <Command.Item className={itemClass} onSelect={run(() => changeEditorMode("split"))}>
            View: Edit and preview
          </Command.Item>
          <Command.Item className={itemClass} onSelect={run(() => changeEditorMode("preview"))}>
            View: Preview
          </Command.Item>
        </Command.Group>

        <Command.Group heading="Theme" className={groupClass}>
          {THEMES.map((t) => (
            <Command.Item key={t} className={itemClass} onSelect={run(() => changeTheme(t))}>
              Switch theme: {THEME_LABELS[t]}
            </Command.Item>
          ))}
        </Command.Group>

        <Command.Group heading="Navigate" className={groupClass}>
          <Command.Item className={itemClass} onSelect={go({ kind: "all" })}>
            Go to All notes
          </Command.Item>
          <Command.Item className={itemClass} onSelect={go({ kind: "favorites" })}>
            Go to Favorites
          </Command.Item>
          <Command.Item className={itemClass} onSelect={go({ kind: "recent" })}>
            Go to Recently opened
          </Command.Item>
          <Command.Item className={itemClass} onSelect={go({ kind: "daily" })}>
            Go to Daily notes
          </Command.Item>
          <Command.Item className={itemClass} onSelect={go({ kind: "graph" })}>
            Go to Graph
          </Command.Item>
          <Command.Item className={itemClass} onSelect={go({ kind: "orphans" })}>
            Go to Orphaned notes
          </Command.Item>
          <Command.Item className={itemClass} onSelect={go({ kind: "broken" })}>
            Go to Broken links
          </Command.Item>
          {tagCounts.map((t) => (
            <Command.Item
              key={t.name}
              value={`tag ${t.name}`}
              className={itemClass}
              onSelect={go({ kind: "tag", value: t.name })}
            >
              Go to tag: #{t.name}
            </Command.Item>
          ))}
          <Command.Item className={itemClass} onSelect={go({ kind: "archived" })}>
            Go to Archive
          </Command.Item>
          <Command.Item className={itemClass} onSelect={go({ kind: "trash" })}>
            Go to Trash
          </Command.Item>
          {folders.map((f) => (
            <Command.Item
              key={f}
              value={`folder ${f}`}
              className={itemClass}
              onSelect={go({ kind: "folder", value: f })}
            >
              Go to folder: {f}
            </Command.Item>
          ))}
        </Command.Group>

        <Command.Group heading="Notes" className={groupClass}>
          {notes.map((n) => (
            <Command.Item
              key={n.id}
              value={`note ${n.title} ${n.path} ${n.id}`}
              className={itemClass}
              onSelect={run(() => openNote(n.id))}
            >
              {n.title}
              <span className="ml-2 truncate text-xs text-fg-muted">
                {isDailyNote(n) ? "Daily note" : n.folder}
              </span>
            </Command.Item>
          ))}
        </Command.Group>
      </Command.List>
    </Command.Dialog>
  );
}
