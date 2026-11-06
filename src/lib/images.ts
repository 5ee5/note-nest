/** Client-side checks for image files before they are sent to the backend. */

/** Same limit as the backend (crates/notenest-core/src/services/attachments.rs). */
export const MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;

const IMAGE_EXTENSION = /\.(png|jpe?g|gif|webp|svg)$/i;

/** A file that looks like an image (the backend still decides by content). */
export function isImageFile(file: { name: string; type: string }): boolean {
  return file.type.startsWith("image/") || IMAGE_EXTENSION.test(file.name);
}

/** Alt text for an inserted image: the file name without its extension, brackets removed. */
export function altText(fileName: string): string {
  const stem = fileName.replace(/\.[^./\\]*$/, "").replace(/[[\]\\]/g, "");
  return stem.trim() || "image";
}
