import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { discardDraft, restoreDraft } from "../lib/actions";
import { formatDate } from "../lib/format";
import { useRecovery } from "../store/recovery";

/** Offers back text that was typed but never saved before an unexpected exit. */
export function RecoveryDialog() {
  const drafts = useRecovery((s) => s.drafts);
  const [dismissed, setDismissed] = useState(false);
  const open = drafts.length > 0 && !dismissed;

  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && setDismissed(true)}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-overlay" />
        <Dialog.Content className="fixed top-1/2 left-1/2 z-50 max-h-[80vh] w-[34rem] max-w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-lg border border-border bg-bg p-5 shadow-xl">
          <Dialog.Title className="text-base font-semibold">Recover unsaved changes</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-fg-muted">
            NoteNest closed before these edits were saved. Nothing is lost until you discard it.
          </Dialog.Description>
          <ul className="mt-4 space-y-3">
            {drafts.map((d) => (
              <li key={d.noteId} className="rounded-md border border-border p-3">
                <div className="flex items-baseline justify-between gap-2">
                  <span className="truncate text-sm font-medium">{d.path}</span>
                  <span className="shrink-0 text-xs text-fg-muted">{formatDate(d.stashedAt)}</span>
                </div>
                {d.noteMissing && (
                  <p className="mt-1 text-xs text-fg-muted">
                    This note no longer exists. Restoring creates a new note with this text.
                  </p>
                )}
                {d.diskChanged && !d.noteMissing && (
                  <p className="mt-1 text-xs text-fg-muted">
                    The file changed after this draft was started. Restoring replaces it and keeps a
                    backup of the current version.
                  </p>
                )}
                <pre className="mt-2 max-h-24 overflow-hidden rounded bg-surface p-2 text-xs whitespace-pre-wrap">
                  {d.content.slice(0, 300)}
                </pre>
                <div className="mt-2 flex justify-end gap-2">
                  <button
                    type="button"
                    onClick={() => void discardDraft(d)}
                    className="rounded-md px-3 py-1 text-sm hover:bg-surface-hover"
                  >
                    Discard
                  </button>
                  <button
                    type="button"
                    onClick={() => void restoreDraft(d)}
                    className="rounded-md bg-accent px-3 py-1 text-sm text-accent-fg"
                  >
                    Restore
                  </button>
                </div>
              </li>
            ))}
          </ul>
          <div className="mt-4 text-right">
            <Dialog.Close className="rounded-md px-3 py-1.5 text-sm hover:bg-surface-hover">
              Decide later
            </Dialog.Close>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
