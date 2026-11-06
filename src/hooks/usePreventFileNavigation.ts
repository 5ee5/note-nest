import { useEffect } from "react";

/**
 * Dropping a file anywhere outside the editor would make the webview navigate to it
 * (replacing the app). Cancel those drops; the editor handles its own.
 */
export function usePreventFileNavigation(): void {
  useEffect(() => {
    const hasFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes("Files");
    const prevent = (e: DragEvent) => {
      if (hasFiles(e)) e.preventDefault();
    };
    window.addEventListener("dragover", prevent);
    window.addEventListener("drop", prevent);
    return () => {
      window.removeEventListener("dragover", prevent);
      window.removeEventListener("drop", prevent);
    };
  }, []);
}
