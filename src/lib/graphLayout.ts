import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";
import type { Graph, GraphNode } from "./api/types";

/** Whole-vault graphs with more notes than this are not drawn unless the user insists. */
export const GRAPH_DRAW_LIMIT = 1500;

export interface LaidOutNode extends GraphNode {
  x: number;
  y: number;
  /** Drawing radius, from the number of links. */
  r: number;
}

export interface LaidOutEdge {
  source: LaidOutNode;
  target: LaidOutNode;
  /** Both notes link to each other. */
  mutual: boolean;
}

export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export interface Layout {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  bounds: Bounds;
}

export function nodeRadius(degree: number): number {
  return 4 + Math.min(10, Math.sqrt(degree) * 2);
}

/** Small seeded generator so the same graph always lays out the same way. */
function lcg(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

type SimNode = SimulationNodeDatum & GraphNode;

/**
 * Positions the notes with a force simulation run to completion up front (no animation),
 * so the result is deterministic, testable without a browser and cheap to redraw.
 * Edges between the same two notes are merged; links to notes outside the graph are dropped.
 */
export function layoutGraph(graph: Graph): Layout {
  const sim: SimNode[] = graph.nodes.map((n) => ({ ...n }));
  const byId = new Map(sim.map((n) => [n.id, n]));

  const pairs = new Map<string, { a: string; b: string; mutual: boolean }>();
  for (const [from, to] of graph.edges) {
    if (from === to || !byId.has(from) || !byId.has(to)) continue;
    const [a, b] = from < to ? [from, to] : [to, from];
    const key = `${a}|${b}`;
    const known = pairs.get(key);
    if (known) known.mutual = true;
    else pairs.set(key, { a, b, mutual: false });
  }
  const links: SimulationLinkDatum<SimNode>[] = [...pairs.values()].map((p) => ({
    source: p.a,
    target: p.b,
  }));

  const n = sim.length;
  if (n > 0) {
    const simulation = forceSimulation(sim)
      .randomSource(lcg(1))
      .force(
        "link",
        forceLink<SimNode, SimulationLinkDatum<SimNode>>(links)
          .id((d) => d.id)
          .distance(40)
          .strength(0.6),
      )
      .force("charge", forceManyBody().strength(-60).distanceMax(400))
      .force(
        "collide",
        forceCollide<SimNode>((d) => nodeRadius(d.degree) + 2),
      )
      .force("center", forceCenter(0, 0))
      // Keeps unconnected notes and small components from drifting far away.
      .force("x", forceX(0).strength(0.04))
      .force("y", forceY(0).strength(0.04))
      .stop();
    const ticks = n > 800 ? 120 : 300;
    for (let i = 0; i < ticks; i++) simulation.tick();
  }

  const laid = sim.map<LaidOutNode>((d) => ({
    id: d.id,
    title: d.title,
    path: d.path,
    archived: d.archived,
    degree: d.degree,
    x: d.x ?? 0,
    y: d.y ?? 0,
    r: nodeRadius(d.degree),
  }));
  const laidById = new Map(laid.map((d) => [d.id, d]));
  const edges: LaidOutEdge[] = [...pairs.values()].flatMap((p) => {
    const source = laidById.get(p.a);
    const target = laidById.get(p.b);
    return source && target ? [{ source, target, mutual: p.mutual }] : [];
  });

  return { nodes: laid, edges, bounds: boundsOf(laid) };
}

export function boundsOf(nodes: readonly LaidOutNode[]): Bounds {
  if (nodes.length === 0) return { minX: 0, minY: 0, maxX: 0, maxY: 0 };
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const d of nodes) {
    minX = Math.min(minX, d.x - d.r);
    minY = Math.min(minY, d.y - d.r);
    maxX = Math.max(maxX, d.x + d.r);
    maxY = Math.max(maxY, d.y + d.r);
  }
  return { minX, minY, maxX, maxY };
}

export interface Transform {
  /** Scale. */
  k: number;
  /** Screen position of the world origin. */
  x: number;
  y: number;
}

export const MIN_ZOOM = 0.05;
export const MAX_ZOOM = 8;

export function clampZoom(k: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, k));
}

/** A transform that shows all of `bounds` centred in a `width` x `height` canvas. */
export function fitTransform(
  bounds: Bounds,
  width: number,
  height: number,
  padding = 32,
): Transform {
  const w = Math.max(bounds.maxX - bounds.minX, 1);
  const h = Math.max(bounds.maxY - bounds.minY, 1);
  const k = clampZoom(Math.min((width - padding * 2) / w, (height - padding * 2) / h, 1.5));
  const cx = (bounds.minX + bounds.maxX) / 2;
  const cy = (bounds.minY + bounds.maxY) / 2;
  return { k, x: width / 2 - cx * k, y: height / 2 - cy * k };
}

/** Zooms by `factor` keeping the world point under screen position (`px`, `py`) fixed. */
export function zoomAt(t: Transform, factor: number, px: number, py: number): Transform {
  const k = clampZoom(t.k * factor);
  const f = k / t.k;
  return { k, x: px - (px - t.x) * f, y: py - (py - t.y) * f };
}

export function toWorld(t: Transform, sx: number, sy: number): { x: number; y: number } {
  return { x: (sx - t.x) / t.k, y: (sy - t.y) / t.k };
}

/** The note drawn under screen position (`sx`, `sy`), preferring the topmost (last drawn). */
export function pickNode(
  layout: Layout,
  t: Transform,
  sx: number,
  sy: number,
  slop = 3,
): LaidOutNode | null {
  const { x, y } = toWorld(t, sx, sy);
  for (let i = layout.nodes.length - 1; i >= 0; i--) {
    const d = layout.nodes[i];
    if (!d) continue;
    const reach = d.r + slop / t.k;
    if ((d.x - x) ** 2 + (d.y - y) ** 2 <= reach * reach) return d;
  }
  return null;
}

/** Ids of the notes directly linked to `id`, in either direction. */
export function neighbourIds(layout: Layout, id: string): Set<string> {
  const out = new Set<string>();
  for (const e of layout.edges) {
    if (e.source.id === id) out.add(e.target.id);
    else if (e.target.id === id) out.add(e.source.id);
  }
  return out;
}
