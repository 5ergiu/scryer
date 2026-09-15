import assert from "node:assert/strict";
import test from "node:test";

import {
  changedSetupMediaPaths,
  runAdvisorySetupMediaPathSave,
  type SetupMediaPathsInput,
} from "./setup-media-paths.ts";

const defaultInput = {
  moviePath: "/data/movies",
  seriesPath: "/data/series",
  animePath: "/data/anime",
};

test("confirmed missing setup paths warn but still save and advance", async () => {
  let savedInput: SetupMediaPathsInput | null = null;
  let advanced = false;
  let validationState: unknown = null;
  let savedValidationState: unknown = null;

  await runAdvisorySetupMediaPathSave({
    input: defaultInput,
    validatePath: async () => ({
      graphQLErrors: [{ extensions: { code: "VALIDATION_ERROR" } }],
    }),
    onValidation: (state) => {
      validationState = state;
    },
    savePaths: async (input) => {
      savedInput = input;
    },
    onSaved: (state) => {
      savedValidationState = state;
      advanced = true;
    },
  });

  assert.deepEqual(validationState, {
    invalidPathFields: { movies: true, series: true, anime: true },
    unavailable: false,
  });
  assert.deepEqual(savedInput, defaultInput);
  assert.deepEqual(savedValidationState, validationState);
  assert.equal(advanced, true);
});

test("unavailable setup validation remains advisory", async () => {
  let saved = false;
  let advanced = false;
  let validationState: unknown = null;

  await runAdvisorySetupMediaPathSave({
    input: defaultInput,
    validatePath: async () => ({
      graphQLErrors: [{ extensions: { code: "SERVICE_UNAVAILABLE" } }],
    }),
    onValidation: (state) => {
      validationState = state;
    },
    savePaths: async () => {
      saved = true;
    },
    onSaved: () => {
      advanced = true;
    },
  });

  assert.deepEqual(validationState, {
    invalidPathFields: {},
    unavailable: true,
  });
  assert.equal(saved, true);
  assert.equal(advanced, true);
});

test("a setup path save failure prevents advancement", async () => {
  let advanced = false;

  await assert.rejects(
    runAdvisorySetupMediaPathSave({
      input: defaultInput,
      validatePath: async () => null,
      onValidation: () => {},
      savePaths: async () => {
        throw new Error("save failed");
      },
      onSaved: () => {
        advanced = true;
      },
    }),
    /save failed/,
  );

  assert.equal(advanced, false);
});

test("re-running setup writes only the paths that changed", async () => {
  const saves: SetupMediaPathsInput[] = [];
  const saved = { moviePath: "/media/films", seriesPath: "/data/series", animePath: null };
  const run = (input: SetupMediaPathsInput) =>
    runAdvisorySetupMediaPathSave({
      input,
      saved,
      validatePath: async () => null,
      onValidation: () => {},
      savePaths: async (changed) => {
        saves.push(changed);
      },
      onSaved: () => {},
    });

  await run({ moviePath: "/media/films ", seriesPath: "/data/series", animePath: null });
  assert.deepEqual(saves, []);

  await run({ moviePath: "/media/films", seriesPath: "/tv", animePath: "/anime" });
  assert.deepEqual(saves, [{ moviePath: "", seriesPath: "/tv", animePath: "/anime" }]);
});

test("paths that could not be read are all written", () => {
  assert.deepEqual(changedSetupMediaPaths(defaultInput, null), defaultInput);
});
