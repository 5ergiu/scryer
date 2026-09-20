/** One active summary request and one latest trailing invalidation per owner. */
export function createCatalogSummaryLoader(onError: (error: unknown) => void) {
  let active = true;
  let generation = 0;
  let scope = "";
  let ready = false;
  let pending: Promise<void> | undefined;
  let queued: (() => Promise<() => void>) | undefined;
  return {
    activate() {
      active = true;
    },
    dispose() {
      active = false;
      generation += 1;
      pending = undefined;
      queued = undefined;
      ready = false;
      scope = "";
    },
    run(key: string, load: () => Promise<() => void>, invalidate = false) {
      if (!active) return Promise.resolve();
      const changed = scope !== key;
      scope = key;
      if (changed || invalidate) ready = false;
      if (pending) {
        if (changed || invalidate) queued = load;
        return pending;
      }
      if (ready) return Promise.resolve();
      const started = generation;
      pending = Promise.resolve()
        .then(async () => {
          let next: (() => Promise<() => void>) | undefined = load;
          while (next && active && generation === started) {
            try {
              const publish = await next();
              if (active && generation === started && !queued) {
                publish();
                ready = true;
              }
            } catch (error) {
              if (active && generation === started && !queued) onError(error);
            }
            if (!active || generation !== started) return;
            next = queued;
            queued = undefined;
          }
        })
        .finally(() => {
          if (generation === started) pending = undefined;
        });
      return pending;
    },
  };
}
