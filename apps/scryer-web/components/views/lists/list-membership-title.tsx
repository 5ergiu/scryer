import * as React from "react";

import { CalendarEventHoverCard, type CalendarEpisodeItem } from "@/components/views/calendar-view";
import { useTranslate } from "@/lib/context/translate-context";
import { useIsMobile } from "@/lib/hooks/use-mobile";
import type { ListMembership } from "@/lib/types/lists";
import {
  listMembershipMetadataLookup,
  listMembershipStateLabelKey,
  listMembershipTitleHref,
} from "@/lib/utils/lists";

import { useListTitleMetadata } from "./use-list-title-metadata";

const CLOSE_DELAY_MS = 180;

type HoverAnchor = { top: number; right: number; bottom: number; left: number };

/**
 * A list entry's title: a link when the title is in a library, and for every
 * entry a hover panel with its artwork and overview from the metadata service.
 */
export function ListMembershipTitle({ membership }: { membership: ListMembership }) {
  const t = useTranslate();
  const isMobile = useIsMobile();
  const [anchor, setAnchor] = React.useState<HoverAnchor | null>(null);
  // The panel opens with what the list says and fills in when the service answers;
  // an entry the service cannot describe keeps the plain panel.
  const [metadata, requestMetadata] = useListTitleMetadata(listMembershipMetadataLookup(membership));
  const closeTimer = React.useRef<number | null>(null);
  const label = membership.displayTitle ?? membership.itemKey;
  const href = listMembershipTitleHref(membership.kind, membership.titleId, membership.seriesMovieLinkId);

  const cancelClose = React.useCallback(() => {
    if (closeTimer.current !== null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  }, []);

  const scheduleClose = React.useCallback(() => {
    cancelClose();
    closeTimer.current = window.setTimeout(() => {
      setAnchor(null);
      closeTimer.current = null;
    }, CLOSE_DELAY_MS);
  }, [cancelClose]);

  React.useEffect(() => () => cancelClose(), [cancelClose]);

  React.useEffect(() => {
    if (!anchor) return;
    const close = () => setAnchor(null);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [anchor]);

  const open = (target: HTMLElement) => {
    if (isMobile) return;
    cancelClose();
    const rect = target.getBoundingClientRect();
    setAnchor({ top: rect.top, right: rect.right, bottom: rect.bottom, left: rect.left });
    requestMetadata();
  };

  const hoverProps = {
    onMouseEnter: (event: React.MouseEvent<HTMLElement>) => open(event.currentTarget),
    onMouseLeave: scheduleClose,
  };
  const state = t(listMembershipStateLabelKey(membership.state));
  const episode: CalendarEpisodeItem = {
    id: membership.itemKey,
    titleId: membership.titleId ?? "",
    libraryId: state,
    libraryName: state,
    titleName: membership.year ? `${label} (${membership.year})` : label,
    titleFacet: membership.kind.toLowerCase(),
    seasonNumber: null,
    episodeNumber: null,
    episodeTitle: null,
    overview: metadata?.overview ?? null,
    imageUrl: null,
    airDate: null,
    monitored: true,
    playbackLinks: [],
    mediaAvailability: { state: "UNMONITORED", primaryQualityLabel: null },
  };

  return (
    <>
      {href ? (
        <a
          href={href}
          data-ui="list-membership-title"
          className="rounded-sm hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--scry-focus)]"
          {...hoverProps}
        >
          {label}
        </a>
      ) : (
        <span data-ui="list-membership-title" {...hoverProps}>
          {label}
        </span>
      )}
      {!isMobile && anchor ? (
        <CalendarEventHoverCard
          preview={{ episode, anchor }}
          posterFallbackUrl={metadata?.posterUrl ?? null}
          onMouseEnter={cancelClose}
          onMouseLeave={scheduleClose}
        />
      ) : null}
    </>
  );
}
