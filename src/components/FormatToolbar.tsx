import { useRef, useState } from "react";
import {
  Bold,
  Code,
  Heading1,
  Heading2,
  Heading3,
  ImagePlus,
  Italic,
  Link,
  List,
  ListChecks,
  ListOrdered,
  Minus,
  Quote,
  Redo2,
  Search,
  SquareCode,
  Strikethrough,
  Table,
  Undo2,
  type LucideIcon,
} from "lucide-react";
import { openSearchPanel } from "@codemirror/search";
import type { StateCommand } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import {
  insertCodeBlock,
  insertHorizontalRule,
  insertLink,
  insertTable,
  redo,
  runCommand,
  setHeading,
  toggleBold,
  toggleBulletList,
  toggleChecklist,
  toggleInlineCode,
  toggleItalic,
  toggleNumberedList,
  toggleQuote,
  toggleStrike,
  undo,
  type ActiveFormats,
} from "./editor/format";
import { modLabel } from "../lib/shortcuts";
import { cn } from "../lib/cn";
import { useEditorView } from "../store/editorView";

interface Item {
  id: string;
  label: string;
  icon: LucideIcon;
  run: (view: EditorView) => void;
  pressed?: (f: ActiveFormats) => boolean;
  disabled?: (s: { canUndo: boolean; canRedo: boolean }) => boolean;
}

const cmd =
  (c: StateCommand) =>
  (view: EditorView): void => {
    runCommand(view, c);
  };

function buildItems(mod: string, onInsertImage?: () => void): Item[][] {
  return [
    [
      {
        id: "undo",
        label: `Undo (${mod}+Z)`,
        icon: Undo2,
        run: cmd(undo),
        disabled: (s) => !s.canUndo,
      },
      {
        id: "redo",
        label: `Redo (${mod}+Shift+Z)`,
        icon: Redo2,
        run: cmd(redo),
        disabled: (s) => !s.canRedo,
      },
    ],
    [
      {
        id: "bold",
        label: `Bold (${mod}+B)`,
        icon: Bold,
        run: cmd(toggleBold),
        pressed: (f) => f.bold,
      },
      {
        id: "italic",
        label: `Italic (${mod}+I)`,
        icon: Italic,
        run: cmd(toggleItalic),
        pressed: (f) => f.italic,
      },
      {
        id: "strike",
        label: `Strikethrough (${mod}+Shift+X)`,
        icon: Strikethrough,
        run: cmd(toggleStrike),
        pressed: (f) => f.strike,
      },
      {
        id: "code",
        label: `Inline code (${mod}+E)`,
        icon: Code,
        run: cmd(toggleInlineCode),
        pressed: (f) => f.code,
      },
    ],
    [
      {
        id: "h1",
        label: "Heading 1",
        icon: Heading1,
        run: cmd(setHeading(1)),
        pressed: (f) => f.heading === 1,
      },
      {
        id: "h2",
        label: "Heading 2",
        icon: Heading2,
        run: cmd(setHeading(2)),
        pressed: (f) => f.heading === 2,
      },
      {
        id: "h3",
        label: "Heading 3",
        icon: Heading3,
        run: cmd(setHeading(3)),
        pressed: (f) => f.heading === 3,
      },
    ],
    [
      {
        id: "bullets",
        label: `Bulleted list (${mod}+Shift+8)`,
        icon: List,
        run: cmd(toggleBulletList),
        pressed: (f) => f.list === "bullet",
      },
      {
        id: "numbers",
        label: `Numbered list (${mod}+Shift+7)`,
        icon: ListOrdered,
        run: cmd(toggleNumberedList),
        pressed: (f) => f.list === "number",
      },
      {
        id: "tasks",
        label: `Checklist (${mod}+Shift+9)`,
        icon: ListChecks,
        run: cmd(toggleChecklist),
        pressed: (f) => f.list === "check",
      },
      {
        id: "quote",
        label: "Quote",
        icon: Quote,
        run: cmd(toggleQuote),
        pressed: (f) => f.list === "quote",
      },
    ],
    [
      { id: "link", label: "Link", icon: Link, run: cmd(insertLink) },
      ...(onInsertImage
        ? [{ id: "image", label: "Insert image", icon: ImagePlus, run: () => onInsertImage() }]
        : []),
      { id: "table", label: "Table", icon: Table, run: cmd(insertTable) },
      { id: "codeblock", label: "Code block", icon: SquareCode, run: cmd(insertCodeBlock) },
      { id: "rule", label: "Horizontal rule", icon: Minus, run: cmd(insertHorizontalRule) },
      {
        id: "find",
        label: `Find and replace (${mod}+F)`,
        icon: Search,
        run: (v) => void openSearchPanel(v),
      },
    ],
  ];
}

/**
 * Formatting toolbar. One button is in the tab order at a time; arrow keys, Home and
 * End move between buttons (WAI-ARIA toolbar pattern). Buttons never take focus from
 * the editor on mouse down, so the selection they act on stays put.
 */
export default function FormatToolbar({
  disabled = false,
  onInsertImage,
}: {
  disabled?: boolean;
  onInsertImage?: () => void;
}) {
  const view = useEditorView((s) => s.view);
  const formats = useEditorView((s) => s.formats);
  const canUndo = useEditorView((s) => s.canUndo);
  const canRedo = useEditorView((s) => s.canRedo);
  const groups = buildItems(modLabel(), onInsertImage);
  const flat = groups.flat();
  const [active, setActive] = useState(0);
  const refs = useRef<(HTMLButtonElement | null)[]>([]);

  const isDisabled = (item: Item) =>
    disabled || !view || (item.disabled?.({ canUndo, canRedo }) ?? false);

  const move = (from: number, step: 1 | -1 | "first" | "last") => {
    const n = flat.length;
    const order =
      step === "first"
        ? [...Array(n).keys()]
        : step === "last"
          ? [...Array(n).keys()].reverse()
          : Array.from({ length: n - 1 }, (_, i) => (from + (i + 1) * step + n * n) % n);
    const target = order.find((i) => !isDisabled(flat[i]!));
    if (target !== undefined) {
      setActive(target);
      refs.current[target]?.focus();
    }
  };

  const onKeyDown = (e: React.KeyboardEvent, index: number) => {
    const key = { ArrowRight: 1, ArrowLeft: -1, Home: "first", End: "last" }[e.key] as
      1 | -1 | "first" | "last" | undefined;
    if (key === undefined) return;
    e.preventDefault();
    move(index, key);
  };

  const offsets = groups.map((_, gi) => groups.slice(0, gi).reduce((n, g) => n + g.length, 0));
  return (
    <div
      role="toolbar"
      aria-label="Formatting"
      aria-orientation="horizontal"
      className="flex flex-wrap items-center gap-0.5 border-b border-border px-6 py-1"
    >
      {groups.map((group, gi) => (
        <div key={gi} className="flex items-center gap-0.5">
          {gi > 0 && <span aria-hidden className="mx-1 h-4 w-px bg-border" />}
          {group.map((item, ii) => {
            const i = offsets[gi]! + ii;
            const Icon = item.icon;
            const pressed = item.pressed?.(formats);
            return (
              <button
                key={item.id}
                ref={(el) => {
                  refs.current[i] = el;
                }}
                type="button"
                aria-label={item.label}
                title={item.label}
                aria-pressed={pressed}
                disabled={isDisabled(item)}
                tabIndex={i === active ? 0 : -1}
                onFocus={() => setActive(i)}
                onKeyDown={(e) => onKeyDown(e, i)}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  if (!view) return;
                  item.run(view);
                  view.focus();
                }}
                className={cn(
                  "rounded-md p-1.5 text-fg-muted hover:bg-surface-hover disabled:opacity-40 disabled:hover:bg-transparent",
                  pressed && "bg-surface-active text-fg",
                )}
              >
                <Icon aria-hidden className="size-4" />
              </button>
            );
          })}
        </div>
      ))}
    </div>
  );
}
