import assert from "node:assert/strict";
import test from "node:test";
import { createCatalogSummaryLoader } from "./catalog-summary-loader.ts";
import {
  buildTitlesQuery,
  titleCatalogCountsQuery,
  titleCatalogManagedBytesQuery,
} from "../graphql/queries.ts";
import { titleCatalogProjectionForTable } from "./title-catalog-query.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

test("scope changes suppress old results and coalesce to the latest scope", async () => {
  const loader = createCatalogSummaryLoader(() =>
    assert.fail("unexpected failure"),
  );
  const first = deferred<() => void>();
  const seen: string[] = [];
  const pending = loader.run("a", () => first.promise);
  assert.equal(
    loader.run("a", async () => {
      throw new Error("duplicate");
    }),
    pending,
  );
  loader.run("b", async () => () => seen.push("b"));
  loader.run("c", async () => () => seen.push("c"));
  first.resolve(() => seen.push("a"));
  await pending;
  assert.deepEqual(seen, ["c"]);
  await loader.run("c", async () => {
    throw new Error("cached scope refetched");
  });
});

test("mutation invalidation queues a reload and independent bytes do not block counts", async () => {
  const errors: unknown[] = [];
  const counts = createCatalogSummaryLoader((error) => errors.push(error));
  const bytes = createCatalogSummaryLoader((error) => errors.push(error));
  const first = deferred<() => void>();
  const bytesResult = deferred<() => void>();
  const seen: string[] = [];
  const pending = counts.run("scope", () => first.promise);
  const bytesPending = bytes.run("library", () => bytesResult.promise);
  counts.run("scope", async () => () => seen.push("fresh"), true);
  first.resolve(() => seen.push("stale"));
  await pending;
  assert.deepEqual(seen, ["fresh"]);
  await counts.run(
    "scope",
    async () => {
      throw new Error("offline");
    },
    true,
  );
  assert.equal(errors.length, 1);
  assert.deepEqual(seen, ["fresh"]);
  bytes.dispose();
  bytesResult.resolve(() => seen.push("late bytes"));
  await bytesPending;
  assert.deepEqual(seen, ["fresh"]);
});

test("poster pages for all facets request no table enrichment or catalog aggregates", () => {
  for (const facet of ["movie", "series", "anime"]) {
    const projection = titleCatalogProjectionForTable({
      facet,
      visibleColumns: {},
      sort: { key: "name", direction: "asc" },
    });
    assert.ok(Object.values(projection).every((value) => !value));
    const query = buildTitlesQuery(
      { ...projection, includeSettings: false },
      { includeAggregates: false },
    );
    assert.match(query, /hasMore/);
    assert.match(query, /tags/);
    assert.match(buildTitlesQuery(projection), /effectiveUseSeasonFolders/);
    assert.doesNotMatch(
      query,
      /totalCount|filterCounts|managedBytes|episodesOwned|sizeBytes|currentQualityTier|effectiveMetadataLanguage|metadataLanguageOverride|inheritsMetadataLanguage|effectiveUseSeasonFolders|inheritsUseSeasonFolders|useSeasonFoldersOverride/,
    );
  }
  assert.doesNotMatch(titleCatalogCountsQuery, /items|hasMore|managedBytes/);
  assert.doesNotMatch(
    titleCatalogManagedBytesQuery,
    /items|hasMore|totalCount|filterCounts/,
  );
});
