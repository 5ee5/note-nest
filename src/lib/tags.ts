import type { TagCount } from "./api";

/**
 * Normalizes a tag the same way the backend does (crates/notenest-core/src/tag.rs):
 * lower-case, spaces become `-`, only letters, digits, `_`, `-` and `/`, at least one
 * letter, at most 100 characters. Returns null for anything that is not a valid tag.
 */
export function normalizeTag(raw: string): string | null {
  const t = raw.trim().replace(/^#+/, "").replace(/\s+/g, "-").toLowerCase();
  if (!/^[\p{L}\p{N}_/-]+$/u.test(t)) return null;
  const out = t.replace(/\/{2,}/g, "/").replace(/^[/-]+|[/-]+$/g, "");
  return out !== "" && /\p{L}/u.test(out) && out.length <= 100 ? out : null;
}

export interface TagNode {
  /** Last path segment, e.g. `rust`. */
  name: string;
  /** Full tag, e.g. `programming/rust`. */
  full: string;
  /** Notes with this tag or any nested tag (as counted by the backend). */
  count: number;
  children: TagNode[];
}

/** Builds the nested tag tree from the flat, roll-up counted list of tags. */
export function buildTagTree(counts: readonly TagCount[]): TagNode[] {
  const nodes = new Map<string, TagNode>();
  for (const { name, count } of counts) {
    nodes.set(name, {
      name: name.slice(name.lastIndexOf("/") + 1),
      full: name,
      count,
      children: [],
    });
  }
  const roots: TagNode[] = [];
  for (const node of nodes.values()) {
    const slash = node.full.lastIndexOf("/");
    const parent = slash === -1 ? undefined : nodes.get(node.full.slice(0, slash));
    (parent ? parent.children : roots).push(node);
  }
  const sort = (list: TagNode[]) => {
    list.sort((a, b) => a.name.localeCompare(b.name));
    list.forEach((n) => sort(n.children));
  };
  sort(roots);
  return roots;
}
