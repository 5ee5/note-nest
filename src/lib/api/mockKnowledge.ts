/**
 * Browser-side stand-in for the Rust knowledge index, used by `MockNotesApi`. It follows
 * the same rules as `crates/notenest-core` (tag syntax, wiki links that ignore code, link
 * resolution precedence, query syntax) so the UI behaves alike in a browser and in tests.
 * It is deliberately simple; the real implementation is tested in Rust.
 */
import { normalizeTag } from "../tags";
import type {
  AffectedNote,
  Backlink,
  BrokenLink,
  Graph,
  NoteLinks,
  NoteRef,
  RelatedNote,
  SearchHit,
  SnippetPart,
  TagCount,
} from "./types";

export interface KNote {
  id: string;
  path: string;
  title: string;
  content: string;
  archived: boolean;
  /** Tags that exist regardless of the content (sample data). */
  seedTags: string[];
}

// ------------------------------------------------------------------ text helpers

const FRONTMATTER = new RegExp(
  "^\\uFEFF?---\\r?\\n([\\s\\S]*?\\r?\\n)?(---|\\.\\.\\.)[ \\t]*(\\r?\\n|$)",
);

export function bodyOf(content: string): string {
  const m = FRONTMATTER.exec(content);
  return m ? content.slice(m[0].length) : content;
}

/** Blanks out code (same length, newlines kept) so offsets and line numbers stay valid. */
function maskCode(text: string): string {
  const blank = (s: string) => s.replace(/[^\n]/g, " ");
  return text.replace(/```[\s\S]*?```/g, blank).replace(/`[^`\n]*`/g, blank);
}

function frontmatterList(content: string, key: string): string[] {
  const m = FRONTMATTER.exec(content);
  if (!m) return [];
  const lines = (m[1] ?? "").split(/\r?\n/);
  const i = lines.findIndex((l) => l.startsWith(`${key}:`));
  if (i === -1) return [];
  const value = (lines[i] ?? "").slice(key.length + 1).trim();
  const items: string[] = [];
  if (value === "") {
    for (let j = i + 1; j < lines.length && /^\s*-\s/.test(lines[j] ?? ""); j++) {
      items.push((lines[j] ?? "").replace(/^\s*-\s*/, ""));
    }
  } else if (value.startsWith("[") && value.endsWith("]")) {
    items.push(...value.slice(1, -1).split(","));
  } else {
    items.push(...value.split(key === "tags" ? /[,\s]+/ : ","));
  }
  return items.map((s) => s.trim().replace(/^["']|["']$/g, "")).filter((s) => s !== "");
}

export function parseTags(content: string): { frontmatter: string[]; inline: string[] } {
  const frontmatter = [
    ...new Set(
      frontmatterList(content, "tags")
        .map(normalizeTag)
        .filter((t): t is string => !!t),
    ),
  ];
  const text = maskCode(bodyOf(content)).replace(/\[\[[^\]]*\]\]/g, (s) => " ".repeat(s.length));
  const inline: string[] = [];
  for (const m of text.matchAll(/(^|[\s([{"'])#([\p{L}\p{N}_/-]+)/gu)) {
    const t = normalizeTag(m[2] ?? "");
    if (t && !frontmatter.includes(t) && !inline.includes(t)) inline.push(t);
  }
  return { frontmatter, inline };
}

export function parseAliases(content: string): string[] {
  return frontmatterList(content, "aliases");
}

export function tagsOf(n: KNote): { tags: string[]; inline: string[] } {
  const { frontmatter, inline } = parseTags(n.content);
  const fm = [...new Set([...n.seedTags, ...frontmatter])];
  return { tags: [...fm, ...inline], inline };
}

// ------------------------------------------------------------------------ links

export interface MLink {
  targetText: string;
  key: string;
  display: string | null;
  line: number;
  context: string;
  start: number;
  end: number;
}

export function linkKey(target: string): string {
  const base = (target.split(/[#^]/)[0] ?? "").trim().replace(/\.md$/i, "");
  return base.trim().toLowerCase();
}

export function extractLinks(content: string): MLink[] {
  const masked = maskCode(content);
  const links: MLink[] = [];
  for (const m of masked.matchAll(/\[\[([^[\]|\n]+?)(?:\|([^[\]\n]*))?\]\]/g)) {
    const start = m.index ?? 0;
    const key = linkKey(m[1] ?? "");
    if (key === "") continue;
    const line = content.slice(0, start).split("\n").length;
    const lineText = content.split("\n")[line - 1] ?? "";
    links.push({
      targetText: m[1] ?? "",
      key,
      display: m[2] && m[2].trim() !== "" ? m[2].trim() : null,
      line,
      context: lineText.trim().slice(0, 200),
      start,
      end: start + m[0].length,
    });
  }
  return links;
}

const folderOf = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
const pathKey = (path: string) => path.toLowerCase().replace(/\.md$/, "");

type Kind = 0 | 1 | 2; // path, title, alias
interface Candidate {
  note: KNote;
  kind: Kind;
}

/** Same precedence as the Rust resolver: path, then title, then alias; own folder; shortest path. */
export class MockResolver {
  private byKey = new Map<string, Candidate[]>();

  constructor(notes: readonly KNote[]) {
    const add = (key: string, note: KNote, kind: Kind) => {
      if (key === "") return;
      const list = this.byKey.get(key) ?? [];
      if (!list.some((c) => c.note.id === note.id)) list.push({ note, kind });
      this.byKey.set(key, list);
    };
    for (const n of notes) {
      const pk = pathKey(n.path);
      add(pk, n, pk.includes("/") ? 0 : 1);
      add(n.title.toLowerCase(), n, 1);
      for (const a of parseAliases(n.content)) add(a.toLowerCase(), n, 2);
    }
  }

  resolve(sourcePath: string, key: string): KNote | undefined {
    const folder = folderOf(sourcePath);
    const list = this.byKey.get(key);
    if (!list) return undefined;
    return [...list].sort(
      (a, b) =>
        a.kind - b.kind ||
        Number(folderOf(a.note.path) !== folder) - Number(folderOf(b.note.path) !== folder) ||
        a.note.path.length - b.note.path.length ||
        a.note.path.localeCompare(b.note.path),
    )[0]?.note;
  }
}

const ref = (n: KNote): NoteRef => ({ id: n.id, title: n.title, path: n.path });

export function noteLinks(notes: readonly KNote[], id: string): NoteLinks {
  const resolver = new MockResolver(notes);
  const me = notes.find((n) => n.id === id);
  if (!me) return { backlinks: [], outgoing: [], related: [] };
  const backlinks: Backlink[] = [];
  for (const s of [...notes].sort((a, b) => a.path.localeCompare(b.path))) {
    if (s.id === id) continue;
    for (const l of extractLinks(s.content)) {
      if (resolver.resolve(s.path, l.key)?.id === id) {
        backlinks.push({
          source: ref(s),
          line: l.line,
          context: l.context,
          targetText: l.targetText,
          display: l.display,
        });
      }
    }
  }
  const outgoing = extractLinks(me.content).map((l) => {
    const t = resolver.resolve(me.path, l.key);
    return {
      targetText: l.targetText,
      display: l.display,
      line: l.line,
      context: l.context,
      target: t ? ref(t) : null,
    };
  });
  return { backlinks, outgoing, related: related(notes, me, resolver) };
}

function edges(notes: readonly KNote[]): [string, string][] {
  const resolver = new MockResolver(notes);
  const seen = new Set<string>();
  const out: [string, string][] = [];
  for (const s of notes) {
    for (const l of extractLinks(s.content)) {
      const t = resolver.resolve(s.path, l.key);
      if (t && t.id !== s.id && !seen.has(`${s.id}>${t.id}`)) {
        seen.add(`${s.id}>${t.id}`);
        out.push([s.id, t.id]);
      }
    }
  }
  return out;
}

function related(notes: readonly KNote[], me: KNote, resolver: MockResolver): RelatedNote[] {
  const myTags = new Set(tagsOf(me).tags);
  const linked = new Set<string>();
  for (const [a, b] of edges(notes)) {
    if (a === me.id) linked.add(b);
    if (b === me.id) linked.add(a);
  }
  void resolver;
  return notes
    .filter((n) => n.id !== me.id && !n.archived)
    .map((n) => {
      const shared = tagsOf(n).tags.filter((t) => myTags.has(t)).length;
      const isLinked = linked.has(n.id);
      return { note: ref(n), shared, isLinked, score: (isLinked ? 30 : 0) + shared * 3 };
    })
    .filter((r) => r.isLinked || r.shared > 0)
    .sort((a, b) => b.score - a.score || a.note.path.localeCompare(b.note.path))
    .slice(0, 8)
    .map((r) => ({ note: r.note, sharedTags: r.shared, linked: r.isLinked }));
}

export function graph(notes: readonly KNote[], center: string | null, depth: number): Graph {
  const all = edges(notes);
  const neighbours = new Map<string, Set<string>>();
  for (const [a, b] of all) {
    (neighbours.get(a) ?? neighbours.set(a, new Set()).get(a)!).add(b);
    (neighbours.get(b) ?? neighbours.set(b, new Set()).get(b)!).add(a);
  }
  let keep: Set<string> | null = null;
  if (center) {
    keep = new Set([center]);
    let frontier = [center];
    for (let d = 0; d < depth; d++) {
      const next: string[] = [];
      for (const n of frontier) {
        for (const m of neighbours.get(n) ?? []) {
          if (keep.has(m)) continue;
          keep.add(m);
          next.push(m);
        }
      }
      frontier = next;
    }
  }
  const included = (id: string) => !keep || keep.has(id);
  return {
    nodes: notes
      .filter((n) => included(n.id))
      .sort((a, b) => a.path.localeCompare(b.path))
      .map((n) => ({
        id: n.id,
        title: n.title,
        path: n.path,
        archived: n.archived,
        degree: neighbours.get(n.id)?.size ?? 0,
      })),
    edges: all.filter(([a, b]) => included(a) && included(b)),
    totalNotes: notes.length,
  };
}

export function orphans(notes: readonly KNote[]): NoteRef[] {
  const connected = new Set(edges(notes).flat());
  return notes
    .filter((n) => !n.archived && !connected.has(n.id))
    .sort((a, b) => a.path.localeCompare(b.path))
    .map(ref);
}

export function brokenLinks(notes: readonly KNote[]): BrokenLink[] {
  const resolver = new MockResolver(notes);
  const out: BrokenLink[] = [];
  for (const s of [...notes].sort((a, b) => a.path.localeCompare(b.path))) {
    for (const l of extractLinks(s.content)) {
      if (!resolver.resolve(s.path, l.key))
        out.push({ source: ref(s), targetText: l.targetText, line: l.line, context: l.context });
    }
  }
  return out;
}

// ------------------------------------------------------------------------- tags

export function tagCounts(notes: readonly KNote[]): TagCount[] {
  const sets = new Map<string, Set<string>>();
  for (const n of notes) {
    for (const tag of tagsOf(n).tags) {
      const parts = tag.split("/");
      for (let i = 1; i <= parts.length; i++) {
        const name = parts.slice(0, i).join("/");
        (sets.get(name) ?? sets.set(name, new Set()).get(name)!).add(n.id);
      }
    }
  }
  return [...sets]
    .map(([name, s]) => ({ name, count: s.size }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** Replaces the frontmatter `tags` list (flow style); removes the key (and an empty block). */
export function setFrontmatterTags(content: string, tags: string[]): string {
  const clean = [...new Set(tags.map(normalizeTag).filter((t): t is string => !!t))];
  const line = `tags: [${clean.join(", ")}]`;
  const m = FRONTMATTER.exec(content);
  if (!m) return clean.length === 0 ? content : `---\n${line}\n---\n${content}`;
  const lines = (m[1] ?? "").split("\n").filter((l) => l !== "");
  const i = lines.findIndex((l) => l.startsWith("tags:"));
  let j = i + 1;
  if (i !== -1 && (lines[i] ?? "").slice(5).trim() === "")
    while (/^\s*-\s/.test(lines[j] ?? "")) j++;
  const kept = i === -1 ? lines : [...lines.slice(0, i), ...lines.slice(j)];
  const next =
    clean.length > 0
      ? i === -1
        ? [...kept, line]
        : [...kept.slice(0, i), line, ...kept.slice(i)]
      : kept;
  const rest = content.slice(m[0].length);
  return next.length === 0 ? rest : `---\n${next.join("\n")}\n---\n${rest}`;
}

// ----------------------------------------------------------------------- search

const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

interface Parsed {
  terms: { text: string; phrase: boolean }[];
  tags: string[];
}

export function parseQuery(input: string): Parsed {
  const parsed: Parsed = { terms: [], tags: [] };
  for (const m of input.matchAll(/"([^"]*)"|(\S+)/g)) {
    const phrase = m[1] !== undefined;
    const token = (m[1] ?? m[2] ?? "").replace(/\p{C}/gu, "");
    if (!phrase) {
      const name = token.startsWith("#")
        ? token.slice(1)
        : token.startsWith("tag:")
          ? token.slice(4)
          : null;
      if (name !== null) {
        const t = normalizeTag(name);
        if (t && !parsed.tags.includes(t)) parsed.tags.push(t);
        continue;
      }
    }
    const text = token.split(/\s+/).filter(Boolean).join(" ").slice(0, 100);
    if (/[\p{L}\p{N}]/u.test(text) && parsed.terms.length < 16) parsed.terms.push({ text, phrase });
  }
  return parsed;
}

function markParts(text: string, terms: { text: string; phrase: boolean }[]): SnippetPart[] {
  const folded = fold(text);
  const ranges: [number, number][] = [];
  for (const t of terms) {
    const re = new RegExp(
      `(?<![\\p{L}\\p{N}])${escapeRe(fold(t.text))}${t.phrase ? "" : "[\\p{L}\\p{N}]*"}`,
      "gu",
    );
    for (const m of folded.matchAll(re)) ranges.push([m.index ?? 0, (m.index ?? 0) + m[0].length]);
  }
  ranges.sort((a, b) => a[0] - b[0]);
  const parts: SnippetPart[] = [];
  let at = 0;
  for (const [s, e] of ranges) {
    if (s < at) continue;
    if (s > at) parts.push({ text: text.slice(at, s), matched: false });
    parts.push({ text: text.slice(s, e), matched: true });
    at = e;
  }
  if (at < text.length) parts.push({ text: text.slice(at), matched: false });
  return parts;
}

export function search(notes: readonly KNote[], query: string, limit: number): SearchHit[] {
  const q = parseQuery(query);
  if (q.terms.length === 0 && q.tags.length === 0) return [];
  const hits: { hit: SearchHit; score: number; updated: number }[] = [];
  for (const n of notes) {
    const { tags } = tagsOf(n);
    if (!q.tags.every((t) => tags.some((x) => x === t || x.startsWith(`${t}/`)))) continue;
    const body = bodyOf(n.content);
    const hay = (text: string, term: (typeof q.terms)[number]) =>
      new RegExp(`(?<![\\p{L}\\p{N}])${escapeRe(fold(term.text))}`, "u").test(fold(text));
    if (!q.terms.every((t) => hay(n.title, t) || hay(body, t) || hay(tags.join(" "), t))) continue;
    const titleScore = q.terms.filter((t) => hay(n.title, t)).length;
    const first = q.terms.length
      ? fold(body).search(new RegExp(`(?<![\\p{L}\\p{N}])${escapeRe(fold(q.terms[0]!.text))}`, "u"))
      : -1;
    const start = Math.max(0, first - 40);
    const window = first === -1 ? body.slice(0, 100) : body.slice(start, start + 120);
    hits.push({
      hit: {
        id: n.id,
        title: n.title,
        path: n.path,
        archived: n.archived,
        titleParts: markParts(n.title, q.terms),
        snippet: markParts(window.replace(/\s+/g, " ").trim(), q.terms),
      },
      score: titleScore,
      updated: 0,
    });
  }
  return hits
    .sort((a, b) => b.score - a.score || a.hit.path.localeCompare(b.hit.path))
    .slice(0, limit)
    .map((h) => h.hit);
}

// ------------------------------------------------------------------ link updates

export interface PlannedEdit {
  line: number;
  old: string;
  next: string | null;
}

function rewriteTarget(
  old: string,
  newPath: string,
  sourcePath: string,
  resolver: MockResolver,
  targetId: string,
): string | null {
  const at = old.search(/[#^]/);
  const base = at === -1 ? old : old.slice(0, at);
  const suffix = at === -1 ? "" : old.slice(at);
  const ext = /\.md$/i.test(base) ? ".md" : "";
  const noExt = newPath.replace(/\.md$/, "");
  const title = noExt.slice(noExt.lastIndexOf("/") + 1);
  for (const form of base.includes("/") ? [noExt] : [title, noExt]) {
    const candidate = `${form}${ext}${suffix}`;
    if (resolver.resolve(sourcePath, linkKey(candidate))?.id === targetId) return candidate;
  }
  return null;
}

/** For each note, the links that stop resolving if `moves` (id → new path and title) happen. */
export function planLinkUpdates(
  notes: readonly KNote[],
  moves: ReadonlyMap<string, { path: string; title: string }>,
): Map<string, PlannedEdit[]> {
  const before = new MockResolver(notes);
  const moved = notes.map((n) => {
    const m = moves.get(n.id);
    return m ? { ...n, path: m.path, title: m.title } : n;
  });
  const after = new MockResolver(moved);
  const plan = new Map<string, PlannedEdit[]>();
  for (const s of notes) {
    const sNew = moved.find((n) => n.id === s.id)!;
    for (const l of extractLinks(s.content)) {
      const target = before.resolve(s.path, l.key);
      if (!target || !moves.has(target.id)) continue;
      if (after.resolve(sNew.path, l.key)?.id === target.id) continue;
      const next = rewriteTarget(
        l.targetText,
        moves.get(target.id)!.path,
        sNew.path,
        after,
        target.id,
      );
      (plan.get(s.id) ?? plan.set(s.id, []).get(s.id)!).push({
        line: l.line,
        old: l.targetText,
        next,
      });
    }
  }
  return plan;
}

export function affected(
  notes: readonly KNote[],
  plan: Map<string, PlannedEdit[]>,
): AffectedNote[] {
  return [...plan]
    .map(([id, edits]) => ({ note: ref(notes.find((n) => n.id === id)!), links: edits.length }))
    .sort((a, b) => a.note.path.localeCompare(b.note.path));
}

/** Applies planned edits to a note's content; returns the new text and how many were applied. */
export function applyEdits(
  content: string,
  edits: PlannedEdit[],
): { text: string; applied: number } {
  const pending = edits.filter((e) => e.next !== null);
  const reps: { start: number; end: number; next: string }[] = [];
  for (const l of extractLinks(content)) {
    const i = pending.findIndex((e) => e.line === l.line && e.old === l.targetText);
    if (i === -1) continue;
    const [e] = pending.splice(i, 1);
    const span = content.slice(l.start, l.end);
    const at = span.indexOf(e!.old);
    if (at !== -1)
      reps.push({ start: l.start + at, end: l.start + at + e!.old.length, next: e!.next! });
  }
  let text = content;
  for (const r of reps.sort((a, b) => b.start - a.start))
    text = text.slice(0, r.start) + r.next + text.slice(r.end);
  return { text, applied: reps.length };
}
