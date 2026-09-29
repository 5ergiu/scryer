export function buildTvdbMovieUrl(
  tvdbIdValue: string | null | undefined,
  slugValue?: string | null,
) {
  const tvdbId = tvdbIdValue?.trim();
  if (!tvdbId) {
    return null;
  }
  const slug = slugValue?.trim();
  const base = "https://thetvdb.com";
  if (slug) {
    return `${base}/movies/${encodeURIComponent(slug)}`;
  }
  return `${base}/dereferrer/movie/${encodeURIComponent(tvdbId)}`;
}

export function buildTvdbSeriesUrl(
  slugValue?: string | null,
  tvdbIdValue?: string | null,
) {
  // A TMDB-primary series has no TVDB id, and its slug is not a TheTVDB
  // slug, so it gets no TheTVDB link at all.
  const tvdbId = tvdbIdValue?.trim();
  if (!tvdbId) {
    return null;
  }
  const slug = slugValue?.trim();
  if (slug) {
    return `https://thetvdb.com/series/${encodeURIComponent(slug)}`;
  }
  return `https://thetvdb.com/dereferrer/series/${encodeURIComponent(tvdbId)}`;
}
