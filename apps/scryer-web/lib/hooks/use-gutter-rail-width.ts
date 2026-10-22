import * as React from "react";

export type GutterRailSize = {
  /** Narrowest the rail may be and still dock. */
  minWidth: number;
  /** Widest the rail grows when the gutter has room to spare. */
  maxWidth: number;
  /** Space kept between the content's right edge and the rail. */
  gap: number;
  /** Space kept between the rail and the scroll container's right edge. */
  inset: number;
};

/**
 * Width for a rail docked in the right gutter beside centered content, or null
 * when the gutter cannot hold the rail's minimum. The rail only ever takes
 * gutter space: the content keeps its own width and stays centered whether the
 * rail is docked or not, and the rail moves out of the gutter long before the
 * content would have to narrow.
 *
 * The container must reserve its scrollbar gutter (`scrollbar-gutter: stable`)
 * so a rail taller than the content cannot change the measured width by adding
 * a scrollbar.
 */
export function useGutterRailWidth(
  containerRef: React.RefObject<HTMLElement | null>,
  contentRef: React.RefObject<HTMLElement | null>,
  size: GutterRailSize | null,
) {
  const [width, setWidth] = React.useState<number | null>(null);
  const minWidth = size?.minWidth;
  const maxWidth = size?.maxWidth;
  const gap = size?.gap;
  const inset = size?.inset;

  React.useLayoutEffect(() => {
    const container = containerRef.current;
    const content = contentRef.current;
    if (
      !container ||
      !content ||
      minWidth === undefined ||
      maxWidth === undefined ||
      gap === undefined ||
      inset === undefined
    ) {
      setWidth(null);
      return;
    }

    const measure = () => {
      const containerRight =
        container.getBoundingClientRect().left + container.clientLeft + container.clientWidth;
      const available = containerRight - content.getBoundingClientRect().right - gap - inset;
      setWidth(available >= minWidth ? Math.floor(Math.min(maxWidth, available)) : null);
    };

    measure();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(container);
    observer.observe(content);
    return () => observer.disconnect();
  }, [containerRef, contentRef, minWidth, maxWidth, gap, inset]);

  return width;
}
