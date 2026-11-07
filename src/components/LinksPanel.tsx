import { ChevronRight } from "lucide-react";
import { offerCreateNote, openNote } from "../lib/actions";
import { cn } from "../lib/cn";
import { useLinks, type LinksTab } from "../store/links";

const TABS: { id: LinksTab; label: string }[] = [
  { id: "backlinks", label: "Backlinks" },
  { id: "links", label: "Links" },
  { id: "related", label: "Related" },
];

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="px-2 py-2 text-xs text-fg-muted">{children}</p>;
}

/** A collapsible panel with what connects the open note to others. */
export function LinksPanel() {
  const data = useLinks((s) => s.data);
  const error = useLinks((s) => s.error);
  const open = useLinks((s) => s.open);
  const tab = useLinks((s) => s.tab);
  const setOpen = useLinks((s) => s.setOpen);
  const setTab = useLinks((s) => s.setTab);

  const counts = {
    backlinks: data?.backlinks.length ?? 0,
    links: data?.outgoing.length ?? 0,
    related: data?.related.length ?? 0,
  };
  const broken = data?.outgoing.filter((o) => !o.target).length ?? 0;

  return (
    <section aria-label="Note connections" className="border-t border-border">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-1.5 px-7 py-1.5 text-left text-xs text-fg-muted hover:bg-surface-hover"
      >
        <ChevronRight
          aria-hidden
          className={cn("size-3.5 transition-transform", open && "rotate-90")}
        />
        <span>
          {counts.backlinks} backlink{counts.backlinks === 1 ? "" : "s"} · {counts.links} link
          {counts.links === 1 ? "" : "s"}
          {broken > 0 && <span className="ml-1 text-fg">({broken} broken)</span>}
        </span>
      </button>

      {open && (
        <div className="max-h-56 overflow-y-auto px-5 pb-2">
          <div role="tablist" aria-label="Connections" className="mb-1 flex gap-1 px-2">
            {TABS.map((t) => (
              <button
                key={t.id}
                type="button"
                role="tab"
                aria-selected={tab === t.id}
                onClick={() => setTab(t.id)}
                className={cn(
                  "rounded-md px-2 py-0.5 text-xs",
                  tab === t.id
                    ? "bg-surface-active font-medium"
                    : "text-fg-muted hover:bg-surface-hover",
                )}
              >
                {t.label} ({counts[t.id]})
              </button>
            ))}
          </div>

          {error && <Empty>{error}</Empty>}

          {tab === "backlinks" && (
            <ul aria-label="Backlinks">
              {counts.backlinks === 0 && !error && <Empty>No other note links here yet.</Empty>}
              {data?.backlinks.map((b, i) => (
                <li key={`${i}:${b.source.id}:${b.line}:${b.targetText}`}>
                  <button
                    type="button"
                    onClick={() => void openNote(b.source.id)}
                    className="w-full rounded-md px-2 py-1.5 text-left hover:bg-surface-hover"
                  >
                    <div className="truncate text-sm font-medium">{b.source.title}</div>
                    <div className="truncate text-xs text-fg-muted">{b.context}</div>
                  </button>
                </li>
              ))}
            </ul>
          )}

          {tab === "links" && (
            <ul aria-label="Links in this note">
              {counts.links === 0 && !error && (
                <Empty>This note does not link to other notes.</Empty>
              )}
              {data?.outgoing.map((o, i) => (
                <li
                  key={`${i}:${o.line}:${o.targetText}`}
                  className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-surface-hover"
                >
                  {o.target ? (
                    <button
                      type="button"
                      onClick={() => void openNote(o.target!.id)}
                      className="min-w-0 flex-1 truncate text-left"
                    >
                      {o.target.title}
                      <span className="ml-2 text-xs text-fg-muted">line {o.line}</span>
                    </button>
                  ) : (
                    <>
                      <span className="min-w-0 flex-1 truncate">
                        [[{o.targetText}]]
                        <span className="ml-2 text-xs text-fg-muted">
                          not found · line {o.line}
                        </span>
                      </span>
                      <button
                        type="button"
                        onClick={() => offerCreateNote(o.targetText)}
                        className="shrink-0 rounded-md border border-border px-2 py-0.5 text-xs hover:bg-surface-hover"
                      >
                        Create
                      </button>
                    </>
                  )}
                </li>
              ))}
            </ul>
          )}

          {tab === "related" && (
            <ul aria-label="Related notes">
              {counts.related === 0 && !error && (
                <Empty>Nothing related yet. Shared tags and links connect notes.</Empty>
              )}
              {data?.related.map((r) => (
                <li key={r.note.id}>
                  <button
                    type="button"
                    onClick={() => void openNote(r.note.id)}
                    className="flex w-full items-baseline justify-between gap-3 rounded-md px-2 py-1.5 text-left hover:bg-surface-hover"
                  >
                    <span className="truncate text-sm">{r.note.title}</span>
                    <span className="shrink-0 text-xs text-fg-muted">
                      {r.linked && "linked"}
                      {r.linked && r.sharedTags > 0 && " · "}
                      {r.sharedTags > 0 &&
                        `${r.sharedTags} shared tag${r.sharedTags === 1 ? "" : "s"}`}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}
