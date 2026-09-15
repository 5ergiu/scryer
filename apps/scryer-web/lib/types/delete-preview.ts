export type DeletePreview = {
  fingerprint: string;
  totalFileCount: number;
  mediaCount: number;
  subtitleCount: number;
  imageCount: number;
  otherCount: number;
  directoryCount: number;
  requiresTypedConfirmation: boolean;
  typedConfirmationPrompt: string | null;
  targetLabel: string;
  samplePaths: string[];
};

export type DeleteTitlePreviewResult = {
  titleId: string;
  preview: DeletePreview | null;
  error: string | null;
};

export type DeleteTitlesPreview = {
  preview: DeletePreview;
  items: DeleteTitlePreviewResult[];
  failedCount: number;
};

export type DeleteEpisodeFilePreviewResult = {
  fileId: string;
  episodeId: string;
  /** Every requested episode the file covers; a multi-episode file lists each. */
  episodeIds: string[];
  error: string | null;
};

/**
 * The selected episodes whose files a batch delete resolved. One file can
 * cover several episodes (S01E04-E05), so this is not one episode per item.
 */
export function episodeIdsCoveredByEpisodeFileDelete(
  items: readonly DeleteEpisodeFilePreviewResult[],
): Set<string> {
  return new Set(items.flatMap((item) => item.episodeIds));
}

export type DeleteEpisodeFilesPreview = {
  preview: DeletePreview;
  items: DeleteEpisodeFilePreviewResult[];
  fileCount: number;
  failedCount: number;
};
