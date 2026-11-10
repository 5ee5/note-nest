/**
 * Keeps the UI in line with changes made to the vault outside NoteNest.
 *
 * The backend reconciles its index whenever its file watcher sees changes and then
 * reports what changed (`NotesApi.onVaultChanged`). Because OS file events can be
 * missed, the app also asks for a reconciliation whenever its window regains focus.
 * Changes are applied one at a time, in order.
 */
import { useEffect } from "react";
import type { VaultChange } from "./api";
import { openNote } from "./actions";
import { useEditor } from "../store/editor";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

function isEmpty(c: VaultChange): boolean {
  return c.changed.length === 0 && c.removed.length === 0 && !c.folders && !c.files;
}

export async function applyVaultChange(change: VaultChange): Promise<void> {
  if (isEmpty(change)) return;
  const before = new Map(useNotes.getState().notes.map((n) => [n.id, n]));
  const openId = useEditor.getState().noteId;
  const outcome = await useEditor.getState().externalChange(change);
  await useNotes.getState().load();
  if (change.files) useNotes.getState().filesChanged();
  if (!openId) return;

  const title = before.get(openId)?.title ?? "The open note";
  if (outcome === "closed") {
    // A note renamed outside NoteNest without an id in its frontmatter comes back under
    // a new id. If exactly one note vanished and one appeared, follow it.
    const added = change.changed.filter((id) => !before.has(id));
    if (change.removed.length === 1 && added.length === 1) {
      await openNote(added[0]);
      useUi.getState().notify("info", `“${title}” was renamed or moved outside NoteNest.`);
    } else {
      useUi.getState().selectNote(null);
      useUi.getState().notify("info", `“${title}” was deleted or moved outside NoteNest.`);
    }
  } else if (outcome === "conflict") {
    useUi
      .getState()
      .notify("warning", `“${title}” was changed outside NoteNest while you were editing it.`);
  }
}

let queue: Promise<void> = Promise.resolve();

/** Applies `change` after any change still being applied. */
export function enqueueVaultChange(change: VaultChange): Promise<void> {
  queue = queue
    .then(() => applyVaultChange(change))
    .catch((e) => console.warn("could not apply outside changes", e));
  return queue;
}

/** Subscribes to outside changes for as long as the vault is open. */
export function useVaultSync(): void {
  const api = useNotes((s) => s.api);
  useEffect(() => {
    const stop = api.onVaultChanged((change) => void enqueueVaultChange(change));
    const onFocus = () => {
      api
        .reconcile()
        .then((change) => enqueueVaultChange(change))
        .catch((e) => console.warn("could not check the vault for outside changes", e));
    };
    window.addEventListener("focus", onFocus);
    return () => {
      stop();
      window.removeEventListener("focus", onFocus);
    };
  }, [api]);
}
