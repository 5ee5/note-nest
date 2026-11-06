import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { THEMES, type Theme } from "../lib/theme";
import { modLabel } from "../lib/shortcuts";
import { useUi } from "../store/ui";

const THEME_LABELS: Record<Theme, string> = {
  light: "Light",
  dark: "Dark",
  system: "System default",
};

export function SettingsDialog() {
  const open = useUi((s) => s.settingsOpen);
  const setOpen = useUi((s) => s.setSettingsOpen);
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
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
            Preferences are stored on this device only.
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
                    onChange={() => setTheme(t)}
                    className="accent-accent"
                  />
                  {THEME_LABELS[t]}
                </label>
              ))}
            </div>
          </fieldset>

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
