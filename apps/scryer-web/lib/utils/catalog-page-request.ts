// A failed page stays paused until an explicit retry or a new catalog load.
export function createCatalogPageRequestGate() {
  let generation = 0;
  let paused = false;
  let pending = false;
  return {
    get paused() {
      return paused;
    },
    reset() {
      generation += 1;
      paused = false;
      pending = false;
    },
    async run<T>(request: () => Promise<T>): Promise<T | undefined> {
      if (paused || pending) return undefined;
      const current = generation;
      pending = true;
      try {
        return await request();
      } catch (error) {
        if (current === generation) paused = true;
        throw error;
      } finally {
        if (current === generation) pending = false;
      }
    },
  };
}
