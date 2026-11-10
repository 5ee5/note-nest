/**
 * Native file and folder dialogs (desktop only). Kept in one module so tests can
 * replace them.
 */
import { open, save } from "@tauri-apps/plugin-dialog";

const MARKDOWN = [{ name: "Markdown", extensions: ["md", "MD"] }];

/** Markdown files to import; empty if the user cancelled. */
export async function pickMarkdownFiles(): Promise<string[]> {
  const picked = await open({
    multiple: true,
    directory: false,
    title: "Import Markdown files",
    filters: MARKDOWN,
  });
  if (picked === null) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/** A folder; null if the user cancelled. */
export async function pickFolder(title: string): Promise<string | null> {
  const picked = await open({ multiple: false, directory: true, title });
  return typeof picked === "string" ? picked : null;
}

/** Where to save a Markdown file; null if the user cancelled. */
export async function pickSaveMarkdown(defaultName: string): Promise<string | null> {
  return save({ title: "Export note", defaultPath: defaultName, filters: MARKDOWN });
}
