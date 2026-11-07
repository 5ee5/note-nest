import { describe, expect, it } from "vitest";
import {
  applyEdits,
  extractLinks,
  graph,
  linkKey,
  MockResolver,
  noteLinks,
  normalizeTag,
  orphans,
  parseQuery,
  parseTags,
  planLinkUpdates,
  search,
  setFrontmatterTags,
  tagCounts,
  type KNote,
} from "./mockKnowledge";

let counter = 0;
const note = (path: string, content = "", extra: Partial<KNote> = {}): KNote => ({
  id: `n${++counter}`,
  path,
  title: path.replace(/^.*\//, "").replace(/\.md$/, ""),
  content,
  archived: false,
  seedTags: [],
  ...extra,
});

describe("tags", () => {
  it("normalizes like the backend", () => {
    expect(normalizeTag("#Programming/Rust")).toBe("programming/rust");
    expect(normalizeTag("my tag")).toBe("my-tag");
    for (const bad of ["", "123", "c++", "-"]) expect(normalizeTag(bad)).toBeNull();
  });

  it("reads frontmatter and inline tags, skipping code and URLs", () => {
    const md =
      "---\ntags: [Rust, a/b]\n---\nText #rust #new and `#code` and https://x.com/#frag #123\n```\n#block\n```\n";
    expect(parseTags(md)).toEqual({ frontmatter: ["rust", "a/b"], inline: ["new"] });
  });

  it("edits the frontmatter list only", () => {
    expect(setFrontmatterTags("Body", ["A", "b"])).toBe("---\ntags: [a, b]\n---\nBody");
    expect(setFrontmatterTags("---\ntitle: x\ntags: [old]\n---\nB", ["new"])).toBe(
      "---\ntitle: x\ntags: [new]\n---\nB",
    );
    expect(setFrontmatterTags("---\ntags: [old]\n---\nB", [])).toBe("B");
    expect(setFrontmatterTags("---\ntags:\n  - a\n  - b\n---\nB", ["c"])).toBe(
      "---\ntags: [c]\n---\nB",
    );
  });

  it("counts tags with nested roll-up", () => {
    const notes = [
      note("A.md", "#programming/rust"),
      note("B.md", "#programming/python #programming/rust"),
    ];
    expect(tagCounts(notes)).toEqual([
      { name: "programming", count: 2 },
      { name: "programming/python", count: 1 },
      { name: "programming/rust", count: 2 },
    ]);
  });
});

describe("links", () => {
  it("finds wiki links outside code with lines and display text", () => {
    const links = extractLinks(
      "Intro\n\nSee [[Rust]] and [[a/b|shown]] `[[code]]`\n```\n[[block]]\n```\n[[Note#H]]",
    );
    expect(links.map((l) => [l.key, l.display, l.line])).toEqual([
      ["rust", null, 3],
      ["a/b", "shown", 3],
      ["note", null, 7],
    ]);
    expect(linkKey("Folder/Note.md#H")).toBe("folder/note");
  });

  it("resolves like the Rust resolver", () => {
    const notes = [
      note("Alpha.md"),
      note("Projects/Alpha.md"),
      note("Other.md", "---\naliases: [AA]\n---\n"),
    ];
    const r = new MockResolver(notes);
    expect(r.resolve("Projects/N.md", "alpha")?.path).toBe("Projects/Alpha.md");
    expect(r.resolve("N.md", "alpha")?.path).toBe("Alpha.md");
    expect(r.resolve("N.md", "projects/alpha")?.path).toBe("Projects/Alpha.md");
    expect(r.resolve("N.md", "aa")?.path).toBe("Other.md");
    expect(r.resolve("N.md", "nope")).toBeUndefined();
  });

  it("computes backlinks, graph and orphans", () => {
    const a = note("A.md", "[[B]] [[Nope]]");
    const b = note("B.md", "[[A]]");
    const lone = note("Lone.md", "");
    const notes = [a, b, lone];
    expect(noteLinks(notes, b.id).backlinks.map((l) => l.source.title)).toEqual(["A"]);
    expect(noteLinks(notes, a.id).outgoing.map((l) => l.target?.title ?? null)).toEqual([
      "B",
      null,
    ]);
    expect(graph(notes, null, 0).edges).toHaveLength(2);
    expect(graph(notes, a.id, 1).nodes.map((n) => n.title)).toEqual(["A", "B"]);
    expect(orphans(notes).map((n) => n.title)).toEqual(["Lone"]);
  });
});

describe("search", () => {
  const notes = [
    note("Programming.md", "Systems programming in Rust.", { seedTags: ["lang/rust"] }),
    note("Épsilon.md", "A Greek letter."),
    note("Journal.md", "cooking cooking cooking"),
    note("Notes on cooking.md", "recipes"),
  ];

  it("matches word prefixes, ignoring case and accents", () => {
    expect(search(notes, "PROG", 10).map((h) => h.title)).toEqual(["Programming"]);
    expect(search(notes, "epsi", 10).map((h) => h.title)).toEqual(["Épsilon"]);
    expect(search(notes, "zzz", 10)).toEqual([]);
  });

  it("ranks title matches first and highlights", () => {
    const hits = search(notes, "cooking", 10);
    expect(hits.map((h) => h.title)).toEqual(["Notes on cooking", "Journal"]);
    expect(hits[1]!.snippet.some((p) => p.matched)).toBe(true);
  });

  it("filters by tag including nested tags", () => {
    expect(search(notes, "#lang", 10).map((h) => h.title)).toEqual(["Programming"]);
    expect(search(notes, "rust tag:lang/rust", 10)).toHaveLength(1);
    expect(parseQuery('a "b c" #T tag:x/y').tags).toEqual(["t", "x/y"]);
  });

  it("never throws on odd input", () => {
    for (const q of ['"', "***", "\u0000", "(", "#", "tag:"])
      expect(() => search(notes, q, 5)).not.toThrow();
  });
});

describe("link updates", () => {
  const target = note("Docs/Old Name.md", "---\naliases: [Legacy]\n---\n");
  const linker = note(
    "Linker.md",
    "A [[Old Name]] b [[Old Name|shown]] c [[Old Name#H]] d [[Docs/Old Name]] e [[Legacy]] f [[Other]]\n```\n[[Old Name]]\n```\n",
  );
  const notes = [target, linker, note("Other.md")];

  it("plans only the links that would stop resolving", () => {
    const plan = planLinkUpdates(
      notes,
      new Map([[target.id, { path: "Docs/New.md", title: "New" }]]),
    );
    const edits = plan.get(linker.id)!;
    expect(edits.map((e) => [e.old, e.next])).toEqual([
      ["Old Name", "New"],
      ["Old Name", "New"],
      ["Old Name#H", "New#H"],
      ["Docs/Old Name", "Docs/New"],
    ]);
  });

  it("applies edits at the right places and leaves code alone", () => {
    const plan = planLinkUpdates(
      notes,
      new Map([[target.id, { path: "Docs/New.md", title: "New" }]]),
    );
    const { text, applied } = applyEdits(linker.content, plan.get(linker.id)!);
    expect(applied).toBe(4);
    expect(text).toContain(
      "A [[New]] b [[New|shown]] c [[New#H]] d [[Docs/New]] e [[Legacy]] f [[Other]]",
    );
    expect(text).toContain("```\n[[Old Name]]\n```");
  });

  it("does not touch title links when only the folder changes", () => {
    const titleOnly = note("T.md", "[[Old Name]]");
    const plan = planLinkUpdates(
      [target, titleOnly],
      new Map([[target.id, { path: "Elsewhere/Old Name.md", title: "Old Name" }]]),
    );
    expect(plan.size).toBe(0);
  });
});
