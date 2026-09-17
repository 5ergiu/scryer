import * as React from "react";

import { useJobRunToasts } from "@/components/root/job-run-provider";
import type { JobRun } from "@/lib/types";

/**
 * Show a job the viewer just started and run `onTerminal` once it finishes.
 * Anything still being watched is let go when the component unmounts.
 */
export function useTrackedJobRuns(): (
  run: JobRun,
  onTerminal?: (run: JobRun) => void,
) => void {
  const { registerInteractiveJobRun } = useJobRunToasts();
  const unregistersRef = React.useRef(new Set<() => void>());

  React.useEffect(() => {
    const unregisters = unregistersRef.current;
    return () => {
      for (const unregister of unregisters) {
        unregister();
      }
      unregisters.clear();
    };
  }, []);

  return React.useCallback(
    (run: JobRun, onTerminal?: (run: JobRun) => void) => {
      if (!onTerminal) {
        registerInteractiveJobRun(run);
        return;
      }
      const unregister = registerInteractiveJobRun(run, (terminalRun) => {
        unregister();
        unregistersRef.current.delete(unregister);
        onTerminal(terminalRun);
      });
      unregistersRef.current.add(unregister);
    },
    [registerInteractiveJobRun],
  );
}
