//! Language-specific naming tokens. Catalog collation is deliberately separate.

use regex::Regex;
use std::sync::LazyLock;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use unicode_segmentation::UnicodeSegmentation;

struct Articles {
    aliases: &'static [&'static str],
    words: &'static [&'static str],
    elided: &'static [&'static str],
}

const ARTICLES: &[Articles] = &[
    Articles {
        aliases: &["en", "eng"],
        words: &["the", "a", "an"],
        elided: &[],
    },
    Articles {
        aliases: &["es", "spa"],
        words: &["el", "la", "los", "las", "un", "una", "unos", "unas"],
        elided: &[],
    },
    Articles {
        aliases: &["pt", "por"],
        words: &["o", "a", "os", "as", "um", "uma", "uns", "umas"],
        elided: &[],
    },
    Articles {
        aliases: &["fr", "fra", "fre"],
        words: &["le", "la", "les", "un", "une", "des"],
        elided: &["l'", "l’"],
    },
    Articles {
        aliases: &["it", "ita"],
        words: &["il", "lo", "la", "i", "gli", "le", "un", "uno", "una"],
        elided: &["l'", "l’", "un'", "un’"],
    },
    Articles {
        aliases: &["de", "deu", "ger"],
        words: &["der", "die", "das", "ein", "eine"],
        elided: &[],
    },
    Articles {
        aliases: &["nl", "nld", "dut"],
        words: &["de", "het", "een"],
        elided: &[],
    },
    Articles {
        aliases: &["sv", "swe"],
        words: &["en", "ett"],
        elided: &[],
    },
    Articles {
        aliases: &["da", "dan"],
        words: &["en", "et"],
        elided: &[],
    },
    Articles {
        aliases: &["nb", "nob", "no", "nor"],
        words: &["en", "ei", "et"],
        elided: &[],
    },
    Articles {
        aliases: &["nn", "nno"],
        words: &["ein", "ei", "eit"],
        elided: &[],
    },
];

static LETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{L}").expect("letter pattern"));
static LETTER_OR_DIGIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}\p{Nd}]").expect("letter or digit pattern"));
static LATIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Latin}").expect("Latin pattern"));

/// Move a single lexical article, preserving its spelling and trailing qualifiers.
/// Unknown languages never borrow another language's article vocabulary.
pub fn title_with_article(title: &str, language: Option<&str>) -> String {
    let language = language.unwrap_or("").trim().to_ascii_lowercase();
    let language = language.split(['-', '_']).next().unwrap_or("");
    let Some(table) = ARTICLES
        .iter()
        .find(|table| table.aliases.contains(&language))
    else {
        return title.to_string();
    };
    let normalized: String = title.trim().nfc().collect();
    let mut end = normalized.len();
    while normalized[..end].ends_with(')') {
        let prefix = &normalized[..end];
        let Some(open) = prefix.rfind(" (") else {
            break;
        };
        if prefix[open + 2..prefix.len() - 1].is_empty()
            || prefix[open + 2..prefix.len() - 1].contains(['(', ')'])
        {
            break;
        }
        end = open;
    }
    let body = &normalized[..end];
    let suffix = &normalized[end..];
    let split = body
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .and_then(|(index, _)| {
            table
                .words
                .iter()
                .any(|word| body[..index].eq_ignore_ascii_case(word))
                .then_some(index)
        })
        .or_else(|| {
            table.elided.iter().find_map(|article| {
                // Prefixes are ASCII letters followed by an apostrophe. Slice only at
                // verified UTF-8 boundaries in the original spelling.
                body.get(..article.len())
                    .filter(|prefix| prefix.eq_ignore_ascii_case(article))
                    .and_then(|_| body.get(article.len()..))
                    .filter(|rest| LETTER.is_match(rest))
                    .map(|_| article.len())
            })
        });
    let Some(split) = split else {
        return title.to_string();
    };
    let rest = body[split..].trim();
    if rest.is_empty() {
        return title.to_string();
    }
    format!("{rest}, {}{suffix}", &body[..split])
}

/// The first complete letter/digit grapheme after article adjustment.
pub fn title_first_character(title: &str, language: Option<&str>) -> String {
    let adjusted = title_with_article(title, language);
    let initial_source = if adjusted != title {
        adjusted
            .rsplit_once(", ")
            .map(|(body, _)| body)
            .unwrap_or(&adjusted)
    } else {
        &adjusted
    };
    let normalized: String = initial_source.nfc().collect();
    let Some(grapheme) = normalized
        .graphemes(true)
        .find(|part| LETTER_OR_DIGIT.is_match(part))
    else {
        return "_".into();
    };
    if !LATIN.is_match(grapheme) {
        return grapheme.to_uppercase();
    }
    let upper = grapheme.to_uppercase();
    let first = upper.chars().next().unwrap_or('_');
    let mapped = match first {
        'Æ' => Some('A'),
        'Œ' | 'Ø' => Some('O'),
        'Ł' => Some('L'),
        'Ð' | 'Đ' => Some('D'),
        'Þ' => Some('T'),
        'ẞ' => Some('S'),
        _ => None,
    };
    if let Some(mapped) = mapped {
        return mapped.to_string();
    }
    let folded: String = upper.nfd().filter(|c| !is_combining_mark(*c)).collect();
    if let Some(first) = folded.chars().next().filter(char::is_ascii_alphabetic) {
        first.to_string()
    } else {
        upper
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming_articles_do_not_consume_contractions_or_language_collisions() {
        for (language, title) in [
            ("de", "Den Beispiel"),
            ("de", "Dem Beispiel"),
            ("de", "Des Beispiel"),
            ("it", "Del Esempio"),
            ("it", "Della Esempio"),
            ("it", "Nel Esempio"),
            ("es", "Al Ejemplo"),
            ("fr", "Du Exemple"),
            ("pt", "Do Exemplo"),
            ("pt", "Dos Exemplos"),
            ("da", "Ei Eksempel"),
            ("nn", "En Eksempel"),
            ("en", "[The Example]"),
            ("en", "The (2001) (Extended)"),
        ] {
            assert_eq!(title_with_article(title, Some(language)), title);
        }
        assert_eq!(
            title_with_article("The A Example", Some("en")),
            "A Example, The"
        );
        assert_eq!(
            title_with_article("Un’Avventura", Some("ita")),
            "Avventura, Un’"
        );
        assert_eq!(
            title_with_article("L'exemple (Extended) (2001)", Some("FRE")),
            "exemple, L' (Extended) (2001)"
        );
        assert_eq!(
            title_with_article("Des exemples", Some("fr")),
            "exemples, Des"
        );
        assert_eq!(title_with_article("The Example", None), "The Example");
    }

    #[test]
    fn naming_articles_follow_the_title_language() {
        for (language, title, expected) in [
            ("eng", "The Example (2001)", "Example, The (2001)"),
            ("en", "A Lantern of Clouds", "Lantern of Clouds, A"),
            ("es", "A Ciegas", "A Ciegas"),
            ("es", "Los Ejemplos", "Ejemplos, Los"),
            ("pt-BR", "A Vida", "Vida, A"),
            ("es", "A Vida", "A Vida"),
            ("fr", "De nuages et d'or", "De nuages et d'or"),
            ("fr", "Les Exemples", "Exemples, Les"),
            ("it", "I Esempi", "Esempi, I"),
            ("en", "I, Example", "I, Example"),
            ("de", "Das Beispiel", "Beispiel, Das"),
            ("en", "Das Beispiel", "Das Beispiel"),
            ("en", "Die Quietly", "Die Quietly"),
            ("nl", "De Voorbeelden", "Voorbeelden, De"),
            ("sv", "Ett exempel", "exempel, Ett"),
            ("da", "Et eksempel", "eksempel, Et"),
            ("nb", "Ei bok", "bok, Ei"),
            ("nn", "Eit døme", "døme, Eit"),
            ("it", "L’Esempio (2001)", "Esempio, L’ (2001)"),
            ("en", "L'Esempio", "L'Esempio"),
            ("en", "The Example  (2001)", "Example, The (2001)"),
            ("fr", "Le (2019)", "Le (2019)"),
            ("en", "The", "The"),
            ("en", "\"The Example\"", "\"The Example\""),
            ("und", "The Example", "The Example"),
        ] {
            assert_eq!(
                title_with_article(title, Some(language)),
                expected,
                "{language}: {title}"
            );
        }
    }

    #[test]
    fn naming_initial_keeps_complete_unicode_graphemes() {
        for (title, expected) in [
            ("The Éxample", "E"),
            ("... Æther", "A"),
            ("Øresund", "O"),
            ("Łódź", "L"),
            ("ßeta", "S"),
            ("\"42 Stories\"", "4"),
            ("😀... привет", "П"),
            ("鋼の物語", "鋼"),
            ("\u{0915}\u{094d}\u{0937} Example", "क्ष"),
            ("عَالم", "عَ"),
            ("", "_"),
            ("...", "_"),
        ] {
            assert_eq!(
                title_first_character(title, Some("en")),
                expected,
                "{title}"
            );
        }
    }
}
