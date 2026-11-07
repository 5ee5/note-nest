import { useEffect, useRef } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { useUi } from "../store/ui";

const SHOWN = 8;
/** Keyboard-synthesized clicks this soon after opening are almost certainly a lingering Enter key. */
const KEY_GUARD_MS = 400;

/**
 * Asks whether links in other notes should be rewritten because a rename or move would
 * break them. Closing the dialog (Escape, the backdrop) cancels the whole operation.
 */
export function LinkUpdateDialog() {
  const request = useUi((s) => s.linkUpdate);
  const total = request?.affected.reduce((n, a) => n + a.links, 0) ?? 0;
  const openedAt = useRef(0);
  useEffect(() => {
    if (request) openedAt.current = performance.now();
  }, [request]);

  /**
   * The dialog usually opens because the user pressed Enter in a text field. That key is
   * still down when the dialog appears, and its release must not "click" a button.
   */
  const choose = (choice: "update" | "keep" | "cancel") => (e: React.MouseEvent) => {
    if (e.detail === 0 && performance.now() - openedAt.current < KEY_GUARD_MS) return;
    request?.resolve(choice);
  };

  return (
    <Dialog.Root
      open={request !== null}
      onOpenChange={(open) => !open && request?.resolve("cancel")}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-overlay" />
        <Dialog.Content
          onOpenAutoFocus={(e) => {
            // Focus the dialog itself, not a button, so a lingering key press activates nothing.
            e.preventDefault();
            (e.currentTarget as HTMLElement).focus();
          }}
          className="fixed top-1/2 left-1/2 z-50 w-[30rem] max-w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-bg p-5 shadow-xl"
        >
          <Dialog.Title className="text-base font-semibold">{request?.heading}</Dialog.Title>
          <Dialog.Description className="mt-2 text-sm text-fg-muted">
            {total} link{total === 1 ? "" : "s"} in {request?.affected.length} other note
            {request?.affected.length === 1 ? "" : "s"} would stop working. NoteNest can update them
            for you; the original of every changed note is backed up first.
          </Dialog.Description>
          <ul
            aria-label="Notes with links to update"
            className="mt-3 max-h-48 space-y-1 overflow-y-auto text-sm"
          >
            {request?.affected.slice(0, SHOWN).map((a) => (
              <li
                key={a.note.id}
                className="flex justify-between gap-3 rounded-md bg-surface px-2 py-1"
              >
                <span className="truncate">{a.note.title}</span>
                <span className="shrink-0 text-xs text-fg-muted">
                  {a.links} link{a.links === 1 ? "" : "s"}
                </span>
              </li>
            ))}
            {request && request.affected.length > SHOWN && (
              <li className="px-2 text-xs text-fg-muted">
                and {request.affected.length - SHOWN} more
              </li>
            )}
          </ul>
          <div className="mt-5 flex justify-end gap-2">
            <button
              type="button"
              onClick={choose("cancel")}
              className="rounded-md px-3 py-1.5 text-sm hover:bg-surface-hover"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={choose("keep")}
              className="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-hover"
            >
              Rename only
            </button>
            <button
              type="button"
              onClick={choose("update")}
              className="rounded-md bg-accent px-3 py-1.5 text-sm text-accent-fg"
            >
              Update links
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
