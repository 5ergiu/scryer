export type SelectedSidePanelOwner = "movie-record" | "series-container";

export function selectedSidePanelOwner(view: string): SelectedSidePanelOwner {
  return view === "movies" ? "movie-record" : "series-container";
}

export function selectedOverviewUsesMovieRecord(view: string): boolean {
  return selectedSidePanelOwner(view) === "movie-record";
}

export function selectedSeriesSidePanelTitleId(
  view: string,
  selectedOverviewTitleId: string | null,
): string | null {
  return selectedOverviewUsesMovieRecord(view) ? null : selectedOverviewTitleId;
}

/**
 * Whether Escape should close the open title overview.
 *
 * Every view can open an overview; only the rendering path differs, so this
 * asks about the active overview id rather than the series-only side-panel id.
 * Gating on the latter left movies — whose side-panel id is always null — with
 * no way to close the overview from the keyboard.
 */
export function selectedOverviewEscapeClosesOverview(
  selectedTitleLayoutActive: boolean,
  activeOverviewTitleId: string | null,
): boolean {
  return selectedTitleLayoutActive && activeOverviewTitleId !== null;
}
