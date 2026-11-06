import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export default function App() {
  const [info, setInfo] = useState<string>("NoteNest");

  useEffect(() => {
    if (!isTauri()) return;
    invoke<string>("app_info")
      .then(setInfo)
      .catch((e) => console.error("app_info failed", e));
  }, []);

  return (
    <div className="grid h-screen grid-cols-[14rem_18rem_1fr] bg-white text-neutral-900 dark:bg-neutral-900 dark:text-neutral-100">
      <aside
        aria-label="Sidebar"
        className="border-r border-neutral-200 p-3 dark:border-neutral-800"
      >
        <h1 className="text-sm font-semibold">NoteNest</h1>
      </aside>
      <section
        aria-label="Note list"
        className="border-r border-neutral-200 p-3 dark:border-neutral-800"
      >
        <p className="text-sm text-neutral-500">No notes yet.</p>
      </section>
      <main className="flex flex-col p-6">
        <p className="text-neutral-500">Editor placeholder</p>
        <footer className="mt-auto text-xs text-neutral-400">{info}</footer>
      </main>
    </div>
  );
}
