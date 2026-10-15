import * as React from "react";
import { useClient } from "urql";
import { Button } from "@/components/ui/button";
import { useJobRunToasts } from "@/components/root/job-run-provider";
import { JOB_RUN_FIELDS } from "@/lib/graphql/queries";
import { useTranslate } from "@/lib/context/translate-context";
import { useGlobalStatus } from "@/lib/context/global-status-context";
import { normalizeJobRun } from "@/lib/utils/job-runs";

type Selection = { fileIds: string[] } | { seasonIds: string[] } | { titleIds: string[] };
const refresh = `mutation RefreshFileScores($selection: FileScoreSelectionInput!) {
  refreshFileScores(selection: $selection) { ${JOB_RUN_FIELDS} }
}`;
const cancel = `mutation CancelFileScoreRefresh($id: ID!) { cancelFileScoreRefresh(runId: $id) }`;

export function RefreshScoresButton({ selection, disabled = false, single = false }: {
  selection: Selection; disabled?: boolean; single?: boolean;
}) {
  const t = useTranslate();
  const client = useClient();
  const setStatus = useGlobalStatus();
  const { registerInteractiveJobRun, runs } = useJobRunToasts();
  const [pending, setPending] = React.useState(false);
  const [runId, setRunId] = React.useState<string | null>(null);
  const active = runs.some((run) => run.id === runId && !run.completedAt);
  const submit = async () => {
    setPending(true);
    try {
      const result = await client.mutation(refresh, { selection }).toPromise();
      if (result.error) throw result.error;
      const run = normalizeJobRun(result.data?.refreshFileScores);
      if (!run) throw new Error(t("scoring.refreshFailed"));
      setRunId(run.id);
      registerInteractiveJobRun(run);
    } catch {
      setStatus(t("scoring.refreshFailed"), { level: "ERROR" });
    } finally { setPending(false); }
  };
  return <span className="inline-flex gap-1" onClick={(event) => event.stopPropagation()}>
    <Button size="sm" variant="outline" disabled={disabled || pending || active || !Object.values(selection)[0].length}
      onClick={() => void submit()}>{t(single ? "scoring.refreshOne" : "scoring.refreshSelected")}</Button>
    {active ? <Button size="sm" variant="ghost" onClick={() => void client.mutation(cancel, { id: runId }).toPromise()
      .then((result) => { if (result.error) setStatus(t("scoring.refreshFailed"), { level: "ERROR" }); })}>{t("label.cancel")}</Button> : null}
  </span>;
}

/** Reuse the existing job stream to reload visible file data after a score job finishes. */
export function useScoreRefreshUpdates(reload: () => Promise<void> | void) {
  const { runs } = useJobRunToasts();
  const terminal = runs.filter((run) => run.jobKey === "MEDIA_FILE_RESCORE" && run.completedAt)
    .map((run) => run.id).sort().join(",");
  const previous = React.useRef(new Set(terminal ? terminal.split(",") : []));
  React.useEffect(() => {
    const current = new Set(terminal ? terminal.split(",") : []);
    const newlyCompleted = [...current].some((id) => !previous.current.has(id));
    previous.current = current;
    if (newlyCompleted) {
      void reload();
    }
  }, [terminal, reload]);
}
