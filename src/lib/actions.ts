import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

/** Actions shared by keyboard shortcuts, the command palette and the UI. */

export async function createNote(): Promise<void> {
  const note = await useNotes.getState().createNote();
  if (note) {
    useUi.getState().setView({ kind: "all" });
    useUi.getState().selectNote(note.id);
  }
}

export async function openDailyNote(): Promise<void> {
  const note = await useNotes.getState().openDailyNote();
  if (note) {
    useUi.getState().setView({ kind: "daily" });
    useUi.getState().selectNote(note.id);
  }
}

export async function openNote(id: string): Promise<void> {
  useUi.getState().selectNote(id);
  await useNotes.getState().markOpened(id);
}
