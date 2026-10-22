import * as React from "react";

import type { GutterPanelFrame } from "@/lib/hooks/use-gutter-panel";
import { cn } from "@/lib/utils";

/**
 * Lets content rendered into a gutter panel, including content portaled in
 * from elsewhere in the tree, drop its own card frame and offer the panel's
 * close button in its header.
 */
export const GutterPanelContext = React.createContext<{ close: () => void } | null>(null);

type GutterPanelProps = {
  id: string;
  label: string;
  frame: GutterPanelFrame;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: React.ReactNode;
};

/**
 * A full-height side panel pinned to the right edge of the page content. It
 * is not modal: there is no backdrop and the page stays usable while it is
 * open. It stays mounted while closed so content can be portaled into it.
 */
export function GutterPanel({ id, label, frame, open, onOpenChange, children }: GutterPanelProps) {
  const close = React.useCallback(() => onOpenChange(false), [onOpenChange]);
  const context = React.useMemo(() => ({ close }), [close]);

  React.useEffect(() => {
    // Docked, the panel covers nothing, so Escape is left to the page.
    if (!open || frame.docked) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [close, frame.docked, open]);

  return (
    <aside
      id={id}
      aria-label={label}
      style={{ top: frame.top, right: frame.right, width: frame.width }}
      className={cn(
        "scry-scroll fixed bottom-0 z-40 flex flex-col overflow-y-auto border-l border-[var(--scry-border)] bg-[var(--scry-surf)] text-[var(--scry-ink2)] transition-[translate,opacity,visibility] duration-300 ease-in-out",
        !frame.docked && "shadow-[-20px_0_50px_rgba(0,0,0,0.38)]",
        open
          ? "visible translate-x-0 opacity-100"
          : "pointer-events-none invisible translate-x-full opacity-0",
      )}
    >
      <GutterPanelContext.Provider value={context}>{children}</GutterPanelContext.Provider>
    </aside>
  );
}
