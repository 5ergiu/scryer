type Counts = { wanted: number; cutoff: number; pending: number };

// Keep only the active authorization scope. Late writes through an old handle
// cannot change the new account's counts.
export function createWantedNavigationCountsCache() {
  let cached: { scope: string; counts: Counts } | null = null;
  return (scope: string): Counts => {
    if (cached?.scope !== scope) {
      cached = { scope, counts: { wanted: 0, cutoff: 0, pending: 0 } };
    }
    return cached.counts;
  };
}
