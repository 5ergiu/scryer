import assert from "node:assert/strict";
import test from "node:test";

import { metadataResultExternalIds } from "./metadata-result-external-ids.ts";

test("a TMDB-primary series is identified by its SMG title and TMDB series ids", () => {
  assert.deepEqual(
    metadataResultExternalIds(
      {
        tvdbId: "",
        smgId: 303,
        tmdbId: 3030,
        imdbId: null,
        externalIds: [
          { source: "smg", value: "303" },
          { source: "tmdb", value: "3030" },
        ],
      },
      "SERIES",
    ),
    [
      { source: "smg", kind: "title", value: "303" },
      { source: "tmdb", kind: "series", value: "3030" },
    ],
  );
});

test("a movie's TMDB id and a series' TVDB id stay kindless", () => {
  assert.deepEqual(
    metadataResultExternalIds(
      { tvdbId: "", smgId: 202, tmdbId: 2020, imdbId: "tt0202020" },
      "MOVIE",
    ),
    [
      { source: "smg", kind: "title", value: "202" },
      { source: "tmdb", value: "2020" },
      { source: "imdb", value: "tt0202020" },
    ],
  );
  assert.deepEqual(
    metadataResultExternalIds({ tvdbId: "12345", smgId: null, tmdbId: null, imdbId: null }, "ANIME"),
    [{ source: "tvdb", value: "12345" }],
  );
});
