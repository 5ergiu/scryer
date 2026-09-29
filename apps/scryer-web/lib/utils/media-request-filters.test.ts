import assert from "node:assert/strict";
import test from "node:test";

import type { MediaRequestRecord } from "../types/titles.ts";
import {
  requestCountByFacet,
  requestCountByStatus,
  requestsWithStatus,
} from "./media-request-filters.ts";

function request(
  id: string,
  status: MediaRequestRecord["status"],
  facet: MediaRequestRecord["facet"],
): MediaRequestRecord {
  return { id, status, facet } as MediaRequestRecord;
}

const loaded = [
  request("request-1", "PENDING", "MOVIE"),
  request("request-2", "APPROVED", "MOVIE"),
  request("request-3", "APPROVED", "SERIES"),
  request("request-4", "REJECTED", "ANIME"),
];

test("every tab counts from the full list, not only the selected tab", () => {
  assert.equal(requestCountByStatus(loaded, "PENDING"), 1);
  assert.equal(requestCountByStatus(loaded, "APPROVED"), 2);
  assert.equal(requestCountByStatus(loaded, "CANCELED"), 0);
  assert.equal(requestCountByStatus(loaded, "all"), 4);
});

test("the selected tab shows only its status, and facets count within it", () => {
  const approved = requestsWithStatus(loaded, "APPROVED");
  assert.deepEqual(
    approved.map((entry) => entry.id),
    ["request-2", "request-3"],
  );
  assert.equal(requestCountByFacet(approved, "MOVIE"), 1);
  assert.equal(requestCountByFacet(approved, "ANIME"), 0);
  assert.equal(requestsWithStatus(loaded, "all").length, 4);
});
