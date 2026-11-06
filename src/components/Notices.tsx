import { useEffect } from "react";
import { X } from "lucide-react";
import { cn } from "../lib/cn";
import { useUi, type Notice } from "../store/ui";

const AUTO_DISMISS_MS = 8000;

function NoticeItem({ notice }: { notice: Notice }) {
  const dismiss = useUi((s) => s.dismissNotice);
  useEffect(() => {
    if (notice.kind === "error") return; // errors stay until dismissed
    const t = setTimeout(() => dismiss(notice.id), AUTO_DISMISS_MS);
    return () => clearTimeout(t);
  }, [notice.id, notice.kind, dismiss]);

  return (
    <li
      role={notice.kind === "error" ? "alert" : "status"}
      className={cn(
        "flex items-start gap-2 rounded-md border bg-bg px-3 py-2 text-sm shadow-lg",
        notice.kind === "error" ? "border-ring" : "border-border",
      )}
    >
      <span className="min-w-0 flex-1 break-words">{notice.message}</span>
      <button
        type="button"
        aria-label="Dismiss"
        onClick={() => dismiss(notice.id)}
        className="rounded p-0.5 text-fg-muted hover:bg-surface-hover"
      >
        <X aria-hidden className="size-3.5" />
      </button>
    </li>
  );
}

export function Notices() {
  const notices = useUi((s) => s.notices);
  return (
    <ul
      aria-label="Notifications"
      className="pointer-events-none fixed right-4 bottom-4 z-[60] flex w-96 max-w-[calc(100vw-2rem)] flex-col gap-2 [&>li]:pointer-events-auto"
    >
      {notices.map((n) => (
        <NoticeItem key={n.id} notice={n} />
      ))}
    </ul>
  );
}
