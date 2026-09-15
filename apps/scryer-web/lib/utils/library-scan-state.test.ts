import assert from "node:assert/strict";
import test from "node:test";

import type { LibraryScanProgress } from "../types/library-scans.ts";
import { LibraryScanState } from "./library-scan-state.ts";

function scan(
  id: string,
  second = 0,
  status: LibraryScanProgress["status"] = "RUNNING",
): LibraryScanProgress {
  return {
    sessionId: id,
    libraryId: `library-${id}`,
    facet: "ANIME",
    mode: "FULL",
    status,
    startedAt: "2026-01-01T00:00:00Z",
    updatedAt: `2026-01-01T00:00:${String(second).padStart(2, "0")}Z`,
    foundTitles: 2,
    titleMatchTotalKnown: true,
    hydrationTotalKnown: true,
    mediaAnalysisTotalKnown: true,
    titleMatchProgress: { total: 2, completed: 2, failed: 0 },
    hydrationProgress: { total: 2, completed: 2, failed: 0 },
    mediaAnalysisProgress: { total: 13, completed: second, failed: 0 },
    summary: null,
  };
}

test("polling continues through frequent updates from another library", async (t) => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  const state = new LibraryScanState();
  state.accept([scan("small"), scan("large")]);
  let polls = 0;
  const errors: unknown[] = [];
  const stop = state.poll(
    () =>
      state.refresh(
        async () => {
          polls++;
          return [scan("large", 10)];
        },
        async (id) => scan(id, 10, "COMPLETED"),
      ),
    (error) => errors.push(error),
  );
  for (let i = 0; i < 25; i++) {
    state.accept([scan("large", 1)]);
    t.mock.timers.tick(200);
  }
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(polls, 1);
  assert.equal(state.getSnapshot().small.status, "COMPLETED");
  assert.deepEqual(errors, []);
  stop();
  t.mock.timers.tick(10_000);
  assert.equal(polls, 1);
});

test("a late active snapshot cannot overwrite newer progress or terminal status", async () => {
  const state = new LibraryScanState();
  state.accept([scan("one")]);
  let resolve!: (value: LibraryScanProgress[]) => void;
  const request = state.refresh(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
    async () => null,
  );
  state.accept([scan("one", 8, "COMPLETED"), scan("two", 8)]);
  resolve([scan("one", 1), scan("two", 1)]);
  await request;
  assert.equal(state.getSnapshot().one.status, "COMPLETED");
  assert.equal(state.getSnapshot().two.mediaAnalysisProgress.completed, 8);
});

test("an empty active response recovers the actual finished report", async () => {
  const state = new LibraryScanState();
  state.accept([scan("one")]);
  const finished = {
    ...scan("one", 10, "WARNING"),
    summary: { scanned: 2, matched: 1, imported: 1, skipped: 0, unmatched: 1 },
  };
  const active = await state.refresh(
    async () => [],
    async (id) => {
      assert.equal(id, "one");
      return finished;
    },
  );
  assert.deepEqual(active, []);
  assert.equal(state.getSnapshot().one, finished);
});

test("concurrent refreshes share one request, and failure allows retry", async () => {
  const state = new LibraryScanState();
  let reject!: (error: Error) => void;
  const first = state.refresh(
    () =>
      new Promise((_, fail) => {
        reject = fail;
      }),
    async () => null,
  );
  const second = state.refresh(
    async () => {
      throw new Error("duplicate request");
    },
    async () => null,
  );
  assert.equal(first, second);
  reject(new Error("offline"));
  await assert.rejects(first, /offline/);
  await state.refresh(
    async () => [scan("one")],
    async () => null,
  );
  assert.ok(state.getSnapshot().one);
});

test("a dismissed completion cannot be resurrected by an outstanding lookup", async () => {
  const state = new LibraryScanState();
  state.accept([scan("one")]);
  let resolve!: (value: LibraryScanProgress) => void;
  const request = state.refresh(
    async () => [],
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await Promise.resolve();
  state.accept([scan("one", 9, "COMPLETED")]);
  state.dismiss("one");
  resolve(scan("one", 10, "COMPLETED"));
  await request;
  assert.equal(state.getSnapshot().one, undefined);
});

test("a missing lookup cannot remove a newer live snapshot", async () => {
  const state = new LibraryScanState();
  state.accept([scan("one")]);
  let resolve!: (value: null) => void;
  const request = state.refresh(
    async () => [],
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await Promise.resolve();
  state.accept([scan("one", 8), scan("two", 8)]);
  resolve(null);
  await request;
  assert.equal(state.getSnapshot().one.mediaAnalysisProgress.completed, 8);
  assert.ok(state.getSnapshot().two);
});

test("failed completion lookup preserves the running state for retry", async () => {
  const state = new LibraryScanState();
  const initial = scan("one");
  state.accept([initial]);
  await assert.rejects(
    state.refresh(
      async () => [],
      async () => {
        throw new Error("offline");
      },
    ),
    /offline/,
  );
  assert.equal(state.getSnapshot().one, initial);
  await state.refresh(
    async () => [],
    async () => scan("one", 10, "FAILED"),
  );
  assert.equal(state.getSnapshot().one.status, "FAILED");
});
