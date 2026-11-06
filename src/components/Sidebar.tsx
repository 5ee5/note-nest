import {
  Calendar,
  Clock,
  Folder,
  Hash,
  Library,
  Settings,
  Star,
  type LucideIcon,
} from "lucide-react";
import { allTags, type View } from "../lib/notes";
import { cn } from "../lib/cn";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

function sameView(a: View, b: View): boolean {
  if (a.kind !== b.kind) return false;
  return "value" in a && "value" in b ? a.value === b.value : true;
}

function NavButton({ icon: Icon, label, view }: { icon: LucideIcon; label: string; view: View }) {
  const current = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const active = sameView(current, view);
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={() => setView(view)}
      className={cn(
        "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm",
        active ? "bg-surface-active font-medium" : "text-fg-muted hover:bg-surface-hover",
      )}
    >
      <Icon aria-hidden className="size-4 shrink-0" />
      <span className="truncate">{label}</span>
    </button>
  );
}

function SectionTitle({ children }: { children: string }) {
  return (
    <h2 className="mt-4 mb-1 px-2 text-xs font-semibold tracking-wide text-fg-muted uppercase">
      {children}
    </h2>
  );
}

export function Sidebar() {
  const notes = useNotes((s) => s.notes);
  const folders = useNotes((s) => s.folders);
  const setSettingsOpen = useUi((s) => s.setSettingsOpen);
  const tags = allTags(notes);

  return (
    <nav aria-label="Sidebar" className="flex h-full flex-col bg-surface">
      <div className="px-4 pt-4 pb-2 text-sm font-semibold">NoteNest</div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        <NavButton icon={Library} label="All notes" view={{ kind: "all" }} />
        <NavButton icon={Star} label="Favorites" view={{ kind: "favorites" }} />
        <NavButton icon={Clock} label="Recently opened" view={{ kind: "recent" }} />
        <NavButton icon={Calendar} label="Daily notes" view={{ kind: "daily" }} />

        <SectionTitle>Tags</SectionTitle>
        {tags.length === 0 && <p className="px-2 text-xs text-fg-muted">No tags yet.</p>}
        {tags.map((tag) => (
          <NavButton key={tag} icon={Hash} label={tag} view={{ kind: "tag", value: tag }} />
        ))}

        <SectionTitle>Folders</SectionTitle>
        {folders.length === 0 && <p className="px-2 text-xs text-fg-muted">No folders yet.</p>}
        {folders.map((f) => (
          <NavButton key={f} icon={Folder} label={f} view={{ kind: "folder", value: f }} />
        ))}
      </div>
      <div className="border-t border-border p-2">
        <button
          type="button"
          onClick={() => setSettingsOpen(true)}
          className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-fg-muted hover:bg-surface-hover"
        >
          <Settings aria-hidden className="size-4" />
          Settings
        </button>
      </div>
    </nav>
  );
}
