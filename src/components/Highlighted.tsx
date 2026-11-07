import type { SnippetPart } from "../lib/api";

/** Text with the parts the search matched marked up; built from data, never from HTML. */
export function Highlighted({ parts }: { parts: readonly SnippetPart[] }) {
  return (
    <>
      {parts.map((p, i) =>
        p.matched ? (
          <mark key={i} className="rounded bg-selection px-0.5 text-fg">
            {p.text}
          </mark>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  );
}
