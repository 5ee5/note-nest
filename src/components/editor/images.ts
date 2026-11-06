import { EditorView } from "@codemirror/view";
import { isImageFile } from "../../lib/images";
import { storeImageFiles } from "../../lib/actions";
import { useEditor } from "../../store/editor";
import { useUi } from "../../store/ui";
import { insertText, runCommand } from "./format";

/**
 * Stores the images as attachments of the open note and inserts their Markdown at the
 * cursor (or at `pos`). If the user switched notes while the files were being stored,
 * nothing is inserted (the relative paths would be wrong for another note).
 */
export async function insertImages(view: EditorView, files: File[], pos?: number): Promise<void> {
  const noteId = useEditor.getState().noteId;
  const text = await storeImageFiles(files);
  if (!text) return;
  if (useEditor.getState().noteId !== noteId || !view.dom.isConnected) {
    useUi
      .getState()
      .notify(
        "warning",
        "The image was saved to Attachments, but the note changed before it could be inserted.",
      );
    return;
  }
  if (pos !== undefined)
    view.dispatch({ selection: { anchor: Math.min(pos, view.state.doc.length) } });
  runCommand(view, insertText(text));
  view.focus();
}

const hasFiles = (dt: DataTransfer | null) => !!dt && Array.from(dt.types).includes("Files");

/** Paste and drop handlers: images become attachments, other text/paste behavior is untouched. */
export const imageHandlers = EditorView.domEventHandlers({
  paste(event, view) {
    const files = Array.from(event.clipboardData?.files ?? []).filter(isImageFile);
    if (files.length === 0) return false;
    event.preventDefault();
    void insertImages(view, files);
    return true;
  },
  dragover(event) {
    if (hasFiles(event.dataTransfer)) event.preventDefault(); // allow the drop
    return false;
  },
  drop(event, view) {
    if (!hasFiles(event.dataTransfer)) return false;
    // Never let the webview open a dropped file; non-images are reported by the store.
    event.preventDefault();
    const files = Array.from(event.dataTransfer?.files ?? []);
    const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
    void insertImages(view, files, pos ?? undefined);
    return true;
  },
});
