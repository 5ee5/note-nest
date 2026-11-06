import { useEffect } from "react";
import { useVault } from "../store/vault";

/** Shows the app once a vault is open; otherwise lets the user choose where their notes live. */
export function VaultGate({ children }: { children: React.ReactNode }) {
  const phase = useVault((s) => s.phase);
  const error = useVault((s) => s.error);
  const defaultPath = useVault((s) => s.defaultPath);
  const initialize = useVault((s) => s.initialize);
  const openAt = useVault((s) => s.openAt);
  const chooseFolder = useVault((s) => s.chooseFolder);

  useEffect(() => {
    void initialize();
  }, [initialize]);

  if (phase === "ready") return <>{children}</>;

  if (phase === "checking" || phase === "opening") {
    return (
      <div className="grid h-full place-items-center text-sm text-fg-muted" role="status">
        Opening your vault…
      </div>
    );
  }

  return (
    <main className="grid h-full place-items-center p-6">
      <div className="w-full max-w-md">
        <h1 className="text-xl font-semibold">Welcome to NoteNest</h1>
        <p className="mt-2 text-sm text-fg-muted">
          Your notes are plain Markdown files in a folder you choose. Nothing leaves your computer.
        </p>
        {error && (
          <p role="alert" className="mt-4 rounded-md border border-ring px-3 py-2 text-sm">
            {error}
          </p>
        )}
        <div className="mt-6 flex flex-col gap-2">
          {defaultPath && (
            <button
              type="button"
              onClick={() => void openAt(defaultPath, true)}
              className="rounded-md bg-accent px-4 py-2 text-left text-sm text-accent-fg"
            >
              Use the default location
              <span className="block text-xs opacity-80">{defaultPath}</span>
            </button>
          )}
          <button
            type="button"
            onClick={() => void chooseFolder()}
            className="rounded-md border border-border px-4 py-2 text-left text-sm hover:bg-surface-hover"
          >
            Choose an existing folder…
          </button>
        </div>
      </div>
    </main>
  );
}
