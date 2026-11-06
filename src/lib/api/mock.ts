import type { NoteSummary, NotesApi } from "./types";

const iso = (daysAgo: number, hour = 10): string => {
  const d = new Date();
  d.setDate(d.getDate() - daysAgo);
  d.setHours(hour, 0, 0, 0);
  return d.toISOString();
};

export function localDateString(date = new Date()): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function sampleNotes(): NoteSummary[] {
  const n = (
    id: string,
    folder: string,
    title: string,
    tags: string[],
    preview: string,
    daysAgo: number,
    extra: Partial<NoteSummary> = {},
  ): NoteSummary => ({
    id,
    title,
    path: folder ? `${folder}/${title}.md` : `${title}.md`,
    folder,
    tags,
    favorite: false,
    preview,
    createdAt: iso(daysAgo + 10),
    updatedAt: iso(daysAgo),
    lastOpenedAt: null,
    ...extra,
  });
  return [
    n("s1", "Inbox", "ideas", [], "Things to look into later: a Markdown linter, a tiny CLI…", 0),
    n("s2", "Programming", "Rust", ["programming/rust"], "Ownership, borrowing and lifetimes…", 1, {
      favorite: true,
      lastOpenedAt: iso(0, 9),
    }),
    n("s3", "Programming", "Linux", ["programming/linux"], "Useful commands and config notes.", 3),
    n(
      "s4",
      "Projects",
      "CodeBuddy",
      ["research/security"],
      "Project goals and open questions.",
      5,
      {
        lastOpenedAt: iso(1, 15),
      },
    ),
    n("s5", "Journal", localDateString(), ["personal/journal"], "Today's plan and reflections.", 0),
    n(
      "s6",
      "",
      "Welcome",
      [],
      "This is sample data. Real notes arrive with the storage engine.",
      20,
      {
        favorite: true,
      },
    ),
  ];
}

/** In-memory stand-in for the real backend. Nothing is written to disk. */
export class MockNotesApi implements NotesApi {
  private notes: NoteSummary[] = sampleNotes();
  private counter = 0;

  listNotes(): Promise<NoteSummary[]> {
    return Promise.resolve(this.notes.map((x) => ({ ...x })));
  }

  listFolders(): Promise<string[]> {
    return Promise.resolve(
      [...new Set(this.notes.map((x) => x.folder).filter((f) => f !== ""))].sort(),
    );
  }

  createNote(): Promise<NoteSummary> {
    const now = new Date().toISOString();
    this.counter += 1;
    const title = this.counter === 1 ? "Untitled" : `Untitled ${this.counter}`;
    const note: NoteSummary = {
      id: `new-${this.counter}`,
      title,
      path: `Inbox/${title}.md`,
      folder: "Inbox",
      tags: [],
      favorite: false,
      preview: "",
      createdAt: now,
      updatedAt: now,
      lastOpenedAt: now,
    };
    this.notes.push(note);
    return Promise.resolve({ ...note });
  }

  async openDailyNote(): Promise<NoteSummary> {
    const title = localDateString();
    const existing = this.notes.find((x) => x.folder === "Journal" && x.title === title);
    if (existing) return { ...existing };
    const now = new Date().toISOString();
    const note: NoteSummary = {
      id: `daily-${title}`,
      title,
      path: `Journal/${title}.md`,
      folder: "Journal",
      tags: ["personal/journal"],
      favorite: false,
      preview: "",
      createdAt: now,
      updatedAt: now,
      lastOpenedAt: now,
    };
    this.notes.push(note);
    return { ...note };
  }

  markOpened(id: string): Promise<void> {
    const note = this.notes.find((x) => x.id === id);
    if (note) note.lastOpenedAt = new Date().toISOString();
    return Promise.resolve();
  }
}
