import { useTranslate } from "@/lib/context/translate-context";
import { useUiDateTimeFormat } from "@/lib/context/ui-settings-context";
import { formatUiDateTime } from "@/lib/utils/date-format";
import type { FileScoreEvaluation, ScoreEntry } from "@/lib/types/file-score";

export function ScoreBreakdown({ entries }: { entries: readonly ScoreEntry[] }) {
  const t = useTranslate();
  if (!entries.length) return null;
  return <details className="text-xs">
    <summary className="cursor-pointer">{t("scoring.breakdown")}</summary>
    <ul className="space-y-1 py-2">
      {entries.map((entry, index) => <li key={index} className="flex flex-wrap gap-2">
        <span>{entry.source ? `${entry.source}: ` : ""}{entry.code}</span>
        <span>{entry.delta > 0 ? "+" : ""}{entry.delta}</span>
        {entry.kind.endsWith("_rejection") ? <span>{t("scoring.rejected")}</span> : null}
      </li>)}
    </ul>
  </details>;
}

export function FileScoreSummary({ evaluation, acquisitionScore }: {
  evaluation?: FileScoreEvaluation | null; acquisitionScore?: number | null;
}) {
  const t = useTranslate();
  const dateTimeFormat = useUiDateTimeFormat();
  return <div className="space-y-1 text-xs text-muted-foreground">
    <div>{t("scoring.current")}: {evaluation?.score ?? t("scoring.notEvaluated")}</div>
    {evaluation ? <>
      <div><time dateTime={evaluation.evaluatedAt}>{formatUiDateTime(evaluation.evaluatedAt, dateTimeFormat)}</time>
        {evaluation.needsRefresh ? ` · ${t("scoring.needsRefresh")}` : ""}
        {evaluation.timeSensitive ? ` · ${t("scoring.timeSensitive")}` : ""}</div>
      <ScoreBreakdown entries={evaluation.entries} />
      {evaluation.rejectionCodes.length ? <div>{t("scoring.rejected")}: {evaluation.rejectionCodes.join(", ")}</div> : null}
    </> : null}
    {acquisitionScore != null ? <div>{t("mediaInfo.acquisitionScore")}: {acquisitionScore}</div> : null}
  </div>;
}
