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
  /** Shown beside the label on the edge tab that opens the panel. */
  icon: React.ReactNode;
  frame: GutterPanelFrame;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: React.ReactNode;
};

/**
 * A full-height side panel pinned to the right edge of the page content. It
 * is not modal: there is no backdrop and the page stays usable while it is
 * open. While closed, a vertical tab on the same edge opens it, and it stays
 * mounted so content can be portaled into it.
 */
export function GutterPanel({ id, label, icon, frame, open, onOpenChange, children }: GutterPanelProps) {
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
    <>
      <button
        type="button"
        aria-controls={id}
        aria-expanded={false}
        onClick={() => onOpenChange(true)}
        style={{ top: `calc(${frame.top}px + (100dvh - ${frame.top}px) / 2)`, right: frame.right }}
        className={cn(
          "fixed z-30 flex -translate-y-1/2 items-center gap-2 rounded-l-[10px] border border-r-0 border-[var(--scry-border)] bg-[var(--scry-surf)] px-1.5 py-3 text-[12.5px] font-semibold text-[var(--scry-ink2)] shadow-[-6px_0_18px_rgba(0,0,0,0.22)] transition-[opacity,visibility,background-color] duration-300 [writing-mode:vertical-rl] hover:bg-[var(--scry-hover)]",
          open ? "invisible opacity-0" : "visible opacity-100",
        )}
      >
        {icon}
        {label}
      </button>
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
    </>
  );
}
