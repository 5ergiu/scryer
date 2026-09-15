import { createContext, useContext } from "react";

import type { StatusToastKind } from "@/lib/utils/status-toast";

export type GlobalStatusOptions = {
  toastId?: string;
  /** Set when the caller renders its own richer toast for the same event. */
  suppressToast?: boolean;
  /**
   * The toast level, when the caller already knows it. A catch block knows its
   * status is a failure; the wording does not — server messages such as "nzb
   * download payload was not valid xml" or "category_mismatch: …" carry none of
   * the keywords the text classifier looks for, and would show no toast at all.
   */
  level?: StatusToastKind;
};

export type SetGlobalStatus = (status: string, options?: GlobalStatusOptions) => void;

export const GlobalStatusContext = createContext<SetGlobalStatus | null>(null);

export function useGlobalStatus(): SetGlobalStatus {
  const fn = useContext(GlobalStatusContext);
  if (!fn) throw new Error("useGlobalStatus must be used within GlobalStatusContext.Provider");
  return fn;
}
