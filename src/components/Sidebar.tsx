import {
  Archive,
  Calendar,
  Clock,
  Folder,
  FolderPlus,
  Hash,
  Library,
  Pencil,
  Settings,
  Star,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { allTags, type View } from "../lib/notes";
import { createFolder, deleteFolder, renameFolder } from "../lib/actions";
import { cn } from "../lib/cn";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

function sameView(a: View, b: View): boolean {
  if (a.kind !== b.kind) return false;
  return "value" in a && "value" in b ? a.value === b.value : true;
}

function NavButton({
  icon: Icon,
  label,
  view,
  depth = 0,
}: {
  icon: LucideIcon;
  label: string;
  view: View;
  depth?: number;
}) {
  const current = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const active = sameView(current, view);
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={() => setView(view)}
      style={{ paddingLeft: `${0.5 + depth * 0.9}rem` }}
      className={cn(
        "flex min-w-0 flex-1 items-center gap-2 rounded-md py-1.5 pr-2 text-left text-sm",
        active ? "bg-surface-active font-medium" : "text-fg-muted hover:bg-surface-hover",
      )}
    >
      <Icon aria-hidden className="size-4 shrink-0" />
      <span className="truncate">{label}</span>
    </button>
  );
}

function SectionHeader({ title, children }: { title: string; children?: React.ReactNode }) {
  return (
    <div className="mt-4 mb-1 flex items-center justify-between px-2">
      <h2 className="text-xs font-semibold tracking-wide text-fg-muted uppercase">{title}</h2>
      {children}
    </div>
  );
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="rounded p-1 text-fg-muted hover:bg-surface-hover"
    >
      {children}
    </button>
  );
}

function FolderRow({ path }: { path: string }) {
  const openPrompt = useUi((s) => s.openPrompt);
  const depth = path.split("/").length - 1;
  const name = path.slice(path.lastIndexOf("/") + 1);
  return (
    <div className="group flex items-center">
      <NavButton icon={Folder} label={name} view={{ kind: "folder", value: path }} depth={depth} />
      <span className="flex opacity-0 group-focus-within:opacity-100 group-hover:opacity-100">
        <IconButton
          label={`New subfolder in ${name}`}
          onClick={() =>
            openPrompt({
              title: `New folder in ${path}`,
              label: "Folder name",
              confirmLabel: "Create",
              onSubmit: async (n) => void (await createFolder(`${path}/${n}`)),
            })
          }
        >
          <FolderPlus aria-hidden className="size-3.5" />
        </IconButton>
        <IconButton
          label={`Rename ${name}`}
          onClick={() =>
            openPrompt({
              title: "Rename folder",
              label: "Folder name",
              initial: name,
              confirmLabel: "Rename",
              onSubmit: async (n) => void (await renameFolder(path, n)),
            })
          }
        >
          <Pencil aria-hidden className="size-3.5" />
        </IconButton>
        <IconButton label={`Delete ${name}`} onClick={() => void deleteFolder(path)}>
          <Trash2 aria-hidden className="size-3.5" />
        </IconButton>
      </span>
    </div>
  );
}

export function Sidebar() {
  const notes = useNotes((s) => s.notes);
  const folders = useNotes((s) => s.folders);
  const trashCount = useNotes((s) => s.trash.length);
  const setSettingsOpen = useUi((s) => s.setSettingsOpen);
  const openPrompt = useUi((s) => s.openPrompt);
  const tags = allTags(notes);
  const row = (child: React.ReactNode) => <div className="flex items-center">{child}</div>;

  return (
    <nav aria-label="Sidebar" className="flex h-full flex-col bg-surface">
      <div className="px-4 pt-4 pb-2 text-sm font-semibold">NoteNest</div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {row(<NavButton icon={Library} label="All notes" view={{ kind: "all" }} />)}
        {row(<NavButton icon={Star} label="Favorites" view={{ kind: "favorites" }} />)}
        {row(<NavButton icon={Clock} label="Recently opened" view={{ kind: "recent" }} />)}
        {row(<NavButton icon={Calendar} label="Daily notes" view={{ kind: "daily" }} />)}

        {tags.length > 0 && (
          <>
            <SectionHeader title="Tags" />
            {tags.map((tag) => (
              <div key={tag} className="flex items-center">
                <NavButton icon={Hash} label={tag} view={{ kind: "tag", value: tag }} />
              </div>
            ))}
          </>
        )}

        <SectionHeader title="Folders">
          <IconButton
            label="New folder"
            onClick={() =>
              openPrompt({
                title: "New folder",
                label: "Folder name",
                confirmLabel: "Create",
                onSubmit: async (n) => void (await createFolder(n)),
              })
            }
          >
            <FolderPlus aria-hidden className="size-3.5" />
          </IconButton>
        </SectionHeader>
        {folders.length === 0 && <p className="px-2 text-xs text-fg-muted">No folders yet.</p>}
        {folders.map((f) => (
          <FolderRow key={f} path={f} />
        ))}

        <div className="mt-4 border-t border-border pt-2">
          {row(<NavButton icon={Archive} label="Archive" view={{ kind: "archived" }} />)}
          {row(
            <NavButton
              icon={Trash2}
              label={trashCount > 0 ? `Trash (${trashCount})` : "Trash"}
              view={{ kind: "trash" }}
            />,
          )}
        </div>
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
