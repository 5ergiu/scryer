export type ScoreEntry = { code: string; delta: number; kind: string; source: string | null };
export type FileScoreEvaluation = {
  score: number;
  evaluatedAt: string;
  needsRefresh: boolean;
  timeSensitive: boolean;
  entries: ScoreEntry[];
  rejectionCodes: string[];
};
