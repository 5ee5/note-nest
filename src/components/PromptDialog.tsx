import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { useUi, type PromptRequest } from "../store/ui";

function PromptForm({ prompt }: { prompt: PromptRequest }) {
  const close = useUi((s) => s.closePrompt);
  const [value, setValue] = useState(prompt.initial ?? "");
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = value.trim();
    if (trimmed === "" || busy) return;
    setBusy(true);
    try {
      await prompt.onSubmit(trimmed);
    } finally {
      setBusy(false);
      close();
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)}>
      <Dialog.Title className="text-base font-semibold">{prompt.title}</Dialog.Title>
      <Dialog.Description className="sr-only">{prompt.label}</Dialog.Description>
      <label className="mt-4 block text-sm">
        {prompt.label}
        <input
          autoFocus
          value={value}
          onChange={(e) => setValue(e.target.value)}
          className="mt-1 w-full rounded-md border border-border bg-surface px-2 py-1.5 text-sm"
        />
      </label>
      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={close}
          className="rounded-md px-3 py-1.5 text-sm hover:bg-surface-hover"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={busy || value.trim() === ""}
          className="rounded-md bg-accent px-3 py-1.5 text-sm text-accent-fg disabled:opacity-50"
        >
          {prompt.confirmLabel ?? "OK"}
        </button>
      </div>
    </form>
  );
}

export function PromptDialog() {
  const prompt = useUi((s) => s.prompt);
  const close = useUi((s) => s.closePrompt);
  return (
    <Dialog.Root open={prompt !== null} onOpenChange={(open) => !open && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-overlay" />
        <Dialog.Content className="fixed top-1/2 left-1/2 z-50 w-96 max-w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-bg p-5 shadow-xl">
          {prompt && <PromptForm key={prompt.title + (prompt.initial ?? "")} prompt={prompt} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
