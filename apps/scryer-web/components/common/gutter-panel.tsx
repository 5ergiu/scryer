import * as React from "react";
import { ChevronDown } from "lucide-react";

import { SCROLL_FADE_CONTROL_CLASS } from "@/components/common/horizontal-scroll-fade";
import { Button } from "@/components/ui/button";
import { GUTTER_PANEL_TAB_WIDTH, type GutterPanelFrame } from "@/lib/hooks/use-gutter-panel";
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
 * open. While closed, a vertical tab on the same edge opens it, unless the
 * page is too narrow to hold the tab beside its content; then the page shows
 * a `GutterPanelButton` instead. The panel stays mounted so content can be
 * portaled into it.
 */
export function GutterPanel({ id, label, icon, frame, open, onOpenChange, children }: GutterPanelProps) {
  const close = React.useCallback(() => onOpenChange(false), [onOpenChange]);
  const context = React.useMemo(() => ({ close }), [close]);
  const scrollRef = React.useRef<HTMLDivElement>(null);
  const [hasMoreBelow, setHasMoreBelow] = React.useState(false);

  const measureOverflow = React.useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    setHasMoreBelow(element.scrollHeight - element.clientHeight - element.scrollTop > 1);
  }, []);

  React.useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    measureOverflow();
    if (typeof ResizeObserver === "undefined") return;
    // Content portaled in from elsewhere grows its slot, so watch the
    // children as well as the panel itself.
    const resizeObserver = new ResizeObserver(measureOverflow);
    resizeObserver.observe(element);
    for (const child of Array.from(element.children)) resizeObserver.observe(child);
    return () => resizeObserver.disconnect();
  }, [children, measureOverflow]);

  const scrollByPage = React.useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    element.scrollBy({ top: Math.max(element.clientHeight * 0.8, 240), behavior: "smooth" });
  }, []);

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
      {frame.tabFits ? (
        <button
          type="button"
          aria-controls={id}
          aria-expanded={false}
          onClick={() => onOpenChange(true)}
          style={{
            top: `calc(${frame.top}px + (100dvh - ${frame.top}px) / 2)`,
            right: frame.right,
            width: GUTTER_PANEL_TAB_WIDTH,
          }}
          className={cn(
            "fixed z-30 flex -translate-y-1/2 items-center gap-2 rounded-l-[10px] border border-r-0 border-[var(--scry-border)] bg-[var(--scry-bg)] py-3 text-[12.5px] font-semibold text-[var(--scry-ink2)] shadow-[-6px_0_18px_rgba(0,0,0,0.22)] transition-[opacity,visibility,background-color] duration-300 [writing-mode:vertical-rl] hover:bg-[var(--scry-hover)]",
            open ? "invisible opacity-0" : "visible opacity-100",
          )}
        >
          {icon}
          {label}
        </button>
      ) : null}
      <aside
        id={id}
        aria-label={label}
        style={{ top: frame.top, right: frame.right, width: frame.width }}
        className={cn(
          "fixed bottom-0 z-40 flex flex-col border-l border-[var(--scry-border)] bg-[var(--scry-bg)] text-[var(--scry-ink2)] transition-[translate,opacity,visibility] duration-300 ease-in-out",
          // The panel sits on top of the page: a soft shadow when it only
          // takes gutter space, a deeper one when it covers the content.
          frame.docked
            ? "shadow-[-12px_0_32px_rgba(0,0,0,0.28)]"
            : "shadow-[-20px_0_50px_rgba(0,0,0,0.42)]",
          open
            ? "visible translate-x-0 opacity-100"
            : "pointer-events-none invisible translate-x-full opacity-0",
        )}
      >
        <div
          ref={scrollRef}
          onScroll={measureOverflow}
          className="scry-scroll min-h-0 flex-1 overflow-y-auto"
        >
          <GutterPanelContext.Provider value={context}>{children}</GutterPanelContext.Provider>
        </div>
        {hasMoreBelow ? (
          <>
            <div
              aria-hidden="true"
              className="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-14 bg-gradient-to-b from-transparent to-[var(--scry-bg)]"
            />
            <button
              type="button"
              aria-label={`${label}: scroll down`}
              onClick={scrollByPage}
              className={cn(SCROLL_FADE_CONTROL_CLASS, "bottom-3 left-1/2 -translate-x-1/2")}
            >
              <ChevronDown className="size-7 stroke-[3]" aria-hidden="true" />
            </button>
          </>
        ) : null}
      </aside>
    </>
  );
}

type GutterPanelButtonProps = {
  panelId: string;
  label: string;
  icon: React.ReactNode;
  frame: GutterPanelFrame;
  open: boolean;
  onOpen: () => void;
};

/** Opens a closed panel from the page header when its edge tab does not fit. */
export function GutterPanelButton({ panelId, label, icon, frame, open, onOpen }: GutterPanelButtonProps) {
  if (open || frame.tabFits) return null;
  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      className="shrink-0 self-start sm:self-auto"
      aria-controls={panelId}
      aria-expanded={false}
      onClick={onOpen}
    >
      {icon}
      {label}
    </Button>
  );
}
