import { useState } from "react";

import { cn } from "../../../shared/lib/cn";

const STORAGE_KEY = "agentx.workflowStudio.inspectorWidth";
const MIN_WIDTH = 420;
const MAX_WIDTH = 640;

export function InspectorShell({ children, testId, className }: { children: React.ReactNode; testId: string; className?: string }) {
  const [width, setWidth] = useState(defaultWidth);
  const beginResize = (event: React.PointerEvent<HTMLButtonElement>) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = width;
    const move = (next: PointerEvent) => setWidth(clamp(startWidth + startX - next.clientX));
    const finish = (next: PointerEvent) => {
      const resolved = clamp(startWidth + startX - next.clientX);
      setWidth(resolved);
      localStorage.setItem(STORAGE_KEY, String(resolved));
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish);
  };
  return <aside className={cn("studio-inspector relative flex h-full shrink-0 flex-col overflow-hidden border-l border-border bg-surface", className)} data-testid={testId} style={{ width }}>
    <button aria-label="Resize inspector" className="absolute left-0 top-1/2 z-[120] h-16 w-2 -translate-x-1/2 -translate-y-1/2 cursor-col-resize rounded-full bg-border/70 hover:bg-primary" onDoubleClick={() => setWidth(defaultResponsiveWidth())} onPointerDown={beginResize} type="button" />
    {children}
  </aside>;
}

function defaultWidth() {
  const stored = Number(localStorage.getItem(STORAGE_KEY));
  return Number.isFinite(stored) && stored >= MIN_WIDTH && stored <= MAX_WIDTH ? stored : defaultResponsiveWidth();
}

function defaultResponsiveWidth() {
  return window.innerWidth >= 1440 ? 480 : 440;
}

function clamp(width: number) {
  return Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, Math.round(width)));
}
