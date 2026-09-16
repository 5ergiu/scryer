import assert from "node:assert/strict";
import test from "node:test";

import {
  selectedOverviewEscapeClosesOverview,
  selectedOverviewUsesMovieRecord,
  selectedSeriesSidePanelTitleId,
  selectedSidePanelOwner,
} from "./selected-overview-policy.ts";

test("movie selected overview uses the movie record side panel", () => {
  assert.equal(selectedSidePanelOwner("movies"), "movie-record");
  assert.equal(selectedOverviewUsesMovieRecord("movies"), true);
});

test("series selected overview uses the series side panel container", () => {
  assert.equal(selectedSidePanelOwner("series"), "series-container");
  assert.equal(selectedOverviewUsesMovieRecord("series"), false);
});

test("anime selected overview uses the series side panel container", () => {
  assert.equal(selectedSidePanelOwner("anime"), "series-container");
  assert.equal(selectedOverviewUsesMovieRecord("anime"), false);
});

test("series and anime can render the side panel from selected title id", () => {
  assert.equal(selectedSeriesSidePanelTitleId("series", "title-1"), "title-1");
  assert.equal(selectedSeriesSidePanelTitleId("anime", "title-2"), "title-2");
  assert.equal(selectedSeriesSidePanelTitleId("movies", "title-3"), null);
});

test("Escape closes an open overview on every view, movies included", () => {
  // The movie overview carries an active id while its series side-panel id is
  // always null, so a shortcut gated on the latter never reached movies.
  for (const view of ["movies", "series", "anime"]) {
    const activeOverviewTitleId = "title-1";
    assert.equal(
      selectedOverviewEscapeClosesOverview(true, activeOverviewTitleId),
      true,
      `${view} overview should close on Escape`,
    );
    assert.equal(
      selectedSeriesSidePanelTitleId(view, activeOverviewTitleId),
      view === "movies" ? null : activeOverviewTitleId,
    );
  }
});

test("Escape is inert when no overview is open", () => {
  assert.equal(selectedOverviewEscapeClosesOverview(true, null), false);
  assert.equal(selectedOverviewEscapeClosesOverview(false, "title-1"), false);
  assert.equal(selectedOverviewEscapeClosesOverview(false, null), false);
});
