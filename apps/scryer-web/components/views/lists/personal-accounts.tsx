import * as React from "react";
import { Link } from "react-router";
import { LoadingMark } from "@/components/common/loading-mark";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { useTranslate } from "@/lib/context/translate-context";
import type {
  ListAccount,
  ListPreview,
  ListProviderItem,
  ListProviderManifest,
  ListSourceDraft,
  ListSubscription,
} from "@/lib/types/lists";
import { isListSourceFollowed } from "@/lib/utils/lists";
import { buildViewPath } from "@/lib/utils/routing";
import { CatalogListTable, type CatalogListRow } from "./catalog-list-table";
import { ListProviderSetup } from "./list-provider-setup";
import { ProviderRail, ProviderRailEntry } from "./provider-rail";
import { ProviderTile } from "./provider-tile";

type Props = {
  providers: ListProviderManifest[];
  accounts: ListAccount[];
  /** The member's own follows, so a list already followed is not offered again. */
  subscriptions: ListSubscription[];
  managedAccount: ListAccount | null;
  accountLoading: boolean;
  busyIds: ReadonlySet<string>;
  linkingProvider: string | null;
  linkError: string | null;
  onLink: (provider: string) => void;
  onCancelLink: () => void;
  onManage: (account: ListAccount | null) => void;
  onUnlink: (account: ListAccount) => Promise<boolean>;
  onFollow: (manifest: ListProviderManifest, item: ListProviderItem, source: ListSourceDraft, name: string) => void;
  /** Reads one of the account's lists as it stands, for the preview in its row. */
  onPreviewSource: (source: ListSourceDraft) => Promise<ListPreview | null>;
};

type ConnectionsProps = Pick<Props, "providers" | "accounts" | "linkingProvider" | "onLink"> & {
  renderActions?: (account: ListAccount) => React.ReactNode;
};

export function PersonalAccountConnections(props: ConnectionsProps) {
  const t = useTranslate();
  return (
    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
      {props.providers.flatMap((provider) => {
        const linkedAccounts = props.accounts.filter((entry) => entry.provider === provider.providerType);
        return (linkedAccounts.length ? linkedAccounts : [null]).map((linked) => ({ provider, linked }));
      }).map(({ provider, linked }) => (
        <Card key={linked?.id ?? provider.providerType} id={`lists-account-${linked?.id ?? provider.providerType}`}>
          <CardContent>
            <div className="flex items-center gap-3">
              <ProviderTile provider={provider} />
              <div className="min-w-0 flex-1">
                <h3 className="font-semibold">{provider.name}</h3>
                {linked ? <p className="truncate text-sm text-muted-foreground">{linked.displayName ?? linked.username ?? linked.externalUserId}</p> : null}
              </div>
              {linked ? <Badge tone={linked.status === "ACTIVE" ? "positive" : "warning"}>{t(linked.status === "ACTIVE" ? "lists.accounts.connected" : "lists.accounts.reconnect")}</Badge> : null}
            </div>
            {linked?.errorMessage ? <p className="text-sm text-destructive">{linked.errorMessage}</p> : null}
            <div className="flex flex-wrap gap-2">
              {linked ? props.renderActions?.(linked) : null}
              <Button size="sm" variant={linked ? "outline" : "default"} disabled={!!props.linkingProvider} onClick={() => props.onLink(provider.providerType)}>
                {t(linked ? "lists.accounts.reconnect" : "lists.accounts.connect")}
              </Button>
              {linked ? (
                <Button size="sm" variant="outline" disabled={!!props.linkingProvider} onClick={() => props.onLink(provider.providerType)}>
                  {t("lists.accounts.connectAnother")}
                </Button>
              ) : null}
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

/** A provider in the rail: one entry per linked account, or one for a provider with none. */
type AccountEntry = { key: string; provider: ListProviderManifest; linked: ListAccount | null };

/**
 * The member's own provider accounts: a rail of the providers that offer
 * personal lists, and for the open one its connection and the lists that
 * account can follow.
 */
export function PersonalAccounts(props: Props) {
  const t = useTranslate();
  const { onManage } = props;
  const catalog = props.providers.filter((provider) => provider.groups.some((group) => group.items.some((item) => item.personal)));
  const entries = catalog.flatMap((provider): AccountEntry[] => {
    const linkedAccounts = props.accounts.filter((entry) => entry.provider === provider.providerType);
    return linkedAccounts.length
      ? linkedAccounts.map((linked) => ({ key: linked.id, provider, linked }))
      : [{ key: provider.providerType, provider, linked: null }];
  });
  // Linking or unlinking changes an entry's key, so the provider is kept to fall back on.
  const [picked, setPicked] = React.useState<{ key: string; providerType: string } | null>(null);
  // A further account being connected opens once it arrives; the accounts there before it are kept to tell it apart.
  const [adding, setAdding] = React.useState<{ providerType: string; known: string[] } | null>(null);
  const selected =
    entries.find((entry) => entry.linked && entry.provider.providerType === adding?.providerType && !adding.known.includes(entry.key)) ??
    entries.find((entry) => entry.key === picked?.key) ??
    entries.find((entry) => entry.provider.providerType === picked?.providerType) ??
    entries[0] ??
    null;
  const [unlinkingId, setUnlinkingId] = React.useState<string | null>(null);

  // The open account's lists are read from the provider when it is opened, and again once it is reconnected.
  const linked = selected?.linked ?? null;
  const linkedId = linked?.id ?? null;
  const linkedStatus = linked?.status ?? null;
  const openAccount = React.useRef<() => void>(() => {});
  React.useEffect(() => {
    openAccount.current = () => onManage(linked);
  });
  React.useEffect(() => {
    openAccount.current();
  }, [linkedId, linkedStatus]);

  const manifest = selected?.provider ?? null;
  // The loaded account carries its lists; until it arrives the linked one stands in.
  const account = linked && props.managedAccount?.id === linked.id ? props.managedAccount : linked;
  const items = manifest?.groups.flatMap((group) => group.items).filter((item) => item.personal) ?? [];
  const choices = account ? [
    ...account.ownedLists.map((list) => ({ key: `list:${list.id}`, label: list.name, sourceType: list.sourceType, params: list.params, kinds: list.kinds })),
    ...account.statuses.map((status) => ({ key: `status:${status.key}`, label: status.label, sourceType: status.sourceType, params: status.params, kinds: status.kinds })),
  ] : [];
  const rows: CatalogListRow[] = [];
  if (account && manifest) {
    const followed = (sourceType: string, params: ListSourceDraft["params"]) =>
      isListSourceFollowed(props.subscriptions, { provider: account.provider, sourceType, params, accountId: account.id });
    const followId = (key: string) => `lists-account-follow-${account.id}-${key.replace(/[^A-Za-z0-9_-]/g, "-")}`;
    for (const item of items) {
      if (choices.some((choice) => choice.sourceType === item.sourceType)) continue;
      const source: ListSourceDraft = { provider: account.provider, sourceType: item.sourceType, params: [], url: null, credentialId: account.id };
      rows.push({
        key: `item:${item.id}`,
        name: item.name,
        description: item.description,
        kinds: item.kinds,
        intervalSeconds: item.defaultIntervalSeconds,
        followed: followed(item.sourceType, []),
        followId: followId(item.id),
        onFollow: () => props.onFollow(manifest, item, source, item.name),
        // A list that needs a value has nothing to read until it is given one.
        onPreview: item.params.some((param) => param.required) ? undefined : () => props.onPreviewSource(source),
      });
    }
    for (const choice of choices) {
      const item = items.find((entry) => entry.sourceType === choice.sourceType);
      if (!item) continue;
      const source: ListSourceDraft = { provider: account.provider, sourceType: choice.sourceType, params: choice.params, fixedParams: true, url: null, credentialId: account.id };
      rows.push({
        key: choice.key,
        name: choice.label,
        kinds: choice.kinds,
        intervalSeconds: item.defaultIntervalSeconds,
        followed: followed(choice.sourceType, choice.params),
        followId: followId(choice.key),
        onFollow: () => props.onFollow(manifest, { ...item, kinds: choice.kinds }, source, choice.label),
        onPreview: () => props.onPreviewSource(source),
      });
    }
  }
  const confirmingUnlink = !!account && unlinkingId === account.id;

  return (
    <section id="lists-personal-accounts" className="space-y-3">
      <div className="flex min-h-8 flex-wrap items-center justify-between gap-3">
        <h2 className="font-display text-[17px] font-bold">{t("lists.accounts.heading")}</h2>
        <Button asChild size="sm" variant="outline">
          <Link id="lists-profile-link" to={buildViewPath("settings", "profile")}>
            {t("lists.accounts.linkProfile")}
          </Link>
        </Button>
      </div>
      <p className="text-sm text-[var(--scry-muted)]">{t("lists.accounts.copy")}</p>
      {catalog.length === 0 ? <ListProviderSetup /> : null}
      {selected ? (
        <div id="lists-accounts" className="grid gap-4 md:grid-cols-[220px_minmax(0,1fr)]">
          <ProviderRail label={t("lists.catalog.providers")}>
            {entries.map((entry) => (
              <ProviderRailEntry
                key={entry.key}
                id={`lists-account-${entry.key}`}
                active={entry.key === selected.key}
                onSelect={() => {
                  setAdding(null);
                  setPicked({ key: entry.key, providerType: entry.provider.providerType });
                }}
                icon={<ProviderTile provider={entry.provider} size="sm" />}
                name={entry.provider.name}
                detail={
                  entry.linked
                    ? (entry.linked.displayName ?? entry.linked.username ?? entry.linked.externalUserId)
                    : t("lists.accounts.notConnected")
                }
              />
            ))}
          </ProviderRail>

          <div id={`lists-account-panel-${selected.key}`} className="min-w-0 space-y-3">
            <div className="flex flex-wrap items-center gap-2">
              {account ? (
                <Badge tone={account.status === "ACTIVE" ? "positive" : "warning"}>
                  {t(account.status === "ACTIVE" ? "lists.accounts.connected" : "lists.accounts.needsReconnect")}
                </Badge>
              ) : null}
              <Button
                id={`lists-account-link-${selected.key}`}
                size="sm"
                variant={account?.status === "ACTIVE" ? "outline" : "primary"}
                disabled={!!props.linkingProvider}
                onClick={() => props.onLink(selected.provider.providerType)}
              >
                {t(account ? "lists.accounts.reconnect" : "lists.accounts.connect")}
              </Button>
              {account ? (
                <Button
                  id={`lists-account-add-${selected.key}`}
                  size="sm"
                  variant="outline"
                  disabled={!!props.linkingProvider}
                  onClick={() => {
                    const providerType = selected.provider.providerType;
                    setAdding({
                      providerType,
                      known: props.accounts.filter((entry) => entry.provider === providerType).map((entry) => entry.id),
                    });
                    props.onLink(providerType);
                  }}
                >
                  {t("lists.accounts.connectAnother")}
                </Button>
              ) : null}
              {account && !confirmingUnlink ? (
                <Button id={`lists-account-unlink-${account.id}`} size="sm" variant="outline" onClick={() => setUnlinkingId(account.id)}>
                  {t("lists.accounts.unlink")}
                </Button>
              ) : null}
            </div>
            {account?.errorMessage ? (
              <p className="text-sm text-[var(--scry-danger-text)]">{account.errorMessage}</p>
            ) : null}
            {props.linkError ? <p role="alert" className="text-sm text-[var(--scry-danger-text)]">{props.linkError}</p> : null}
            {props.linkingProvider ? (
              <div role="status" className="flex items-center gap-2 text-sm">
                <LoadingMark className="h-4 w-4" />{t("lists.accounts.waiting")}
                <Button size="sm" variant="outline" onClick={props.onCancelLink}>{t("label.cancel")}</Button>
              </div>
            ) : null}
            {props.linkingProvider && adding ? (
              <p className="text-[12.5px] text-[var(--scry-muted)]">{t("lists.accounts.connectAnotherHint")}</p>
            ) : null}
            {account && confirmingUnlink ? (
              <div className="space-y-2 rounded-[12px] border border-[var(--scry-border3)] p-3">
                <p className="text-sm text-[var(--scry-muted)]">{t("lists.accounts.unlinkHelp")}</p>
                <div className="flex gap-2">
                  <Button
                    id={`lists-account-unlink-confirm-${account.id}`}
                    size="sm"
                    disabled={props.busyIds.has(account.id)}
                    onClick={() => void props.onUnlink(account).then(() => setUnlinkingId(null))}
                  >
                    {t("lists.accounts.confirmUnlink")}
                  </Button>
                  <Button size="sm" variant="outline" onClick={() => setUnlinkingId(null)}>{t("label.cancel")}</Button>
                </div>
              </div>
            ) : null}
            {!account ? null : props.accountLoading ? (
              <p role="status" className="flex items-center gap-2 text-[12.5px] text-[var(--scry-muted)]">
                <LoadingMark className="h-4 w-4" />
                {t("label.loading")}
              </p>
            ) : rows.length > 0 ? (
              <CatalogListTable id={`lists-account-lists-${account.id}`} rows={rows} />
            ) : null}
          </div>
        </div>
      ) : null}
    </section>
  );
}
