import * as React from "react";
import { Shield } from "lucide-react";
import { useClient } from "urql";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useGlobalStatus } from "@/lib/context/global-status-context";
import { useTranslate } from "@/lib/context/translate-context";
import {
  createOperatorKeepClaimMutation,
  releaseTitleClaimMutation,
} from "@/lib/graphql/mutations";
import { titleClaimsQuery } from "@/lib/graphql/queries";
import type { TitleClaimRecord } from "@/lib/types/request-rule-sets";
import { liveOperatorKeep } from "@/lib/utils/request-rule-sets";

/// Keep a title with one click, or show it as kept with the release control
/// for that keep. The release asks for a reason: the API refuses a blank one.
export function TitleKeepControl({
  id,
  titleId,
  keepClaimId,
  onChanged,
}: {
  id: string;
  titleId: string;
  keepClaimId: string | null;
  onChanged: () => void;
}) {
  const t = useTranslate();
  const client = useClient();
  const setGlobalStatus = useGlobalStatus();
  const [busy, setBusy] = React.useState(false);
  const [releaseReason, setReleaseReason] = React.useState<string | null>(null);

  const run = async (request: Promise<{ error?: unknown }>, successKey: string) => {
    setBusy(true);
    try {
      const { error } = await request;
      if (error) throw error;
      setGlobalStatus(t(successKey));
      onChanged();
    } catch (error) {
      setGlobalStatus(error instanceof Error ? error.message : t("status.apiError"), {
        level: "ERROR",
      });
    } finally {
      setBusy(false);
    }
  };

  if (!keepClaimId) {
    return (
      <Button
        id={id}
        type="button"
        size="sm"
        variant="secondary"
        disabled={busy}
        onClick={() =>
          void run(
            client.mutation(createOperatorKeepClaimMutation, { titleId }).toPromise(),
            "status.titleKept",
          )
        }
      >
        <Shield className="h-4 w-4" />
        {t("label.keep")}
      </Button>
    );
  }

  return (
    <span className="inline-flex items-center gap-2">
      <Badge id={`${id}-kept`} tone="positive">
        {t("label.kept")}
      </Badge>
      <Button
        id={`${id}-release`}
        type="button"
        size="sm"
        variant="secondary"
        disabled={busy}
        onClick={() => setReleaseReason("")}
      >
        {t("requests.claimRelease")}
      </Button>
      <Dialog
        open={releaseReason !== null}
        onOpenChange={(open) => {
          if (!open) setReleaseReason(null);
        }}
      >
        <DialogContent id={`${id}-release-dialog`} className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("requests.claimReleaseTitle")}</DialogTitle>
          </DialogHeader>
          <label className="space-y-2">
            <span className="block text-sm font-medium text-card-foreground">
              {t("requests.claimReleaseReason")}
            </span>
            <Input
              id={`${id}-release-reason`}
              value={releaseReason ?? ""}
              placeholder={t("requests.claimReleaseReasonPlaceholder")}
              onChange={(event) => setReleaseReason(event.target.value)}
            />
            <span className="block text-xs text-muted-foreground">
              {t("requests.claimReleaseReasonHelp")}
            </span>
          </label>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setReleaseReason(null)}>
              {t("label.cancel")}
            </Button>
            <Button
              id={`${id}-release-confirm`}
              type="button"
              disabled={busy || !releaseReason?.trim()}
              onClick={() => {
                const reason = releaseReason?.trim() ?? "";
                setReleaseReason(null);
                void run(
                  client
                    .mutation(releaseTitleClaimMutation, {
                      input: { claimId: keepClaimId, reason },
                    })
                    .toPromise(),
                  "status.claimReleased",
                );
              }}
            >
              {t("label.yes")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </span>
  );
}

/// The title page's Keep: reads the title's claims once, then rereads them
/// after each keep or release. Render it only for a reader who manages the
/// title; the claims query refuses anyone else.
export function TitleKeepAction({ titleId }: { titleId: string }) {
  const client = useClient();
  const [claims, setClaims] = React.useState<TitleClaimRecord[] | null>(null);

  const load = React.useCallback(async () => {
    const { data } = await client
      .query(titleClaimsQuery, { titleId }, { requestPolicy: "network-only" })
      .toPromise();
    setClaims((data?.titleClaims ?? null) as TitleClaimRecord[] | null);
  }, [client, titleId]);

  React.useEffect(() => {
    void load();
  }, [load]);

  if (!claims) return null;
  return (
    <div className="mb-3 flex justify-end">
      <TitleKeepControl
        id="title-overview-keep"
        titleId={titleId}
        keepClaimId={liveOperatorKeep(claims)?.id ?? null}
        onChanged={() => void load()}
      />
    </div>
  );
}
