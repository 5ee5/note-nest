import type { LucideIcon } from "lucide-react";
import type { View } from "../lib/notes";
import { cn } from "../lib/cn";
import { useUi } from "../store/ui";

export function sameView(a: View, b: View): boolean {
  if (a.kind !== b.kind) return false;
  return "value" in a && "value" in b ? a.value === b.value : true;
}

/** A sidebar entry that switches the explorer to `view`. */
export function NavButton({
  icon: Icon,
  label,
  view,
  depth = 0,
  suffix,
}: {
  icon: LucideIcon;
  label: string;
  view: View;
  depth?: number;
  /** Small trailing text, e.g. a note count. */
  suffix?: string | number;
}) {
  const current = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const active = sameView(current, view);
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={() => setView(view)}
      style={{ paddingLeft: `${0.5 + depth * 0.9}rem` }}
      className={cn(
        "flex min-w-0 flex-1 items-center gap-2 rounded-md py-1.5 pr-2 text-left text-sm",
        active ? "bg-surface-active font-medium" : "text-fg-muted hover:bg-surface-hover",
      )}
    >
      <Icon aria-hidden className="size-4 shrink-0" />
      <span className="truncate">{label}</span>
      {suffix !== undefined && (
        <span className="ml-auto shrink-0 text-xs opacity-70">{suffix}</span>
      )}
    </button>
  );
}
