import assert from "node:assert/strict";
import test from "node:test";
import { earliestSearchExpiry, indexerSearchExpiresAt } from "./indexer-search-expiry.ts";

test("completed searches expire five minutes after server completion, not receipt", () => {
  assert.equal(indexerSearchExpiresAt({
    startedAt: "2026-01-01T00:00:00Z",
    completedAt: "2026-01-01T00:00:30Z",
  }), Date.parse("2026-01-01T00:05:30Z"));
});

test("partial and cancelled searches without completion expire conservatively", () => {
  assert.equal(indexerSearchExpiresAt({ startedAt: "2026-01-01T00:00:00Z" }),
    Date.parse("2026-01-01T00:05:00Z"));
  assert.equal(indexerSearchExpiresAt({}), 0);
  assert.equal(indexerSearchExpiresAt({ completedAt: "invalid" }), 0);
});

test("retry jobs do not extend the original results' lifetime", () => {
  const deadlines = new Map([["original", 100], ["retry", 200]]);
  assert.equal(earliestSearchExpiry(deadlines), 100);
  deadlines.set("retry", 300);
  assert.equal(earliestSearchExpiry(deadlines), 100);
  deadlines.clear();
  assert.equal(earliestSearchExpiry(deadlines), null);
});
