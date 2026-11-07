import { describe, expect, it } from "vitest";
import type { NoteSummary } from "./api/types";
import { allTags, selectNotes } from "./notes";

const note = (id: string, over: Partial<NoteSummary> = {}): NoteSummary => ({
  id,
  title: id,
  path: `${id}.md`,
  folder: "",
  tags: [],
  inlineTags: [],
  pinned: false,
  archived: false,
  preview: "",
  createdAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
  lastOpenedAt: null,
  ...over,
});

const NOTES = [
  note("a", { title: "Alpha", updatedAt: "2026-01-02T00:00:00.000Z", tags: ["programming/rust"] }),
  note("b", {
    title: "Beta",
    pinned: true,
    folder: "Programming",
    lastOpenedAt: "2026-02-01T00:00:00.000Z",
    updatedAt: "2026-01-03T00:00:00.000Z",
  }),
  note("c", {
    title: "2026-11-06",
    folder: "Journal",
    lastOpenedAt: "2026-03-01T00:00:00.000Z",
  }),
];

describe("selectNotes", () => {
  it("sorts by last modified, newest first, without mutating input", () => {
    const copy = [...NOTES];
    const ids = selectNotes(NOTES, { kind: "all" }, "updated", "").map((n) => n.id);
    expect(ids).toEqual(["b", "a", "c"]);
    expect(NOTES).toEqual(copy);
  });

  it("sorts by title", () => {
    const titles = selectNotes(NOTES, { kind: "all" }, "title", "").map((n) => n.title);
    expect(titles).toEqual(["2026-11-06", "Alpha", "Beta"]);
  });

  it("filters views", () => {
    const ids = (v: Parameters<typeof selectNotes>[1]) =>
      selectNotes(NOTES, v, "updated", "").map((n) => n.id);
    expect(ids({ kind: "favorites" })).toEqual(["b"]);
    expect(ids({ kind: "daily" })).toEqual(["c"]);
    expect(ids({ kind: "folder", value: "Programming" })).toEqual(["b"]);
    expect(ids({ kind: "tag", value: "programming" })).toEqual(["a"]);
  });

  it("hides archived notes everywhere except the Archive view", () => {
    const withArchived = [...NOTES, note("z", { title: "Old", archived: true })];
    const ids = (v: Parameters<typeof selectNotes>[1]) =>
      selectNotes(withArchived, v, "title", "").map((n) => n.id);
    expect(ids({ kind: "all" })).not.toContain("z");
    expect(ids({ kind: "archived" })).toEqual(["z"]);
  });

  it("orders recently opened by open time", () => {
    const ids = selectNotes(NOTES, { kind: "recent" }, "title", "").map((n) => n.id);
    expect(ids).toEqual(["c", "b"]);
  });

  it("matches queries case-insensitively against title and tags", () => {
    expect(selectNotes(NOTES, { kind: "all" }, "updated", "ALPH").map((n) => n.id)).toEqual(["a"]);
    expect(selectNotes(NOTES, { kind: "all" }, "updated", "rust").map((n) => n.id)).toEqual(["a"]);
    expect(selectNotes(NOTES, { kind: "all" }, "updated", "zzz")).toEqual([]);
  });

  it("collects unique sorted tags", () => {
    expect(allTags([note("x", { tags: ["b", "a"] }), note("y", { tags: ["a"] })])).toEqual([
      "a",
      "b",
    ]);
  });
});
