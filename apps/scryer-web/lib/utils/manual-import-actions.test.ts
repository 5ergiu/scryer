import assert from "node:assert/strict";
import test from "node:test";

import {
  allowsManualImport,
  compareManualImportSeasonLabels,
  directMovieManualImportMappings,
  downloadImportActions,
  manualImportNeedsMapping,
  manualImportSelectionNeedsDialog,
} from "./manual-import-actions.ts";

test("manual import season labels sort numerically", () => {
  const labels = ["Season 1", "Season 10", "Season 2", "Season 20"];

  assert.deepEqual(labels.sort(compareManualImportSeasonLabels), [
    "Season 1",
    "Season 2",
    "Season 10",
    "Season 20",
  ]);
});

test("direct movie manual import maps only the largest candidate", () => {
  assert.deepEqual(
    directMovieManualImportMappings([
      { candidateId: "sample", sizeBytes: 12_000_000 },
      { candidateId: "movie", sizeBytes: 4_200_000_000 },
      { candidateId: "featurette", sizeBytes: 300_000_000 },
    ]),
    [{ candidateId: "movie" }],
  );
});

test("direct movie manual import treats unknown sizes as smallest and keeps ties stable", () => {
  assert.deepEqual(
    directMovieManualImportMappings([
      { candidateId: "unknown-size", sizeBytes: null },
      { candidateId: "first-of-tie", sizeBytes: 100 },
      { candidateId: "second-of-tie", sizeBytes: 100 },
    ]),
    [{ candidateId: "first-of-tie" }],
  );
  assert.deepEqual(directMovieManualImportMappings([{ candidateId: "only" }]), [
    { candidateId: "only" },
  ]);
});

test("direct movie manual import maps nothing without candidates", () => {
  assert.deepEqual(directMovieManualImportMappings([]), []);
});

// Which download offers which import action is decided once on the server
// (`derive_download_queue_import_actions`); the state matrix that used to live
// here is now covered by the Rust test
// `import_actions_follow_the_state_the_download_is_actually_in`. What the web
// still owns is reading that answer, including from a payload that predates
// the field.

const ALL_IMPORT_ACTIONS = {
  manualImportInteractive: true,
  manualImportDirect: true,
  assignTitle: true,
  ignore: true,
  markFailed: true,
};

test("import actions come straight from the server's answer", () => {
  assert.deepEqual(
    downloadImportActions({ importActions: ALL_IMPORT_ACTIONS }),
    ALL_IMPORT_ACTIONS,
  );
});

test("a payload without server import actions offers nothing", () => {
  const expected = {
    manualImportInteractive: false,
    manualImportDirect: false,
    assignTitle: false,
    ignore: false,
    markFailed: false,
  };

  assert.deepEqual(downloadImportActions({}), expected);
  assert.deepEqual(downloadImportActions({ importActions: null }), expected);
  assert.equal(allowsManualImport({}), false);
});

test("either manual import flavour counts as manual import eligibility", () => {
  assert.equal(
    allowsManualImport({
      importActions: { ...ALL_IMPORT_ACTIONS, manualImportDirect: false },
    }),
    true,
  );
  assert.equal(
    allowsManualImport({
      importActions: { ...ALL_IMPORT_ACTIONS, manualImportInteractive: false },
    }),
    true,
  );
  assert.equal(
    allowsManualImport({
      importActions: {
        ...ALL_IMPORT_ACTIONS,
        manualImportInteractive: false,
        manualImportDirect: false,
      },
    }),
    false,
  );
});

test("only series and anime imports need episode mapping", () => {
  assert.equal(manualImportNeedsMapping("SERIES"), true);
  assert.equal(manualImportNeedsMapping("anime"), true);
  assert.equal(manualImportNeedsMapping("MOVIE"), false);
  assert.equal(manualImportNeedsMapping(null), false);
});

test("a disc image sends a movie import to the dialog", () => {
  assert.equal(
    manualImportSelectionNeedsDialog([
      { fileName: "Example.Movie.2024.mkv" },
      { fileName: "EXAMPLE_DISC.ISO" },
    ]),
    true,
  );
  assert.equal(
    manualImportSelectionNeedsDialog([{ fileName: "Example.Movie.2024.mkv" }]),
    false,
  );
});
