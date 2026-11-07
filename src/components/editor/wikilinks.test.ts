import { CompletionContext } from "@codemirror/autocomplete";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { inCode, linkAt, tagSource, wikiLinkSource, wikiLinksIn } from "./wikilinks";

function stateOf(doc: string): EditorState {
  const state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
  ensureSyntaxTree(state, state.doc.length, 5000); // make sure code nodes are known
  return state;
}

describe("wikiLinksIn", () => {
  it("finds links with their targets and positions", () => {
    const text = "see [[Rust]] and [[Linux|the OS]] but not [[ ]] or [x]";
    expect(wikiLinksIn(text).map((s) => [s.target, text.slice(s.from, s.to)])).toEqual([
      ["Rust", "[[Rust]]"],
      ["Linux", "[[Linux|the OS]]"],
    ]);
    expect(wikiLinksIn("[[A]]", 10)[0]).toMatchObject({ from: 10, to: 15 });
  });

  it("ignores broken syntax", () => {
    for (const bad of ["[[", "]]", "[[a", "[[a]", "[[]]", "[a]]", "[[a\nb]]"]) {
      expect(wikiLinksIn(bad)).toEqual([]);
    }
  });
});

describe("linkAt and inCode", () => {
  const doc = "plain [[Real]] text `[[inline]]`\n\n```\n[[fenced]]\n```\n";
  const state = stateOf(doc);

  it("finds the link under the cursor, including its edges", () => {
    const start = doc.indexOf("[[Real]]");
    expect(linkAt(state, start + 3)?.target).toBe("Real");
    expect(linkAt(state, start)?.target).toBe("Real");
    expect(linkAt(state, start + "[[Real]]".length)?.target).toBe("Real");
    expect(linkAt(state, start - 3)).toBeNull();
  });

  it("does not treat links in code as links", () => {
    expect(inCode(state, doc.indexOf("[[inline]]") + 3)).toBe(true);
    expect(inCode(state, doc.indexOf("[[fenced]]") + 3)).toBe(true);
    expect(inCode(state, doc.indexOf("plain"))).toBe(false);
    expect(linkAt(state, doc.indexOf("[[inline]]") + 3)).toBeNull();
    expect(linkAt(state, doc.indexOf("[[fenced]]") + 3)).toBeNull();
  });
});

const ctx = (doc: string, explicit = false) => {
  const state = stateOf(doc);
  return new CompletionContext(state, doc.length, explicit);
};

describe("wikiLinkSource", () => {
  const notes = [
    { title: "Rust", path: "Programming/Rust.md" },
    { title: "Linux", path: "Programming/Linux.md" },
    { title: "Dup", path: "A/Dup.md" },
    { title: "Dup", path: "B/Dup.md" },
  ];
  const source = wikiLinkSource(() => notes);

  it("offers titles after [[ and closes the link", () => {
    const result = source(ctx("See [[Ru"))!;
    expect(result.from).toBe("See [[".length);
    const rust = result.options.find((o) => o.label === "Rust")!;
    expect(rust.apply).toBe("Rust]]");
    expect(rust.detail).toBe("Programming");
  });

  it("does not add a second closing bracket", () => {
    const state = stateOf("[[Ru]]");
    const result = source(new CompletionContext(state, 4, false))!;
    expect(result.options.find((o) => o.label === "Rust")!.apply).toBe("Rust");
  });

  it("uses the path when two notes share a title", () => {
    const labels = source(ctx("[["))!.options.map((o) => o.label);
    expect(labels).toContain("A/Dup");
    expect(labels).toContain("B/Dup");
    expect(labels).not.toContain("Dup");
  });

  it("stays quiet elsewhere", () => {
    expect(source(ctx("no link here"))).toBeNull();
    expect(source(ctx("[[done]] and more"))).toBeNull();
    expect(source(ctx("[[a|display"))).toBeNull();
  });
});

describe("tagSource", () => {
  const source = tagSource(() => ["programming/rust", "todo"]);

  it("offers tags once a character follows #", () => {
    const r = source(ctx("note #to"))!;
    expect(r.from).toBe("note #".length);
    expect(r.options.map((o) => o.label)).toEqual(["programming/rust", "todo"]);
    expect(source(ctx("#t"))).not.toBeNull();
  });

  it("does not pop up for headings, URLs or code", () => {
    expect(source(ctx("#"))).toBeNull();
    expect(source(ctx("# Heading"))).toBeNull();
    expect(source(ctx("see https://x.com/#fra"))).toBeNull();
    expect(source(ctx("a#b"))).toBeNull();
    const inline = stateOf("`code #to`");
    expect(source(new CompletionContext(inline, inline.doc.length - 1, false))).toBeNull();
  });

  it("shows all tags on an explicit request", () => {
    expect(source(ctx("text #", true))).not.toBeNull();
  });
});
