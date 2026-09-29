export type TrailingThrottle = {
  /** Asks for a run: now when the interval has passed, otherwise once at its end. */
  request: () => void;
  /** Drops a queued run, for unmount. */
  cancel: () => void;
};

/**
 * Limits `run` to at most once per `intervalMs`. A request inside the interval
 * queues one trailing run at the interval's end, and any further requests
 * before then fold into it, so the last request is always followed by a run.
 */
export function createTrailingThrottle(
  run: () => void,
  intervalMs: number,
): TrailingThrottle {
  let lastRunAt: number | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const fire = () => {
    timer = null;
    lastRunAt = Date.now();
    run();
  };

  return {
    request() {
      if (timer !== null) {
        return;
      }
      const wait = lastRunAt === null ? 0 : lastRunAt + intervalMs - Date.now();
      if (wait <= 0) {
        fire();
        return;
      }
      timer = setTimeout(fire, wait);
    },
    cancel() {
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
    },
  };
}
