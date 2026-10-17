import { useTranslate } from "@/lib/context/translate-context";
import { useUiDateTimeFormat } from "@/lib/context/ui-settings-context";
import { formatUiDateTime } from "@/lib/utils/date-format";
import { scoringEntryText } from "@/lib/utils/release-decision-explanation";
import type { FileScoreEvaluation, ScoreEntry } from "@/lib/types/file-score";

export function ScoreBreakdown({ entries }: { entries: readonly ScoreEntry[] }) {
  const t = useTranslate();
  if (!entries.length) return null;
  return <details className="text-xs">
    <summary className="cursor-pointer">{t("scoring.breakdown")}</summary>
    <ul className="space-y-1 py-2">
      {entries.map((entry, index) => <li key={index} className="flex flex-wrap gap-2">
        <span>{entry.source ? `${entry.source}: ` : ""}{entry.code}</span>
        <span>{scoringEntryText({
          delta: entry.delta,
          kind: entry.kind === "mandatory_rejection" || entry.kind === "final_score_rejection"
            ? entry.kind : "score_contribution",
        }, t)}</span>
      </li>)}
    </ul>
  </details>;
}

export function FileScoreSummary({ evaluation, acquisitionScore }: {
  evaluation?: FileScoreEvaluation | null; acquisitionScore?: number | null;
}) {
  const t = useTranslate();
  const dateTimeFormat = useUiDateTimeFormat();
  // A file shows a current score only once it has been rescored; until then
  // the score it was grabbed with stands on its own.
  if (!evaluation && acquisitionScore == null) return null;
  return <div className="space-y-1 text-xs text-muted-foreground">
    {evaluation ? <>
      <div>{t("scoring.current")}: {evaluation.score}</div>
      <div><time dateTime={evaluation.evaluatedAt}>{formatUiDateTime(evaluation.evaluatedAt, dateTimeFormat)}</time>
        {evaluation.needsRefresh ? ` · ${t("scoring.needsRefresh")}` : ""}
        {evaluation.timeSensitive ? ` · ${t("scoring.timeSensitive")}` : ""}</div>
      <ScoreBreakdown entries={evaluation.entries} />
      {evaluation.rejectionCodes.length ? <div>{t("scoring.rejected")}: {evaluation.rejectionCodes.join(", ")}</div> : null}
    </> : null}
    {acquisitionScore != null ? <div>{t("mediaInfo.acquisitionScore")}: {acquisitionScore}</div> : null}
  </div>;
}
