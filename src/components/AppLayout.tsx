import { useEffect } from "react";
import { Group, Panel, Separator, useDefaultLayout, usePanelRef } from "react-resizable-panels";
import { safeStorage } from "../lib/safeStorage";
import { useUi } from "../store/ui";
import { Sidebar } from "./Sidebar";
import { NoteExplorer } from "./NoteExplorer";
import { EditorPane } from "./EditorPane";

function ResizeHandle({ label }: { label: string }) {
  return (
    <Separator
      aria-label={label}
      className="w-px bg-border transition-colors hover:bg-ring data-[separator=active]:bg-ring"
    />
  );
}

export function AppLayout() {
  const sidebarRef = usePanelRef();
  const explorerRef = usePanelRef();
  const sidebarCollapsed = useUi((s) => s.sidebarCollapsed);
  const explorerCollapsed = useUi((s) => s.explorerCollapsed);
  const setSidebarCollapsed = useUi((s) => s.setSidebarCollapsed);
  const setExplorerCollapsed = useUi((s) => s.setExplorerCollapsed);

  const { defaultLayout, onLayoutChanged } = useDefaultLayout({
    id: "notenest-layout",
    storage: safeStorage,
  });

  // Store -> panel: collapse/expand when a command or toolbar button flips the flag.
  useEffect(() => {
    const p = sidebarRef.current;
    if (!p) return;
    if (sidebarCollapsed && !p.isCollapsed()) p.collapse();
    if (!sidebarCollapsed && p.isCollapsed()) p.expand();
  }, [sidebarCollapsed, sidebarRef]);

  useEffect(() => {
    const p = explorerRef.current;
    if (!p) return;
    if (explorerCollapsed && !p.isCollapsed()) p.collapse();
    if (!explorerCollapsed && p.isCollapsed()) p.expand();
  }, [explorerCollapsed, explorerRef]);

  return (
    <Group
      orientation="horizontal"
      defaultLayout={defaultLayout}
      onLayoutChanged={onLayoutChanged}
      className="h-full"
    >
      <Panel
        id="sidebar"
        panelRef={sidebarRef}
        defaultSize={240}
        minSize={180}
        maxSize={400}
        collapsible
        collapsedSize={0}
        onResize={(size) => setSidebarCollapsed(size.inPixels === 0)}
      >
        <Sidebar />
      </Panel>
      <ResizeHandle label="Resize sidebar" />
      <Panel
        id="explorer"
        panelRef={explorerRef}
        defaultSize={320}
        minSize={240}
        maxSize={560}
        collapsible
        collapsedSize={0}
        onResize={(size) => setExplorerCollapsed(size.inPixels === 0)}
      >
        <NoteExplorer />
      </Panel>
      <ResizeHandle label="Resize note explorer" />
      <Panel id="editor" minSize={320}>
        <EditorPane />
      </Panel>
    </Group>
  );
}
