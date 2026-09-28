/// Normalize an IMDb ID to the canonical `tt{digits}` format.
///
/// Accepts any of: "tt1234567", "1234567", "tt1234567abc", IMDb URLs.
/// Returns `None` for empty strings or strings with no digits.
pub(crate) fn normalize_imdb_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }

    let lower = value.to_ascii_lowercase();
    for (tt_index, _) in lower.match_indices("tt") {
        let digits: String = lower[tt_index + 2..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            return Some(format!("tt{digits}"));
        }
    }

    if value.chars().all(|ch| ch.is_ascii_digit()) {
        Some(format!("tt{value}"))
    } else {
        None
    }
}

/// Normalize a numeric external ID (TVDB, AniDB, etc.) by extracting digits.
pub(crate) fn normalize_numeric_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    let digits: String = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

/// Whether an external id's kind lets it name a title of `facet`. Only TMDB
/// ids are judged: TMDB numbers movies and series separately, so one number
/// can name both, and an anime title also carries its mapped films' ids as
/// `tmdb:movie:N`. A TMDB id counts for a movie when it is kinded as a movie
/// or not kinded at all, and for a series or anime when it is kinded as a
/// series or not kinded at all. Every other source counts whatever its kind.
pub(crate) fn external_id_kind_fits_facet(
    external_id: &scryer_domain::ExternalId,
    facet: &scryer_domain::MediaFacet,
) -> bool {
    if !external_id.source.trim().eq_ignore_ascii_case("tmdb") {
        return true;
    }
    let wanted = match facet {
        scryer_domain::MediaFacet::Movie => "movie",
        scryer_domain::MediaFacet::Series | scryer_domain::MediaFacet::Anime => "series",
    };
    external_id
        .normalized_kind()
        .is_none_or(|kind| kind == wanted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scryer_domain::{ExternalId, MediaFacet};

    #[test]
    fn a_tmdb_id_fits_only_its_own_kind_or_no_kind() {
        let series = ExternalId::with_kind("tmdb", "series", "5150");
        let movie = ExternalId::with_kind("tmdb", "movie", "5150");
        let unkinded = ExternalId::new("tmdb", "5150");
        for facet in [MediaFacet::Series, MediaFacet::Anime] {
            assert!(external_id_kind_fits_facet(&series, &facet));
            assert!(!external_id_kind_fits_facet(&movie, &facet));
            assert!(external_id_kind_fits_facet(&unkinded, &facet));
        }
        assert!(external_id_kind_fits_facet(&movie, &MediaFacet::Movie));
        assert!(!external_id_kind_fits_facet(&series, &MediaFacet::Movie));
        assert!(external_id_kind_fits_facet(&unkinded, &MediaFacet::Movie));
        assert!(external_id_kind_fits_facet(
            &ExternalId::with_kind("TMDB", " Series ", "5150"),
            &MediaFacet::Series
        ));
    }

    #[test]
    fn other_sources_fit_whatever_their_kind() {
        let tvdb_movie = ExternalId::with_kind("tvdb", "movie", "77001");
        assert!(external_id_kind_fits_facet(
            &tvdb_movie,
            &MediaFacet::Series
        ));
    }

    #[test]
    fn imdb_id_with_prefix() {
        assert_eq!(
            normalize_imdb_id("tt1234567"),
            Some("tt1234567".to_string())
        );
    }

    #[test]
    fn imdb_id_digits_only() {
        assert_eq!(normalize_imdb_id("1234567"), Some("tt1234567".to_string()));
    }

    #[test]
    fn imdb_id_with_trailing_chars() {
        assert_eq!(
            normalize_imdb_id("tt0123456abc"),
            Some("tt0123456".to_string())
        );
    }

    #[test]
    fn imdb_id_from_url() {
        assert_eq!(
            normalize_imdb_id("https://www.imdb.com/title/tt0468569/"),
            Some("tt0468569".to_string())
        );
    }

    #[test]
    fn imdb_id_empty() {
        assert_eq!(normalize_imdb_id(""), None);
    }

    #[test]
    fn imdb_id_whitespace_only() {
        assert_eq!(normalize_imdb_id("  "), None);
    }

    #[test]
    fn imdb_id_no_digits() {
        assert_eq!(normalize_imdb_id("abcdef"), None);
    }

    #[test]
    fn imdb_id_trimmed() {
        assert_eq!(
            normalize_imdb_id("  tt1234567  "),
            Some("tt1234567".to_string())
        );
    }

    #[test]
    fn numeric_id_simple() {
        assert_eq!(normalize_numeric_id("12345"), Some("12345".to_string()));
    }

    #[test]
    fn numeric_id_with_prefix() {
        assert_eq!(normalize_numeric_id("aid12345"), Some("12345".to_string()));
    }

    #[test]
    fn numeric_id_empty() {
        assert_eq!(normalize_numeric_id(""), None);
    }

    #[test]
    fn numeric_id_no_digits() {
        assert_eq!(normalize_numeric_id("abc"), None);
    }
}
