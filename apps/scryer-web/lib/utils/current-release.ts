export type CurrentReleaseFile = {
  role: string;
  grabbedReleaseTitle?: string | null;
  sceneName?: string | null;
  originalFilePath?: string | null;
  filePath: string;
};

export function releaseIdentityName(value: string): string {
  return value.trim().toLowerCase();
}

export function currentReleaseNames(files: readonly CurrentReleaseFile[]): Set<string> {
  const names = new Set<string>();
  for (const file of files) {
    if (file.role.toLowerCase() !== "primary") continue;
    // Prefer the persisted grab identity. A renamed library filename must
    // never make another release with the same quality appear owned.
    const releaseName = file.grabbedReleaseTitle?.trim() || file.sceneName?.trim();
    const originalName = (file.originalFilePath || file.filePath).split(/[\\/]/).pop() ?? "";
    const name = releaseName || originalName.replace(/\.(mkv|mp4|avi|wmv|mov|m4v|ts|m2ts|webm|flv|ogv|strm|mpg|mpeg|vob|iso)$/i, "");
    if (name) names.add(releaseIdentityName(name));
  }
  return names;
}
