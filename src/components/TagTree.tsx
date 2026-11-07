import { useState } from "react";
import { ChevronRight, Hash } from "lucide-react";
import { buildTagTree, type TagNode } from "../lib/tags";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";
import { NavButton } from "./NavButton";

/** Names of the ancestors of `tag` (`a/b/c` has `a` and `a/b`). */
function ancestors(tag: string): string[] {
  const parts = tag.split("/");
  return parts.slice(1).map((_, i) => parts.slice(0, i + 1).join("/"));
}

function Branch({
  node,
  depth,
  open,
  toggle,
}: {
  node: TagNode;
  depth: number;
  open: (full: string) => boolean;
  toggle: (full: string) => void;
}) {
  const expanded = open(node.full);
  return (
    <li>
      <div className="flex items-center" style={{ paddingLeft: `${depth * 0.9}rem` }}>
        {node.children.length > 0 ? (
          <button
            type="button"
            aria-label={`${expanded ? "Collapse" : "Expand"} ${node.full}`}
            aria-expanded={expanded}
            onClick={() => toggle(node.full)}
            className="rounded p-0.5 text-fg-muted hover:bg-surface-hover"
          >
            <ChevronRight
              aria-hidden
              className={`size-3.5 transition-transform ${expanded ? "rotate-90" : ""}`}
            />
          </button>
        ) : (
          <span aria-hidden className="w-5" />
        )}
        <NavButton
          icon={Hash}
          label={node.name}
          view={{ kind: "tag", value: node.full }}
          suffix={node.count}
        />
      </div>
      {expanded && node.children.length > 0 && (
        <ul>
          {node.children.map((c) => (
            <Branch key={c.full} node={c} depth={depth + 1} open={open} toggle={toggle} />
          ))}
        </ul>
      )}
    </li>
  );
}

/** Tags as a collapsible tree: `programming/rust` sits under `programming`. */
export function TagTree() {
  const counts = useNotes((s) => s.tagCounts);
  const view = useUi((s) => s.view);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const tree = buildTagTree(counts);
  if (tree.length === 0) return null;

  // The tag being viewed stays visible even if its parents were collapsed.
  const forced = new Set(view.kind === "tag" ? ancestors(view.value) : []);
  const open = (full: string) => expanded.has(full) || forced.has(full);
  const toggle = (full: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(full)) next.delete(full);
      else next.add(full);
      return next;
    });

  return (
    <ul aria-label="Tags">
      {tree.map((n) => (
        <Branch key={n.full} node={n} depth={0} open={open} toggle={toggle} />
      ))}
    </ul>
  );
}
