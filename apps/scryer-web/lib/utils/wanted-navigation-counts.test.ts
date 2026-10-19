import test from "node:test";
import assert from "node:assert/strict";
import { createWantedNavigationCountsCache } from "./wanted-navigation-counts.ts";

test("navigation counts persist only within the current authorization scope", () => {
  const cache = createWantedNavigationCountsCache();
  const first = cache("operator-a");
  first.wanted = 12;
  first.cutoff = 3;
  first.pending = 7;
  assert.equal(cache("operator-a"), first);
  const second = cache("operator-b");
  assert.deepEqual(second, { wanted: 0, cutoff: 0, pending: 0 });
  first.wanted = 99;
  assert.equal(cache("operator-b").wanted, 0);
  assert.deepEqual(cache("operator-a"), { wanted: 0, cutoff: 0, pending: 0 });
});
