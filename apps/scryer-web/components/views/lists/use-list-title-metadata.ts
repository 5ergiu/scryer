import * as React from "react";
import { useClient } from "urql";

import { metadataMovieQuery, metadataSeriesSummaryQuery } from "@/lib/graphql/queries";
import type { ListTitleMetadataLookup } from "@/lib/utils/lists";

export type ListTitleMetadata = { overview: string | null; posterUrl: string | null };

/**
 * A listed title's description and poster from the metadata service. Nothing is
 * read until `request` is called, and it is read once; a title the service
 * cannot describe stays null.
 */
export function useListTitleMetadata(lookup: ListTitleMetadataLookup | null) {
  const client = useClient();
  const [metadata, setMetadata] = React.useState<ListTitleMetadata | null>(null);
  const requested = React.useRef(false);

  const request = () => {
    if (requested.current || !lookup) return;
    requested.current = true;
    void client
      .query(
        lookup.movie ? metadataMovieQuery : metadataSeriesSummaryQuery,
        { input: lookup.movie ? lookup.input : { ...lookup.input, includeEpisodes: false } },
      )
      .toPromise()
      .then(({ data }) => {
        const found = lookup.movie ? data?.metadataMovie : data?.metadataSeries;
        if (found) setMetadata({ overview: found.overview ?? null, posterUrl: found.posterUrl ?? null });
      });
  };

  return [metadata, request] as const;
}
