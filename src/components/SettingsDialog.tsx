import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { THEMES, type Theme } from "../lib/theme";
import { modLabel } from "../lib/shortcuts";
import { isTauri } from "../lib/api";
import {
  changeTheme,
  backUpVault,
  exportCurrentNote,
  exportVault,
  importMarkdownFiles,
  importMarkdownFolder,
} from "../lib/actions";
import { useUi } from "../store/ui";
import { useVault } from "../store/vault";

const THEME_LABELS: Record<Theme, string> = {
  light: "Light",
  dark: "Dark",
  system: "System default",
};

export function SettingsDialog() {
  const open = useUi((s) => s.settingsOpen);
  const setOpen = useUi((s) => s.setSettingsOpen);
  const theme = useUi((s) => s.theme);
  const vaultPath = useVault((s) => s.path);
  const lastBackup = useUi((s) => s.lastBackup);
  const exportStandard = useUi((s) => s.exportStandard);
  const setExportStandard = useUi((s) => s.setExportStandard);
  /** Closes the dialog (native pickers open on top of the window) and runs `fn`. */
  const close = (fn: () => Promise<void>) => () => {
    setOpen(false);
    void fn();
  };
  const buttonClass = "rounded-md border border-border px-2.5 py-1 text-xs hover:bg-surface-hover";
  const changeVault = useVault((s) => s.change);
  const mod = modLabel();

  const shortcuts: [string, string][] = [
    ["New note", `${mod}+N`],
    ["Search", `${mod}+P`],
    ["Command palette", `${mod}+K`],
  ];

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-overlay" />
        <Dialog.Content className="fixed top-1/2 left-1/2 z-50 w-[28rem] max-w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-bg p-5 shadow-xl">
          <div className="flex items-start justify-between">
            <Dialog.Title className="text-base font-semibold">Settings</Dialog.Title>
            <Dialog.Close aria-label="Close" className="rounded-md p-1 hover:bg-surface-hover">
              <X aria-hidden className="size-4" />
            </Dialog.Close>
          </div>
          <Dialog.Description className="mt-1 text-sm text-fg-muted">
            Your notes and preferences stay on this device.
          </Dialog.Description>

          <fieldset className="mt-4">
            <legend className="mb-2 text-sm font-medium">Theme</legend>
            <div className="space-y-1">
              {THEMES.map((t) => (
                <label key={t} className="flex items-center gap-2 text-sm">
                  <input
                    type="radio"
                    name="theme"
                    value={t}
                    checked={theme === t}
                    onChange={() => changeTheme(t)}
                    className="accent-accent"
                  />
                  {THEME_LABELS[t]}
                </label>
              ))}
            </div>
          </fieldset>

          {isTauri() && (
            <section className="mt-5" aria-label="Vault">
              <h3 className="mb-1 text-sm font-medium">Vault</h3>
              <p className="text-sm break-all text-fg-muted">{vaultPath}</p>
              <button
                type="button"
                onClick={() => {
                  setOpen(false);
                  void changeVault();
                }}
                className="mt-2 rounded-md border border-border px-2.5 py-1 text-xs hover:bg-surface-hover"
              >
                Change vault…
              </button>
            </section>
          )}

          {isTauri() && (
            <section className="mt-5" aria-label="Backup">
              <h3 className="mb-1 text-sm font-medium">Backup</h3>
              <p className="text-sm text-fg-muted">
                {lastBackup
                  ? `Last backup: ${new Date(lastBackup).toLocaleString()}`
                  : "This vault has not been backed up yet."}
              </p>
              <button type="button" onClick={close(backUpVault)} className={`mt-2 ${buttonClass}`}>
                Back up now…
              </button>
              <p className="mt-1 text-xs text-fg-muted">
                An exact, verified copy in a folder you choose. Keep it on another drive. Open it as
                a vault to restore.
              </p>
            </section>
          )}

          {isTauri() && (
            <section className="mt-5" aria-label="Import and export">
              <h3 className="mb-1 text-sm font-medium">Import and export</h3>
              <p className="text-sm text-fg-muted">
                Imports are copied into the folder you are viewing (or the vault root); nothing is
                overwritten.
              </p>
              <div className="mt-2 flex flex-wrap gap-2">
                <button type="button" onClick={close(importMarkdownFiles)} className={buttonClass}>
                  Import Markdown files…
                </button>
                <button type="button" onClick={close(importMarkdownFolder)} className={buttonClass}>
                  Import a folder…
                </button>
                <button type="button" onClick={close(exportCurrentNote)} className={buttonClass}>
                  Export this note…
                </button>
                <button type="button" onClick={close(exportVault)} className={buttonClass}>
                  Export the vault…
                </button>
              </div>
              <label className="mt-2 flex items-start gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={exportStandard}
                  onChange={(e) => setExportStandard(e.target.checked)}
                  className="mt-0.5 accent-accent"
                />
                <span>
                  Export as standard Markdown
                  <span className="block text-xs text-fg-muted">
                    Wiki links become regular links and NoteNest’s own properties (id, pinned,
                    archived) are left out.
                  </span>
                </span>
              </label>
            </section>
          )}

          <section className="mt-5" aria-label="Keyboard shortcuts">
            <h3 className="mb-2 text-sm font-medium">Keyboard shortcuts</h3>
            <dl className="space-y-1 text-sm">
              {shortcuts.map(([name, keys]) => (
                <div key={name} className="flex justify-between">
                  <dt className="text-fg-muted">{name}</dt>
                  <dd>
                    <kbd className="rounded border border-border bg-surface px-1.5 py-0.5 text-xs">
                      {keys}
                    </kbd>
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
