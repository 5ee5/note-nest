import { describe, expect, it } from "vitest";
import type { Graph, GraphNode } from "./api/types";
import {
  MAX_ZOOM,
  MIN_ZOOM,
  boundsOf,
  fitTransform,
  layoutGraph,
  neighbourIds,
  pickNode,
  toWorld,
  zoomAt,
} from "./graphLayout";

function node(id: string, degree = 0): GraphNode {
  return { id, title: id.toUpperCase(), path: `${id}.md`, archived: false, degree };
}

const graph: Graph = {
  nodes: [node("a", 2), node("b", 2), node("c", 1), node("d", 1), node("lonely")],
  edges: [
    ["a", "b"],
    ["b", "a"], // mutual with the line above
    ["b", "c"],
    ["a", "d"],
    ["a", "a"], // self link: ignored
    ["a", "gone"], // target not in the graph: ignored
  ],
  totalNotes: 9,
};

describe("layoutGraph", () => {
  it("places every note at a finite position", () => {
    const { nodes } = layoutGraph(graph);
    expect(nodes).toHaveLength(5);
    for (const n of nodes) {
      expect(Number.isFinite(n.x)).toBe(true);
      expect(Number.isFinite(n.y)).toBe(true);
    }
  });

  it("is deterministic", () => {
    const pos = (g: Graph) =>
      layoutGraph(g).nodes.map((n) => [n.id, n.x.toFixed(6), n.y.toFixed(6)]);
    expect(pos(graph)).toEqual(pos(graph));
  });

  it("merges mutual links, and drops self links and links to notes outside the graph", () => {
    const { edges } = layoutGraph(graph);
    expect(edges).toHaveLength(3);
    const ab = edges.find((e) => [e.source.id, e.target.id].sort().join() === "a,b");
    expect(ab?.mutual).toBe(true);
    expect(edges.filter((e) => e.mutual)).toHaveLength(1);
  });

  it("keeps linked notes closer together than a note with no links", () => {
    const { nodes } = layoutGraph(graph);
    const at = (id: string) => nodes.find((n) => n.id === id)!;
    const dist = (p: string, q: string) => Math.hypot(at(p).x - at(q).x, at(p).y - at(q).y);
    expect(dist("a", "b")).toBeLessThan(120);
  });

  it("sizes nodes by their links", () => {
    const { nodes } = layoutGraph(graph);
    const r = (id: string) => nodes.find((n) => n.id === id)!.r;
    expect(r("a")).toBeGreaterThan(r("lonely"));
  });

  it("handles an empty graph and a single note", () => {
    expect(layoutGraph({ nodes: [], edges: [], totalNotes: 0 })).toEqual({
      nodes: [],
      edges: [],
      bounds: { minX: 0, minY: 0, maxX: 0, maxY: 0 },
    });
    const one = layoutGraph({ nodes: [node("a")], edges: [], totalNotes: 1 });
    expect(one.nodes).toHaveLength(1);
    expect(one.bounds.maxX).toBeGreaterThan(one.bounds.minX);
  });

  it("lays out a large graph in a reasonable time", () => {
    const nodes = Array.from({ length: 1200 }, (_, i) => node(`n${i}`, 2));
    const edges = nodes.map((_, i): [string, string] => [`n${i}`, `n${(i * 7 + 1) % 1200}`]);
    const t = performance.now();
    const laid = layoutGraph({ nodes, edges, totalNotes: 1200 });
    expect(laid.nodes).toHaveLength(1200);
    expect(performance.now() - t).toBeLessThan(15000);
  });
});

describe("view transforms", () => {
  it("fits the bounds inside the canvas, centred", () => {
    const layout = layoutGraph(graph);
    const t = fitTransform(layout.bounds, 800, 600);
    for (const n of layout.nodes) {
      const sx = n.x * t.k + t.x;
      const sy = n.y * t.k + t.y;
      expect(sx).toBeGreaterThanOrEqual(0);
      expect(sx).toBeLessThanOrEqual(800);
      expect(sy).toBeGreaterThanOrEqual(0);
      expect(sy).toBeLessThanOrEqual(600);
    }
  });

  it("zooms around the cursor and clamps the scale", () => {
    const t = { k: 1, x: 10, y: 20 };
    const before = toWorld(t, 300, 200);
    const zoomed = zoomAt(t, 2, 300, 200);
    const after = toWorld(zoomed, 300, 200);
    expect(after.x).toBeCloseTo(before.x);
    expect(after.y).toBeCloseTo(before.y);
    expect(zoomAt(t, 1e9, 0, 0).k).toBe(MAX_ZOOM);
    expect(zoomAt(t, 1e-9, 0, 0).k).toBe(MIN_ZOOM);
  });
});

describe("hit testing", () => {
  it("finds the note under a point, and nothing in empty space", () => {
    const layout = layoutGraph(graph);
    const t = fitTransform(layout.bounds, 800, 600);
    const a = layout.nodes.find((n) => n.id === "a")!;
    expect(pickNode(layout, t, a.x * t.k + t.x, a.y * t.k + t.y)?.id).toBe("a");
    expect(pickNode(layout, t, -500, -500)).toBeNull();
  });

  it("lists the neighbours in either direction", () => {
    const layout = layoutGraph(graph);
    expect([...neighbourIds(layout, "b")].sort()).toEqual(["a", "c"]);
    expect([...neighbourIds(layout, "a")].sort()).toEqual(["b", "d"]);
    expect(neighbourIds(layout, "lonely").size).toBe(0);
  });

  it("computes bounds including the node radius", () => {
    const b = boundsOf([{ ...node("a"), x: 10, y: 20, r: 5 }]);
    expect(b).toEqual({ minX: 5, minY: 15, maxX: 15, maxY: 25 });
  });
});
