import { PanelLeft, Command as CommandIcon, Search } from "lucide-react";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";
import { modLabel } from "../lib/shortcuts";

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
      className="rounded-md p-1.5 text-fg-muted hover:bg-surface-hover"
    >
      {children}
    </button>
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
  const note = notes.find((n) => n.id === selectedNoteId) ?? null;
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

      <div className="flex-1 overflow-y-auto p-8">
        {note ? (
          <article>
            <h1 className="text-2xl font-semibold">{note.title}</h1>
            <p className="mt-1 text-xs text-fg-muted">{note.path}</p>
            <p className="mt-8 text-sm text-fg-muted">
              The Markdown editor arrives in a later milestone.
            </p>
          </article>
        ) : (
          <p className="text-sm text-fg-muted">Select a note, or press {mod}+N to create one.</p>
        )}
      </div>
    </main>
  );
}
