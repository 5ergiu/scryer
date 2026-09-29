import assert from "node:assert/strict";
import test from "node:test";

import { mergeLatestJobRun, parseFullHashBackfillFailures } from "./job-runs.ts";
import type { JobRun } from "../types/jobs.ts";

test("full-hash backfill failures are read with their path and reason", () => {
  const parsed = parseFullHashBackfillFailures({
    jobKey: "FULL_HASH_BACKFILL",
    summaryJson: {
      failed: 2,
      failures: [
        { mediaFileId: "file-000", path: "/synthetic/a.mkv", reason: "file changed while hashing" },
        { mediaFileId: "file-001", path: "/synthetic/b.mkv", reason: "permission denied" },
      ],
      failuresTruncated: false,
    },
  });

  assert.deepEqual(parsed, {
    failures: [
      { mediaFileId: "file-000", path: "/synthetic/a.mkv", reason: "file changed while hashing" },
      { mediaFileId: "file-001", path: "/synthetic/b.mkv", reason: "permission denied" },
    ],
    notListed: 0,
  });
});

test("a truncated failure list reports how many were not listed", () => {
  const parsed = parseFullHashBackfillFailures({
    jobKey: "FULL_HASH_BACKFILL",
    summaryJson: JSON.stringify({
      failed: 150,
      failures: [{ mediaFileId: "file-000", path: "/synthetic/a.mkv", reason: "read error" }],
      failuresTruncated: true,
    }),
  });

  assert.equal(parsed.failures.length, 1);
  assert.equal(parsed.notListed, 149);
});

test("garbage, legacy, and other jobs' summaries yield no failures", () => {
  const empty = { failures: [], notListed: 0 };
  for (const summaryJson of [null, "not json", 42, [], {}, { failed: 3 }, { failures: "nope" }]) {
    assert.deepEqual(
      parseFullHashBackfillFailures({ jobKey: "FULL_HASH_BACKFILL", summaryJson }),
      empty,
    );
  }
  assert.deepEqual(
    parseFullHashBackfillFailures({
      jobKey: "FULL_HASH_BACKFILL",
      summaryJson: { failures: [null, { path: 1, reason: "x" }, { path: "/synthetic/c.mkv" }] },
    }),
    empty,
  );
  assert.deepEqual(
    parseFullHashBackfillFailures({
      jobKey: "HEALTH_CHECKS",
      summaryJson: { failures: [{ path: "/synthetic/a.mkv", reason: "x" }] },
    }),
    empty,
  );
});

function jobRun(overrides: Partial<JobRun> & Pick<JobRun, "id" | "jobKey" | "startedAt">): JobRun {
  return {
    displayName: overrides.jobKey,
    category: "SYSTEM",
    section: "PRIMARY",
    status: "COMPLETED",
    triggerSource: "SCHEDULED_INTERVAL",
    completedAt: overrides.startedAt,
    summaryJson: null,
    summaryText: null,
    errorText: null,
    progressJson: null,
    libraryScanProgress: null,
    ...overrides,
  } as JobRun;
}

test("each job keeps its own latest run however often another job runs", () => {
  let latest: Partial<Record<JobRun["jobKey"], JobRun>> = {};
  latest = mergeLatestJobRun(
    latest,
    jobRun({ id: "housekeeping-1", jobKey: "HOUSEKEEPING", startedAt: "2026-01-01T00:00:00Z" }),
  );
  for (let minute = 10; minute < 60; minute += 1) {
    latest = mergeLatestJobRun(
      latest,
      jobRun({ id: `rss-${minute}`, jobKey: "RSS_SYNC", startedAt: `2026-01-01T00:${minute}:00Z` }),
    );
  }
  assert.equal(latest.HOUSEKEEPING?.id, "housekeeping-1");
  assert.equal(latest.RSS_SYNC?.id, "rss-59");
});

test("an older run never displaces a newer one, and a snapshot updates its own run", () => {
  const running = jobRun({
    id: "rss-2",
    jobKey: "RSS_SYNC",
    startedAt: "2026-01-01T00:02:00Z",
    status: "RUNNING",
    completedAt: null,
  });
  let latest = mergeLatestJobRun({}, running);
  const older = jobRun({ id: "rss-1", jobKey: "RSS_SYNC", startedAt: "2026-01-01T00:01:00Z" });
  assert.equal(mergeLatestJobRun(latest, older), latest);
  latest = mergeLatestJobRun(latest, { ...running, status: "COMPLETED", completedAt: "2026-01-01T00:03:00Z" });
  assert.equal(latest.RSS_SYNC?.status, "COMPLETED");
});
