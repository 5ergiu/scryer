import {
  validateLibraryRootPaths,
  type LibraryRootValidationResult,
} from "./library-root-validation.ts";

export type SetupMediaPathField = "movies" | "series" | "anime";
export type InvalidSetupMediaPathFields = Partial<
  Record<SetupMediaPathField, boolean>
>;

export type SetupMediaPathsInput = {
  moviePath: string;
  seriesPath: string;
  animePath: string | null;
};

export type SetupMediaPathValidationState = {
  invalidPathFields: InvalidSetupMediaPathFields;
  unavailable: boolean;
};

type AdvisorySetupMediaPathSaveOptions = {
  input: SetupMediaPathsInput;
  /** The paths already saved, or null when they could not be read. */
  saved?: SetupMediaPathsInput | null;
  validatePath: (path: string) => Promise<unknown | null | undefined>;
  savePaths: (input: SetupMediaPathsInput) => Promise<void>;
  onValidation: (state: SetupMediaPathValidationState) => void;
  onSaved: (state: SetupMediaPathValidationState) => void;
};

function validationState(
  candidates: Array<{ field: SetupMediaPathField; path: string }>,
  result: LibraryRootValidationResult,
): SetupMediaPathValidationState {
  const invalidPaths = new Set(result.invalidPaths);
  const invalidPathFields: InvalidSetupMediaPathFields = {};
  candidates.forEach(({ field, path }) => {
    if (invalidPaths.has(path)) {
      invalidPathFields[field] = true;
    }
  });
  return { invalidPathFields, unavailable: result.unavailable };
}

/**
 * The paths setup should write: only those that differ from what is saved.
 * Blank leaves that library's roots alone, so re-running setup never rewrites
 * a root it did not change — which would fail while titles live under it.
 */
export function changedSetupMediaPaths(
  input: SetupMediaPathsInput,
  saved: SetupMediaPathsInput | null,
): SetupMediaPathsInput {
  if (!saved) return input;
  const changed = (path: string, savedPath: string | null) =>
    path.trim() === (savedPath ?? "").trim() ? "" : path;
  const animePath = changed(input.animePath ?? "", saved.animePath);
  return {
    moviePath: changed(input.moviePath, saved.moviePath),
    seriesPath: changed(input.seriesPath, saved.seriesPath),
    animePath: animePath.length > 0 ? animePath : null,
  };
}

export async function runAdvisorySetupMediaPathSave({
  input,
  saved = null,
  validatePath,
  savePaths,
  onValidation,
  onSaved,
}: AdvisorySetupMediaPathSaveOptions): Promise<void> {
  const allCandidates: Array<{ field: SetupMediaPathField; path: string }> = [
    { field: "movies", path: input.moviePath },
    { field: "series", path: input.seriesPath },
    { field: "anime", path: input.animePath ?? "" },
  ];
  const candidates = allCandidates.filter(({ path }) => path.length > 0);

  const result = await validateLibraryRootPaths(
    candidates.map(({ path }) => path),
    validatePath,
  );
  const state = validationState(candidates, result);
  onValidation(state);

  const changed = changedSetupMediaPaths(input, saved);
  if (changed.moviePath || changed.seriesPath || changed.animePath) {
    await savePaths(changed);
  }
  onSaved(state);
}
