import * as React from "react";
import { useClient } from "urql";

import { facetById } from "@/lib/facets/registry";
import {
  titleSettingsDefaultRootFolderQuery,
  titleSettingsQualityProfilesQuery,
} from "@/lib/graphql/queries";

export type TitleSettingsQualityProfileOption = { id: string; name: string };

type QualityProfilesResult = {
  qualityProfileSettings?: {
    profiles?: Array<{ id: string; name: string }> | null;
  } | null;
};

type DefaultRootFolderResult = {
  mediaSettings?: { libraryPath?: string | null } | null;
};

function settingsScopeForFacet(facet: string): "MOVIE" | "SERIES" | "ANIME" {
  if (facet === "MOVIE" || facet === "ANIME") {
    return facet;
  }
  return "SERIES";
}

/**
 * The choices a title's settings panel offers. The quality profiles and the
 * facet's default folder are read separately and each is best effort: someone
 * who manages titles but cannot read media settings still gets the quality
 * profile list, and only the default folder label falls back.
 */
export function useTitleSettingsOptions(facet: string): {
  qualityProfiles: TitleSettingsQualityProfileOption[];
  defaultRootFolder: string;
} {
  const client = useClient();
  const scope = settingsScopeForFacet(facet);
  const fallbackRootFolder = facetById(scope)?.defaultLibraryPath ?? "";
  const [qualityProfiles, setQualityProfiles] = React.useState<
    TitleSettingsQualityProfileOption[]
  >([]);
  const [configuredRootFolder, setConfiguredRootFolder] = React.useState<{
    scope: string;
    path: string;
  } | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    void client
      .query<QualityProfilesResult>(
        titleSettingsQualityProfilesQuery,
        {},
        { requestPolicy: "network-only" },
      )
      .toPromise()
      .then(({ data, error }) => {
        if (cancelled || error) {
          return;
        }
        setQualityProfiles(
          (data?.qualityProfileSettings?.profiles ?? []).map((profile) => ({
            id: profile.id.trim(),
            name: profile.name.trim() || profile.id.trim(),
          })),
        );
      })
      .catch(() => {
        // Best effort: the rest of the title's settings stay usable.
      });
    return () => {
      cancelled = true;
    };
  }, [client]);

  React.useEffect(() => {
    let cancelled = false;
    void client
      .query<DefaultRootFolderResult>(
        titleSettingsDefaultRootFolderQuery,
        { scope },
        { requestPolicy: "network-only" },
      )
      .toPromise()
      .then(({ data, error }) => {
        if (cancelled || error) {
          return;
        }
        const path = (data?.mediaSettings?.libraryPath ?? "").trim();
        if (path) {
          setConfiguredRootFolder({ scope, path });
        }
      })
      .catch(() => {
        // Best effort: the facet's built-in default folder is shown instead.
      });
    return () => {
      cancelled = true;
    };
  }, [client, scope]);

  return {
    qualityProfiles,
    defaultRootFolder:
      configuredRootFolder?.scope === scope
        ? configuredRootFolder.path
        : fallbackRootFolder,
  };
}
