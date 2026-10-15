export type CurrentReleaseFile = {
  role: string;
  grabbedReleaseTitle?: string | null;
  sceneName?: string | null;
  originalFilePath?: string | null;
  filePath: string;
  episodeId?: string | null;
  episode?: { collectionId: string | null } | null;
  seriesMovieLinkIds?: string[];
};

export type CurrentReleaseScope = { episodeId?: string | null; collectionId?: string | null; seriesMovieLinkId?: string | null };

export function releaseIdentityName(value: string): string {
  return value.trim().toLowerCase();
}

export function currentReleaseNames(files: readonly CurrentReleaseFile[], scope: CurrentReleaseScope = {}): Set<string> {
  const names = new Set<string>();
  for (const file of files) {
    if (!["primary", "additional"].includes(file.role.toLowerCase())) continue;
    if (scope.episodeId && file.episodeId !== scope.episodeId) continue;
    if (scope.collectionId && file.episode?.collectionId !== scope.collectionId) continue;
    if (scope.seriesMovieLinkId && !file.seriesMovieLinkIds?.includes(scope.seriesMovieLinkId)) continue;
    // Prefer the persisted grab identity. A renamed library filename must
    // never make another release with the same quality appear owned.
    const releaseName = file.grabbedReleaseTitle?.trim() || file.sceneName?.trim();
    const originalName = (file.originalFilePath ?? "").split(/[\\/]/).pop() ?? "";
    const name = releaseName || originalName.replace(/\.(mkv|mp4|avi|wmv|mov|m4v|ts|m2ts|webm|flv|ogv|strm|mpg|mpeg|vob|iso)$/i, "");
    if (name) names.add(releaseIdentityName(name));
  }
  return names;
}
