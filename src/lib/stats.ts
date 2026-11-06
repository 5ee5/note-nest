/** Word and character counts for the status bar. */

export interface TextStats {
  words: number;
  /** Characters (Unicode code points, whitespace included). */
  characters: number;
}

/** Above this many characters words are counted by whitespace only, which is much faster. */
export const FAST_PATH_CHARS = 200_000;

/** The note text without its leading frontmatter block (same rules as the backend). */
export function stripFrontmatter(content: string): string {
  const text = content.startsWith("﻿") ? content.slice(1) : content;
  if (!/^---\r?\n/.test(text)) return text;
  let offset = text.indexOf("\n") + 1;
  while (offset < text.length) {
    const end = text.indexOf("\n", offset);
    const line = text.slice(offset, end === -1 ? text.length : end).replace(/\r$/, "");
    if (line === "---" || line === "...") return end === -1 ? "" : text.slice(end + 1);
    if (end === -1) break;
    offset = end + 1;
  }
  return text; // no closing delimiter: not frontmatter
}

const isSpace = (c: number) => c === 32 || (c >= 9 && c <= 13) || c === 0xa0 || c === 0x3000;

/** Words (whitespace-separated runs) and code points in one allocation-free pass. */
function countFast(text: string): TextStats {
  let words = 0;
  let characters = text.length;
  let inWord = false;
  let prevHigh = false;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    const space = isSpace(c);
    if (!space && !inWord) words++;
    inWord = !space;
    // A low surrogate right after a high one is the second half of one code point.
    if (prevHigh && c >= 0xdc00 && c <= 0xdfff) characters--;
    prevHigh = c >= 0xd800 && c <= 0xdbff;
  }
  return { words, characters };
}

let segmenter: Intl.Segmenter | null | undefined;

function countWords(text: string): number {
  if (segmenter === undefined) {
    segmenter =
      typeof Intl !== "undefined" && "Segmenter" in Intl
        ? new Intl.Segmenter(undefined, { granularity: "word" })
        : null;
  }
  if (!segmenter) return countFast(text).words;
  let words = 0;
  for (const part of segmenter.segment(text)) if (part.isWordLike) words++;
  return words;
}

function countCharacters(text: string): number {
  // UTF-16 length minus one for every surrogate pair gives the number of code points.
  let n = text.length;
  for (let i = 0; i < text.length - 1; i++) {
    const hi = text.charCodeAt(i);
    if (hi >= 0xd800 && hi <= 0xdbff) {
      const lo = text.charCodeAt(i + 1);
      if (lo >= 0xdc00 && lo <= 0xdfff) {
        n--;
        i++;
      }
    }
  }
  return n;
}

/** Counts for arbitrary text (e.g. a selection). */
export function textStats(text: string): TextStats {
  if (text.length > FAST_PATH_CHARS) return countFast(text);
  return { words: countWords(text), characters: countCharacters(text) };
}

/** Counts for a whole note: its body, without frontmatter. */
export function noteStats(content: string): TextStats {
  return textStats(stripFrontmatter(content));
}
