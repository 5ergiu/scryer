import * as React from "react";

export type GutterPanelSize = {
  /** Width of the panel, whether it sits in the gutter or over the content. */
  width: number;
  /** Space kept between the content's right edge and the panel. */
  gap: number;
};

/** Where the panel sits: fixed to the right edge of the scroll container. */
export type GutterPanelFrame = {
  /** The right gutter holds the panel without covering any content. */
  docked: boolean;
  width: number;
  top: number;
  right: number;
};

const UNDOCKED_FRAME: GutterPanelFrame = {
  docked: false,
  width: 0,
  top: 0,
  right: 0,
};

function sameFrame(a: GutterPanelFrame, b: GutterPanelFrame) {
  return a.docked === b.docked && a.width === b.width && a.top === b.top && a.right === b.right;
}

/**
 * A side panel pinned to the right edge of a scroll container beside centered
 * content. The panel only ever takes gutter space: it is open by default while
 * the right gutter holds its width, and closes on its own before it
 * would cover the content, so the content keeps its width and stays centered.
 * On a narrower page it can still be opened over the content.
 *
 * `resetKey` reopens or closes the panel to its default whenever it changes,
 * so a panel the operator dismissed on one page comes back on the next.
 *
 * The container must reserve its scrollbar gutter (`scrollbar-gutter: stable`)
 * so content taller than the page cannot change the measured gutter.
 */
export function useGutterPanel(
  containerRef: React.RefObject<HTMLElement | null>,
  contentRef: React.RefObject<HTMLElement | null>,
  size: GutterPanelSize | null,
  resetKey?: unknown,
) {
  const [frame, setFrame] = React.useState<GutterPanelFrame>(UNDOCKED_FRAME);
  const [open, setOpen] = React.useState(false);
  const width = size?.width;
  const gap = size?.gap;

  React.useLayoutEffect(() => {
    const container = containerRef.current;
    const content = contentRef.current;
    if (!container || !content || width === undefined || gap === undefined) {
      setFrame(UNDOCKED_FRAME);
      return;
    }

    const measure = () => {
      const rect = container.getBoundingClientRect();
      const clientRight = rect.left + container.clientLeft + container.clientWidth;
      const available = clientRight - content.getBoundingClientRect().right - gap;
      const next: GutterPanelFrame = {
        docked: available >= width,
        width: Math.min(width, container.clientWidth),
        top: Math.max(0, Math.round(rect.top)),
        right: Math.max(0, Math.round(document.documentElement.clientWidth - clientRight)),
      };
      setFrame((current) => (sameFrame(current, next) ? current : next));
    };

    measure();
    // The window scrolls instead of the container on narrow layouts, which
    // moves the container's top edge.
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, { passive: true });
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(container);
    observer?.observe(content);
    return () => {
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure);
      observer?.disconnect();
    };
  }, [containerRef, contentRef, width, gap]);

  React.useEffect(() => setOpen(frame.docked), [frame.docked, resetKey]);

  return { frame, open, setOpen };
}
