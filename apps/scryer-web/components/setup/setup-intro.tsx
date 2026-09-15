import { useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { createPortal } from "react-dom";
import { cn } from "@/lib/utils";

const LOGO_SRC = `${import.meta.env.BASE_URL}scryer-logo.svg`;

export function prefersReducedMotion() {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

/**
 * The setup wizard's welcome, the same one Weaver's first-run setup plays: a
 * large logo appears in the middle of the screen, then flies and shrinks onto
 * the logo in the wizard's header while the rest of the page rises in.
 *
 * The wizard hides its header logo while this plays (`setup-intro` on its
 * shell) and starts the page's own animations once `onStart` fires
 * (`setup-intro-playing`), so both begin together. The wizard's page is
 * centred, so the header only settles once the page's content has: the flight
 * waits for `ready`, and for the artwork to decode, then measures where to
 * land. `onDone` fires when the logo has landed, or at once if there is no
 * header logo to land on. Pass stable callbacks: a new one restarts the wait.
 */
export function SetupIntroMark({
  targetRef,
  ready,
  onStart,
  onDone,
}: {
  targetRef: RefObject<HTMLElement | null>;
  ready: boolean;
  onStart: () => void;
  onDone: () => void;
}) {
  const flyingRef = useRef<HTMLImageElement>(null);
  const [flight, setFlight] = useState<CSSProperties | null>(null);

  useEffect(() => {
    const flying = flyingRef.current;
    if (!ready || !flying) {
      return;
    }
    let cancelled = false;
    void flying
      .decode()
      .catch(() => undefined)
      .then(() => {
        if (cancelled) return;
        const target = targetRef.current;
        if (!target) {
          onDone();
          return;
        }
        // How far the big centred logo has to travel and shrink to land
        // exactly on the header logo.
        const landing = target.getBoundingClientRect();
        const start = flying.getBoundingClientRect();
        setFlight({
          "--setup-intro-x": `${landing.left + landing.width / 2 - (start.left + start.width / 2)}px`,
          "--setup-intro-y": `${landing.top + landing.height / 2 - (start.top + start.height / 2)}px`,
          "--setup-intro-scale": String(landing.height / start.height),
        } as CSSProperties);
        onStart();
      });
    return () => {
      cancelled = true;
    };
  }, [ready, targetRef, onStart, onDone]);

  return createPortal(
    <div
      aria-hidden="true"
      className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center"
    >
      <img
        ref={flyingRef}
        src={LOGO_SRC}
        alt=""
        draggable={false}
        style={flight ?? { opacity: 0 }}
        className={cn(
          "size-[min(38vmin,260px)] select-none object-contain",
          flight && "setup-intro-mark",
        )}
        onAnimationEnd={onDone}
      />
    </div>,
    document.body,
  );
}
