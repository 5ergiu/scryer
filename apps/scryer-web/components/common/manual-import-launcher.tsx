import * as React from "react";
import { FileInput } from "lucide-react";
import { useClient } from "urql";

import { LoadingMark } from "@/components/common/loading-mark";
import { ManualImportDialog } from "@/components/dialogs/manual-import-dialog";
import { Button } from "@/components/ui/button";
import { useGlobalStatus } from "@/lib/context/global-status-context";
import { useTranslate } from "@/lib/context/translate-context";
import { userFacingGraphQlErrorMessage } from "@/lib/graphql/error-message";
import {
  beginManualImportSelectionMutation,
  queueManualImportMutation,
} from "@/lib/graphql/mutations";
import type { DownloadQueueItem } from "@/lib/types/download-queue";
import {
  type DirectMovieManualImportCandidate,
  directMovieManualImportMappings,
  manualImportNeedsMapping,
  manualImportSelectionNeedsDialog,
} from "@/lib/utils/manual-import-actions";

type ManualImportSelection = {
  selectionId?: string | null;
  archiveExtractionNeeded?: boolean | null;
  files?: Array<DirectMovieManualImportCandidate & { fileName: string }> | null;
};

type DialogTarget = {
  item: DownloadQueueItem;
  titleId: string;
  titleName: string;
  facet: string | null;
};

export type ManualImportLauncher = {
  /** Import a finished download. Failures are reported, never thrown. */
  launch: (item: DownloadQueueItem) => Promise<void>;
  /** The download a direct movie import is working on. */
  busyItemId: string | null;
  /** Render once; it is the file-mapping dialog when one is needed. */
  dialog: React.ReactNode;
};

/**
 * The one way the app imports a finished download by hand. A movie imports
 * its largest file straight away; series, anime and disc images open the
 * dialog so the files can be matched first. The dashboard, the activity page
 * and both title overviews all use this.
 */
export function useManualImportLauncher({
  title,
  onImportQueued,
}: {
  /** The title being viewed, when the download belongs to it. */
  title?: { id: string; name: string; facet: string } | null;
  onImportQueued: (item: DownloadQueueItem) => void;
}): ManualImportLauncher {
  const client = useClient();
  const t = useTranslate();
  const setGlobalStatus = useGlobalStatus();
  const [busyItemId, setBusyItemId] = React.useState<string | null>(null);
  const [dialogTarget, setDialogTarget] = React.useState<DialogTarget | null>(null);
  const onImportQueuedRef = React.useRef(onImportQueued);
  React.useEffect(() => {
    onImportQueuedRef.current = onImportQueued;
  });

  const beginSelection = React.useCallback(
    async (
      item: DownloadQueueItem,
      titleId: string,
      extractArchives: boolean,
    ): Promise<ManualImportSelection | null> => {
      const { data, error } = await client
        .mutation<{ beginManualImportSelection?: ManualImportSelection | null }>(
          beginManualImportSelectionMutation,
          {
            input: {
              clientId: item.clientId,
              clientType: item.clientType,
              downloadClientItemId: item.downloadClientItemId,
              titleId,
              ...(extractArchives ? { extractArchives: true } : {}),
            },
          },
        )
        .toPromise();
      if (error) {
        throw error;
      }
      return data?.beginManualImportSelection ?? null;
    },
    [client],
  );

  const launch = React.useCallback(
    async (item: DownloadQueueItem) => {
      const titleId = title?.id ?? item.titleId;
      if (!titleId) {
        setGlobalStatus(t("queue.assignTitleBeforeImport"));
        return;
      }
      const target: DialogTarget = {
        item,
        titleId,
        titleName: title?.name ?? item.titleName,
        facet: title?.facet ?? item.facet,
      };
      if (manualImportNeedsMapping(target.facet)) {
        setDialogTarget(target);
        return;
      }

      // A movie lands one file, so only its largest candidate is mapped; the
      // server still picks the primary among whatever is mapped.
      setBusyItemId(item.id);
      try {
        let selection = await beginSelection(item, titleId, false);
        if (selection?.archiveExtractionNeeded) {
          selection = await beginSelection(item, titleId, true);
        }
        const candidates = selection?.files ?? [];
        if (manualImportSelectionNeedsDialog(candidates)) {
          setDialogTarget(target);
          return;
        }
        const files = directMovieManualImportMappings(candidates);
        if (!selection?.selectionId || files.length === 0) {
          setGlobalStatus(t("queue.manualImportFailed"), { level: "ERROR" });
          return;
        }
        const { error } = await client
          .mutation(queueManualImportMutation, {
            input: { selectionId: selection.selectionId, files },
          })
          .toPromise();
        if (error) {
          throw error;
        }
        setGlobalStatus(t("queue.manualImportQueued"));
        onImportQueuedRef.current(item);
      } catch (error: unknown) {
        setGlobalStatus(
          userFacingGraphQlErrorMessage(error, t("queue.manualImportFailed")),
          { level: "ERROR" },
        );
      } finally {
        setBusyItemId((current) => (current === item.id ? null : current));
      }
    },
    [beginSelection, client, setGlobalStatus, t, title?.facet, title?.id, title?.name],
  );

  const dialog = dialogTarget ? (
    <ManualImportDialog
      open
      onOpenChange={(open) => {
        if (!open) {
          setDialogTarget(null);
        }
      }}
      titleId={dialogTarget.titleId}
      facet={dialogTarget.facet}
      titleName={dialogTarget.titleName}
      clientId={dialogTarget.item.clientId}
      clientType={dialogTarget.item.clientType}
      downloadClientItemId={dialogTarget.item.downloadClientItemId}
      onImportQueued={() => onImportQueuedRef.current(dialogTarget.item)}
    />
  ) : null;

  return { launch, busyItemId, dialog };
}

/**
 * A title overview's Manual Import button. It imports the most recent finished
 * download for the title and appears only when there is one to import.
 */
export function TitleManualImportButton({
  launcher,
  completedDownloads,
  canManageTitle,
  className,
}: {
  launcher: ManualImportLauncher;
  completedDownloads: readonly DownloadQueueItem[];
  canManageTitle: boolean;
  className?: string;
}) {
  const t = useTranslate();
  const item = completedDownloads[0];
  if (!canManageTitle || !item) {
    return null;
  }
  const busy = launcher.busyItemId === item.id;
  return (
    <Button
      id="title-overview-manual-import"
      type="button"
      variant="outline"
      size="sm"
      className={className}
      disabled={busy}
      onClick={() => {
        void launcher.launch(item);
      }}
    >
      {busy ? (
        <LoadingMark className="mr-1.5 h-4 w-4" />
      ) : (
        <FileInput className="mr-1.5 h-4 w-4" />
      )}
      {t("queue.manualImport")}
    </Button>
  );
}
