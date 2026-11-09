import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toApiError, type Graph } from "../lib/api";
import { openNote } from "../lib/actions";
import { cn } from "../lib/cn";
import {
  GRAPH_DRAW_LIMIT,
  fitTransform,
  layoutGraph,
  neighbourIds,
  pickNode,
  zoomAt,
  type Layout,
  type Transform,
} from "../lib/graphLayout";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

type Scope = "local" | "vault";
const DEPTHS = [1, 2, 3] as const;

type Loaded = { key: string; graph: Graph } | { key: string; error: string };

function Message({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center p-6 text-center text-sm text-fg-muted">
      <div className="max-w-sm space-y-3">{children}</div>
    </div>
  );
}

function css(el: Element, name: string, fallback: string): string {
  return getComputedStyle(el).getPropertyValue(name).trim() || fallback;
}

/** Paints the layout. Pure with respect to its arguments, so it can be redrawn at any time. */
function draw(
  canvas: HTMLCanvasElement,
  layout: Layout,
  t: Transform,
  size: { w: number; h: number },
  centerId: string | null,
  focusId: string | null,
) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(size.w * dpr);
  canvas.height = Math.round(size.h * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, size.w, size.h);

  const accent = css(canvas, "--accent", "#4f46e5");
  const fg = css(canvas, "--fg", "#1a1a1f");
  const muted = css(canvas, "--fg-muted", "#5f5f6b");
  const border = css(canvas, "--border", "#e2e2e5");
  const bg = css(canvas, "--bg", "#ffffff");

  const near = focusId ? neighbourIds(layout, focusId) : null;
  // Fade everything but the focused note and its neighbours, unless it has none (then the
  // whole graph would just look washed out).
  const dimming = focusId !== null && near !== null && near.size > 0;
  const lit = (id: string) => !dimming || id === focusId || near?.has(id) === true;

  ctx.save();
  ctx.translate(t.x, t.y);
  ctx.scale(t.k, t.k);

  ctx.lineWidth = 1 / t.k;
  for (const e of layout.edges) {
    const touches = focusId !== null && (e.source.id === focusId || e.target.id === focusId);
    ctx.globalAlpha = !dimming || touches ? 1 : 0.2;
    ctx.strokeStyle = touches ? accent : border;
    ctx.beginPath();
    ctx.moveTo(e.source.x, e.source.y);
    ctx.lineTo(e.target.x, e.target.y);
    ctx.stroke();
  }

  for (const n of layout.nodes) {
    ctx.globalAlpha = lit(n.id) ? (n.archived ? 0.55 : 1) : 0.3;
    ctx.fillStyle = n.archived ? muted : accent;
    ctx.beginPath();
    ctx.arc(n.x, n.y, n.r, 0, Math.PI * 2);
    ctx.fill();
    if (n.id === centerId || n.id === focusId) {
      ctx.lineWidth = 2 / t.k;
      ctx.strokeStyle = fg;
      ctx.stroke();
    }
  }

  // Labels: always for the focused note and its neighbours, for everything once zoomed in,
  // and for the busiest notes at a medium zoom.
  ctx.globalAlpha = 1;
  ctx.font = `${12 / t.k}px system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  for (const n of layout.nodes) {
    const emphasised = n.id === centerId || n.id === focusId || (dimming && lit(n.id));
    const show = emphasised || t.k >= 1.4 || (t.k >= 0.7 && n.degree >= 6);
    if (!show || !lit(n.id)) continue;
    const y = n.y + n.r + 2 / t.k;
    ctx.lineWidth = 3 / t.k;
    ctx.strokeStyle = bg;
    ctx.strokeText(n.title, n.x, y);
    ctx.fillStyle = fg;
    ctx.fillText(n.title, n.x, y);
  }
  ctx.restore();
}

export function GraphView() {
  const selectedNoteId = useUi((s) => s.selectedNoteId);
  const setView = useUi((s) => s.setView);
  const notes = useNotes((s) => s.notes);

  const [scope, setScope] = useState<Scope>(selectedNoteId ? "local" : "vault");
  const [depth, setDepth] = useState<number>(2);
  const [drawAnyway, setDrawAnyway] = useState(false);
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [view, setViewState] = useState<{ layout: Layout; t: Transform } | null>(null);

  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const drag = useRef<{ x: number; y: number; moved: boolean } | null>(null);

  const centerId = scope === "local" ? selectedNoteId : null;
  const tooBig = scope === "vault" && notes.length > GRAPH_DRAW_LIMIT && !drawAnyway;
  const wanted =
    scope === "local" && !centerId ? null : tooBig ? null : `${scope}:${centerId}:${depth}`;

  // Fetch whenever what is being shown, or the notes themselves, change.
  useEffect(() => {
    if (wanted === null) return;
    let cancelled = false;
    useNotes
      .getState()
      .api.getGraph(centerId, depth)
      .then((graph) => !cancelled && setLoaded({ key: wanted, graph }))
      .catch((e) => !cancelled && setLoaded({ key: wanted, error: toApiError(e).message }));
    return () => {
      cancelled = true;
    };
  }, [wanted, centerId, depth, notes]);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const measure = () => setSize({ w: el.clientWidth, h: el.clientHeight });
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [wanted, tooBig]);

  const current = loaded && loaded.key === wanted ? loaded : null;
  const graph = current && "graph" in current ? current.graph : null;
  const layout = useMemo(() => (graph ? layoutGraph(graph) : null), [graph]);

  // The user's pan/zoom applies to one layout; a new layout starts fitted to the canvas.
  const transform: Transform | null = layout
    ? view && view.layout === layout
      ? view.t
      : fitTransform(layout.bounds, size.w, size.h)
    : null;

  const focusId = hover ?? picked;

  useEffect(() => {
    if (canvas.current && layout && transform && size.w > 0 && size.h > 0) {
      draw(canvas.current, layout, transform, size, centerId, focusId);
    }
  });

  const local = useCallback((e: { clientX: number; clientY: number }) => {
    const r = canvas.current?.getBoundingClientRect();
    return { x: e.clientX - (r?.left ?? 0), y: e.clientY - (r?.top ?? 0) };
  }, []);

  if (scope === "local" && !centerId && !tooBig) {
    return (
      <Shell scope={scope} setScope={setScope} depth={depth} setDepth={setDepth}>
        <Message>
          <p>Open a note to see the notes around it, or switch to the whole vault.</p>
          <button
            type="button"
            onClick={() => setScope("vault")}
            className="rounded-md border border-border px-3 py-1.5 hover:bg-surface-hover"
          >
            Show the whole vault
          </button>
        </Message>
      </Shell>
    );
  }
  if (tooBig) {
    return (
      <Shell scope={scope} setScope={setScope} depth={depth} setDepth={setDepth}>
        <Message>
          <p>
            Your vault has {notes.length.toLocaleString()} notes, which is too many to draw at once
            without being unreadable and slow.
          </p>
          <p>Show the graph around one note instead, or draw everything anyway.</p>
          <div className="flex justify-center gap-2">
            <button
              type="button"
              onClick={() => setScope("local")}
              className="rounded-md bg-accent px-3 py-1.5 text-accent-fg"
            >
              Show around a note
            </button>
            <button
              type="button"
              onClick={() => setDrawAnyway(true)}
              className="rounded-md border border-border px-3 py-1.5 hover:bg-surface-hover"
            >
              Draw everything
            </button>
          </div>
        </Message>
      </Shell>
    );
  }

  const pickedNode = picked ? layout?.nodes.find((n) => n.id === picked) : undefined;

  return (
    <Shell
      scope={scope}
      setScope={setScope}
      depth={depth}
      setDepth={setDepth}
      summary={
        graph
          ? scope === "local"
            ? `${graph.nodes.length} of ${graph.totalNotes} notes`
            : `${graph.nodes.length} notes, ${layout?.edges.length ?? 0} links`
          : undefined
      }
    >
      <div ref={box} className="relative min-h-0 flex-1 overflow-hidden bg-bg">
        {current && "error" in current ? (
          <Message>
            <p>Could not load the graph: {current.error}</p>
          </Message>
        ) : !layout ? (
          <Message>
            <p>Loading…</p>
          </Message>
        ) : layout.nodes.length === 0 ? (
          <Message>
            <p>No notes to show.</p>
          </Message>
        ) : (
          <>
            <canvas
              ref={canvas}
              role="img"
              aria-label={`Link graph with ${layout.nodes.length} notes and ${layout.edges.length} links. Drag to pan, scroll to zoom, double-click a note to open it.`}
              style={{ width: size.w, height: size.h, touchAction: "none" }}
              className={cn("block", hover ? "cursor-pointer" : "cursor-grab")}
              onPointerDown={(e) => {
                const p = local(e);
                drag.current = { x: p.x, y: p.y, moved: false };
                e.currentTarget.setPointerCapture?.(e.pointerId);
              }}
              onPointerMove={(e) => {
                const p = local(e);
                const d = drag.current;
                if (d && transform) {
                  const dx = p.x - d.x;
                  const dy = p.y - d.y;
                  if (d.moved || Math.hypot(dx, dy) > 4) {
                    d.moved = true;
                    d.x = p.x;
                    d.y = p.y;
                    setViewState({
                      layout,
                      t: { ...transform, x: transform.x + dx, y: transform.y + dy },
                    });
                  }
                  return;
                }
                if (transform) setHover(pickNode(layout, transform, p.x, p.y)?.id ?? null);
              }}
              onPointerUp={(e) => {
                const d = drag.current;
                drag.current = null;
                e.currentTarget.releasePointerCapture?.(e.pointerId);
                if (d && !d.moved && transform) {
                  const p = local(e);
                  setPicked(pickNode(layout, transform, p.x, p.y)?.id ?? null);
                }
              }}
              onPointerLeave={() => setHover(null)}
              onDoubleClick={(e) => {
                const p = local(e);
                const n = transform ? pickNode(layout, transform, p.x, p.y) : null;
                if (n) void open(n.id, setView);
              }}
              onWheel={(e) => {
                if (!transform) return;
                const p = local(e);
                setViewState({
                  layout,
                  t: zoomAt(transform, Math.exp(-e.deltaY * 0.0015), p.x, p.y),
                });
              }}
            />
            <div className="absolute right-3 bottom-3 flex gap-1">
              <ZoomButton
                label="Zoom in"
                onClick={() =>
                  transform &&
                  setViewState({ layout, t: zoomAt(transform, 1.4, size.w / 2, size.h / 2) })
                }
              >
                +
              </ZoomButton>
              <ZoomButton
                label="Zoom out"
                onClick={() =>
                  transform &&
                  setViewState({ layout, t: zoomAt(transform, 1 / 1.4, size.w / 2, size.h / 2) })
                }
              >
                −
              </ZoomButton>
              <ZoomButton label="Fit to window" onClick={() => setViewState(null)}>
                ⤢
              </ZoomButton>
            </div>
            {pickedNode && (
              <div className="absolute bottom-3 left-3 max-w-xs rounded-md border border-border bg-surface p-3 text-sm shadow">
                <div className="truncate font-medium">{pickedNode.title}</div>
                <div className="truncate text-xs text-fg-muted">{pickedNode.path}</div>
                <div className="mt-1 text-xs text-fg-muted">
                  {pickedNode.degree} {pickedNode.degree === 1 ? "link" : "links"}
                  {pickedNode.archived ? " · archived" : ""}
                </div>
                <button
                  type="button"
                  onClick={() => void open(pickedNode.id, setView)}
                  className="mt-2 rounded-md bg-accent px-2.5 py-1 text-xs text-accent-fg"
                >
                  Open note
                </button>
              </div>
            )}
          </>
        )}
      </div>
    </Shell>
  );
}

async function open(id: string, setView: (v: { kind: "all" }) => void) {
  await openNote(id);
  setView({ kind: "all" });
}

function ZoomButton({
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
      className="size-8 rounded-md border border-border bg-surface text-base leading-none hover:bg-surface-hover"
    >
      {children}
    </button>
  );
}

function Shell({
  scope,
  setScope,
  depth,
  setDepth,
  summary,
  children,
}: {
  scope: Scope;
  setScope: (s: Scope) => void;
  depth: number;
  setDepth: (d: number) => void;
  summary?: string | undefined;
  children: React.ReactNode;
}) {
  const seg = (active: boolean) =>
    cn(
      "px-2.5 py-1 text-xs",
      active ? "bg-surface-active font-medium" : "text-fg-muted hover:bg-surface-hover",
    );
  return (
    <section aria-label="Graph" className="flex h-full flex-col bg-bg">
      <header className="flex flex-wrap items-center gap-3 border-b border-border px-3 py-2">
        <h2 className="text-sm font-semibold">Graph</h2>
        <div
          role="group"
          aria-label="Scope"
          className="flex overflow-hidden rounded-md border border-border"
        >
          <button
            type="button"
            aria-pressed={scope === "local"}
            onClick={() => setScope("local")}
            className={seg(scope === "local")}
          >
            Around this note
          </button>
          <button
            type="button"
            aria-pressed={scope === "vault"}
            onClick={() => setScope("vault")}
            className={seg(scope === "vault")}
          >
            Whole vault
          </button>
        </div>
        {scope === "local" && (
          <div
            role="group"
            aria-label="Depth"
            className="flex items-center gap-1 text-xs text-fg-muted"
          >
            <span>Depth</span>
            <div className="flex overflow-hidden rounded-md border border-border">
              {DEPTHS.map((d) => (
                <button
                  key={d}
                  type="button"
                  aria-pressed={depth === d}
                  onClick={() => setDepth(d)}
                  className={seg(depth === d)}
                >
                  {d}
                </button>
              ))}
            </div>
          </div>
        )}
        {summary && <span className="ml-auto text-xs text-fg-muted">{summary}</span>}
      </header>
      {children}
    </section>
  );
}
