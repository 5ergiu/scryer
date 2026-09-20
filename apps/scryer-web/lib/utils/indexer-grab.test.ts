import assert from "node:assert/strict";
import test from "node:test";
import type { Release } from "@/lib/types/releases";
import type { TitleRecord } from "@/lib/types/titles";
import { grabSubjects, rankGrabSuggestions, groupGrabRouting, grabGroupAllows, pendingGrabRows, type GrabClient } from "./indexer-grab.ts";

const client = (id: string, mapped = false): GrabClient => ({ id, name: id, category: "movies", mapped });

test("suggestions use parsed subjects and deduplicate batches without guessing unparsed titles", () => {
  const releases = [
    { parsedRelease: { normalizedTitle: "Example", year: 2009 } },
    { parsedRelease: { normalizedTitle: "EXAMPLE", year: 2009 } },
    { title: "unparsed" },
  ] as Release[];
  assert.deepEqual(grabSubjects(releases), [{ name: "EXAMPLE", year: 2009 }]);
  const title = (id: string, name: string, year: number) => ({ id, name, year }) as TitleRecord;
  const match = title("exact", "Example", 2009);
  const wrongYear = title("other", "Example", 2020);
  const partial = title("partial", "Example two", 2009);
  assert.deepEqual(rankGrabSuggestions([partial, match, wrongYear, match], grabSubjects(releases)), [match, wrongYear, partial]);
});

test("mapped and unmapped routing groups retain their eligible choices", () => {
  const groups = groupGrabRouting([
    { rowKey: "a", plain: [client("one", true)], assigned: [] },
    { rowKey: "b", plain: [client("two", true)], assigned: [] },
    { rowKey: "c", plain: [client("one"), client("two")], assigned: [] },
    { rowKey: "d", plain: [client("one", true)], assigned: [] },
  ]);
  assert.equal(groups.length, 3);
  assert.deepEqual(groups[0].rows.map((row) => row.rowKey), ["a", "d"]);
  assert.deepEqual(groups[0].clients.map((client) => client.id), ["one"]);
  assert.equal(groups[2].clients.length, 2);
});

test("assignment routing failures do not disable plain grabs", () => {
  const [group] = groupGrabRouting([{ rowKey: "a", plain: [client("one")], assigned: [], assignedError: "No library route" }]);
  assert.equal(grabGroupAllows(group, false), true);
  assert.equal(grabGroupAllows(group, true), false);
  assert.equal(grabGroupAllows({ ...group, clientId: "stale" }, false), false);
});

test("retry selection includes only unsuccessful rows", () => {
  const releases = [{ title: "a" }, { title: "b" }] as Release[];
  assert.deepEqual(pendingGrabRows(releases, new Set(["a"]), (release) => release.title), [releases[1]]);
});
