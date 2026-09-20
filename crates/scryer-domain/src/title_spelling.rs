//! In-memory title comparisons. These are deliberately independent of persisted
//! catalog ordering: sorting may remove articles, identity matching must not.

use icu_collator::{
    Collator, CollatorBorrowed,
    options::{CollatorOptions, Strength},
};
use icu_locale::Locale;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpellingEquivalence {
    Exact,
    Locale(&'static str),
    Different,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TitleScript {
    Latin,
    Cyrillic,
    Cjk,
    Other,
}

impl TitleScript {
    /// Stable spelling for a persisted column. Parsed back by
    /// [`TitleScript::parse`], so the two must move together.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Latin => "latin",
            Self::Cyrillic => "cyrillic",
            Self::Cjk => "cjk",
            Self::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "latin" => Self::Latin,
            "cyrillic" => Self::Cyrillic,
            "cjk" => Self::Cjk,
            _ => Self::Other,
        }
    }
}

/// `Title (Year)` and bare `Title` are the same identity for collision
/// purposes: the matching loop bridges the two shapes, so the collision
/// detector and the persisted collision key must too, or a year-suffixed
/// alias reads as "unique".
pub fn strip_trailing_year(key: &str) -> &str {
    if let Some((head, tail)) = key.rsplit_once(' ')
        && tail.len() == 4
        && tail.chars().all(|c| c.is_ascii_digit())
        && (tail.starts_with("19") || tail.starts_with("20"))
        && !head.is_empty()
    {
        return head;
    }
    key
}

/// The form a name is *compared* in, and the year it then carries.
///
/// A name that ends in its own year (`Tide Chart 2023`) is compared without it
/// and asserts that year; every other name is compared whole and inherits the
/// title's year. The trailing four digits are only read as a year when they
/// agree with the title's own year, or when the name is not simply the title's
/// name with a year glued on — otherwise `Blade Runner 2049` would lose its
/// number.
///
/// One function because two places need the same answer: the persisted search
/// projection stores this form, and the matcher compares against it. A
/// disagreement between them is a silent lookup miss.
pub fn title_match_form(
    name: &str,
    title_name: &str,
    title_year: Option<i32>,
) -> (String, Option<i32>) {
    let key = title_lookup_form(name);
    let stripped = strip_trailing_year(&key);
    let canonical = title_lookup_form(title_name);
    let canonical_shape = strip_trailing_year(&canonical);
    let explicit_year = (stripped != key)
        .then(|| key.rsplit_once(' ').and_then(|(_, year)| year.parse().ok()))
        .flatten()
        .filter(|year| {
            Some(*year) == title_year || (key != canonical && stripped != canonical_shape)
        });
    match explicit_year {
        Some(year) => (stripped.to_string(), Some(year)),
        None => (key, title_year),
    }
}

pub fn title_script(value: &str) -> TitleScript {
    let mut script = None;
    for ch in value.chars().filter(|ch| ch.is_alphabetic()) {
        let current = match ch as u32 {
            0x41..=0x7a | 0xc0..=0x24f | 0x1e00..=0x1eff => TitleScript::Latin,
            0x400..=0x52f => TitleScript::Cyrillic,
            0x1100..=0x11ff
            | 0x2e80..=0x2fff
            | 0x3040..=0x30ff
            | 0x3100..=0x318f
            | 0x31a0..=0x31ff
            | 0x3400..=0x9fff
            | 0xa960..=0xa97f
            | 0xac00..=0xd7ff
            | 0xf900..=0xfaff
            | 0x20000..=0x323af => TitleScript::Cjk,
            _ => TitleScript::Other,
        };
        script = Some(match (script, current) {
            (None, next) => next,
            (Some(previous), next) if previous == next => next,
            (Some(TitleScript::Latin), TitleScript::Cjk)
            | (Some(TitleScript::Cjk), TitleScript::Latin) => TitleScript::Cjk,
            _ => TitleScript::Other,
        });
    }
    script.unwrap_or(TitleScript::Other)
}

/// Preserve diacritics and native letters; normalize only representation and
/// word separators. Remaining combining marks belong to the preceding letter.
pub fn normalize_title_spelling(value: &str) -> String {
    let mut result = String::new();
    for ch in value.nfkc().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() || is_combining_mark(ch) {
            result.push(ch);
        } else if (ch.is_whitespace()
            || matches!(
                ch,
                '.' | ','
                    | ':'
                    | ';'
                    | '-'
                    | '_'
                    | '/'
                    | '\\'
                    | '&'
                    | '+'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '\''
                    | '"'
                    | '!'
                    | '?'
                    | '~'
                    | '’'
                    | '‘'
                    | '“'
                    | '”'
                    | '–'
                    | '—'
                    | '−'
                    | '・'
                    | '。'
                    | '、'
            ))
            && !result.ends_with(' ')
            && !result.is_empty()
        {
            result.push(' ');
        }
    }
    result.trim().to_string()
}

/// Articles a catalog writes at the end of a name (`Lantern, The`). The
/// lookup form moves them back to the front so both spellings are one key.
const TRAILING_ARTICLES: &[&str] = &["a", "an", "the"];

/// The catalog's lookup form for one name: [`normalize_title_spelling`] with a
/// trailing article moved to the front.
///
/// This is *the* normalizer. Release/import resolution keys its identities on
/// this form, the persisted search projection stores it verbatim, and the UI's
/// lenient form ([`title_search_lenient_form`]) is derived from it rather than
/// computed by a second routine. Diacritics and native letters survive: two
/// spellings that differ only by an accent are equated by collation, not by
/// throwing the accent away.
///
/// Distinct from `catalog_sort_key`, which *drops* leading articles for
/// display ordering. Reordering is reversible and identity-preserving;
/// dropping is not.
pub fn title_lookup_form(value: &str) -> String {
    let mut tokens = normalize_title_spelling(value)
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if tokens.len() < 2 {
        return tokens.join(" ");
    }
    if let Some(article) = tokens.last().cloned()
        && TRAILING_ARTICLES.contains(&article.as_str())
    {
        tokens.pop();
        let mut reordered = vec![article];
        reordered.extend(tokens);
        return reordered.join(" ");
    }
    tokens.join(" ")
}

/// The form a person typing into the library search box is matched against:
/// [`normalize_title_spelling`] with diacritics folded away and two
/// affordances a keyboard needs.
///
/// The deliberate differences from [`title_lookup_form`]:
///
/// * Combining marks are dropped (NFD, then discard), so `muller` finds
///   `Müller` without the typist reaching for an umlaut. The lookup form keeps
///   them, because `ano` and `año` are different words and identity matching
///   must not conflate them.
/// * `ß` becomes `ss`. NFD leaves it alone — it has no decomposition — so a
///   searcher typing `Strasse` would otherwise never reach `Straße` in this
///   lane. The lookup form keeps `ß`; the German phonebook collation key is
///   what equates the two spellings for identity matching.
/// * `&` becomes the word `and`, because that is what people type.
/// * Every other symbol the normalizer does not list as a separator (`#`,
///   `%`, `@`, …) becomes a space rather than vanishing, so `Title#2` is two
///   tokens to a searcher. The lookup form leaves them out entirely; changing
///   that would move every resolver key.
/// * Runs of single characters are joined (`s h i e l d` -> `shield`), so an
///   initialism typed either way finds the title.
///
/// No article reordering: a searcher typing `lantern` expects a prefix hit on
/// `Lantern, The`, and reordering would demote it to a substring hit.
pub fn title_search_lenient_form(value: &str) -> String {
    let mut widened = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch == '&' {
            widened.push_str(" and ");
        } else if ch.is_alphanumeric() || is_combining_mark(ch) || ch.is_whitespace() {
            widened.push(ch);
        } else {
            widened.push(' ');
        }
    }
    let mut stripped = String::with_capacity(widened.len());
    for ch in normalize_title_spelling(&widened)
        .nfd()
        .filter(|ch| !is_combining_mark(*ch))
    {
        // `normalize_title_spelling` has already lowercased, so `ẞ` arrives
        // here as `ß`.
        if ch == 'ß' {
            stripped.push_str("ss");
        } else {
            stripped.push(ch);
        }
    }
    collapse_initialisms(&stripped)
}

fn collapse_initialisms(raw: &str) -> String {
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let is_initial = |token: &str| {
        token.chars().count() == 1 && token.chars().next().is_some_and(char::is_alphanumeric)
    };
    let mut collapsed: Vec<String> = Vec::with_capacity(tokens.len());
    let mut index = 0usize;
    while index < tokens.len() {
        if !is_initial(tokens[index]) {
            collapsed.push(tokens[index].to_string());
            index += 1;
            continue;
        }
        let start = index;
        while index < tokens.len() && is_initial(tokens[index]) {
            index += 1;
        }
        if index - start >= 2 {
            collapsed.push(tokens[start..index].concat());
        } else {
            collapsed.push(tokens[start].to_string());
        }
    }
    collapsed.join(" ")
}

/// Words that introduce a lower-case Roman numeral in a title.
const NUMERAL_CONTEXT_WORDS: &[&str] = &["part", "season", "chapter", "vol"];

/// Every number a name carries, in the shape the resolver guards on: bare
/// digit runs, plus Roman numerals tagged so `II` cannot be edited into `I`.
///
/// **Pass the name as written.** The Roman-numeral rule reads letter case, so
/// a lowercased lookup form answers differently from the source spelling, and
/// the two sides of one comparison must be fed the same way. The digit half
/// is case-free, so it does not care.
///
/// A token counts as a Roman numeral only when the Roman pattern matches *and*
/// one of these holds:
///
/// * every letter in it is upper case in the source — `Rocky II`, `Part III`;
/// * it is a run of one `i`, `v` or `x` directly after `part`, `season`,
///   `chapter` or `vol` — `part ii`, `season iv` is not a run and is caught by
///   the upper-case rule instead when written `IV`.
///
/// Without that, the pattern alone reads ordinary words as numerals: `mix` is
/// a valid Roman numeral (1009), and a spurious number in the guard splits a
/// title from its own aliases. Known residue: a name shouted in full upper
/// case (`MIX`) still reads as a numeral, because at that point the source
/// carries no signal to tell the two apart.
///
/// NFKC has already folded Unicode Roman numerals into the ASCII spelling by
/// the time a name reaches this.
pub fn title_numbers(value: &str) -> Vec<String> {
    static ROMAN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^m{0,3}(cm|cd|d?c{0,3})(xc|xl|l?x{0,3})(ix|iv|v?i{0,3})$")
            .expect("valid Roman numeral pattern")
    });

    fn word(token: &str) -> &str {
        token.trim_matches(|ch: char| !ch.is_alphanumeric())
    }

    let tokens = value.split_whitespace().collect::<Vec<_>>();
    let mut romans = Vec::new();
    for (position, token) in tokens.iter().enumerate() {
        let token = word(token);
        if token.is_empty() {
            continue;
        }
        let lowered = token.to_lowercase();
        if !ROMAN.is_match(&lowered) {
            continue;
        }
        let shouted = token
            .chars()
            .all(|ch| !ch.is_alphabetic() || ch.is_uppercase());
        let repeated_letter = lowered
            .chars()
            .next()
            .is_some_and(|first| matches!(first, 'i' | 'v' | 'x'))
            && lowered.chars().all(|ch| Some(ch) == lowered.chars().next());
        let after_context = position > 0
            && NUMERAL_CONTEXT_WORDS
                .iter()
                .any(|marker| word(tokens[position - 1]).eq_ignore_ascii_case(marker));
        if shouted || (repeated_letter && after_context) {
            romans.push(format!("roman:{lowered}"));
        }
    }

    value
        .split(|ch: char| !ch.is_numeric())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .chain(romans)
        .collect()
}

/// [`title_numbers`] as one comparable string, for a persisted column and an
/// index. Order follows [`title_numbers`], which is the order the in-memory
/// guard compares, so equality of this key is equality of that guard.
///
/// Takes the name as written, for the reason [`title_numbers`] gives.
pub fn title_numbers_key(value: &str) -> String {
    title_numbers(value).join("\u{1f}")
}

/// Fingerprint of the collation data this build will produce sort keys with.
///
/// Persisting [`title_spelling_key`] output is only sound while the ICU/CLDR
/// data behind it is unchanged: an `icu_collator` bump can silently move every
/// stored key, and a lookup computed with new data would then miss rows
/// written with the old. Rather than trusting a hand-maintained constant, this
/// hashes the actual sort keys of a probe corpus across every profile the
/// catalog uses. Any change to the data, the strength options, or the profile
/// list moves the fingerprint, and the consumer that stamped its rows with the
/// old one rebuilds them.
///
/// This is what makes persisting [`title_spelling_key`] output sound: the
/// keys are stored *with* this stamp, and a mismatch is a rebuild rather than
/// a silent miss.
///
/// Limitation: the probes are a sample, so a data change that leaves every
/// probe's key byte-identical while moving some other name's key is not
/// detected. The dependency versions below narrow that: the build script
/// reads `icu_collator` and `icu_collator_data` out of `Cargo.lock` and they
/// go into the hash, so a crate bump moves the fingerprint whether or not the
/// probes notice. What neither covers is a data change with no version change,
/// which the registry does not permit for a published crate.
pub fn title_collation_data_version() -> &'static str {
    static VERSION: LazyLock<String> = LazyLock::new(|| {
        const PROBES: &[&str] = &[
            "muller",
            "müller",
            "strasse",
            "straße",
            "grüße",
            "le cœur de chloé",
            "майский вечер",
            "流浪地球2",
            "ガラスの城",
            "한글",
        ];
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"title-spelling-collation-v1");
        hasher.update(env!("SCRYER_ICU_COLLATOR_VERSIONS").as_bytes());
        for tag in COLLATION_PROFILES {
            hasher.update(tag.as_bytes());
            for probe in PROBES {
                match title_spelling_key(probe, tag) {
                    Some(key) => {
                        hasher.update(&(key.len() as u32).to_le_bytes());
                        hasher.update(&key);
                    }
                    None => {
                        hasher.update(b"\xff");
                    }
                }
            }
        }
        hasher.finalize().to_hex()[..16].to_string()
    });
    VERSION.as_str()
}

/// Every profile tag [`title_spelling_profiles`] can return. Kept next to it:
/// a new tag there must be added here or the fingerprint stops covering it.
pub const COLLATION_PROFILES: &[&str] = &[
    "en",
    "de",
    "fr",
    "es",
    "it",
    "pt",
    "ru",
    "ja",
    "ko",
    "zh",
    "und",
    "de-u-co-phonebk",
];

type MatchCollator = Arc<CollatorBorrowed<'static>>;
static COLLATOR_CACHE: LazyLock<Mutex<HashMap<&'static str, MatchCollator>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn collator(tag: &'static str) -> Option<MatchCollator> {
    if let Some(cached) = COLLATOR_CACHE.lock().ok()?.get(tag) {
        return Some(cached.clone());
    }
    let mut options = CollatorOptions::default();
    options.strength = Some(match tag {
        "ru" => Strength::Secondary,
        "ja" | "ko" | "zh" => Strength::Tertiary,
        _ => Strength::Primary,
    });
    let locale: Locale = tag.parse().ok()?;
    let value = Arc::new(Collator::try_new(locale.into(), options).ok()?);
    COLLATOR_CACHE.lock().ok()?.insert(tag, value.clone());
    Some(value)
}

fn profile(language: Option<&str>, script: TitleScript) -> Option<&'static str> {
    let language = language
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-");
    let root = language.split('-').next().unwrap_or("");
    match root {
        "en" | "eng" => Some("en"),
        "de" | "deu" | "ger" => Some("de"),
        "fr" | "fra" | "fre" => Some("fr"),
        "es" | "spa" => Some("es"),
        "it" | "ita" => Some("it"),
        "pt" | "por" | "pob" => Some("pt"),
        "ru" | "rus" => Some("ru"),
        "ja" | "jpn" => Some("ja"),
        "ko" | "kor" => Some("ko"),
        "zh" | "zho" | "chi" => Some("zh"),
        _ if script == TitleScript::Latin => Some("und"),
        _ => None,
    }
}

/// Profiles used for a catalog spelling. Discovery and proof must use the
/// same collations, including the German phonebook expansion fallback.
pub fn title_spelling_profiles(value: &str, language: Option<&str>) -> Vec<&'static str> {
    let script = title_script(value);
    let Some(tag) = profile(language, script) else {
        return Vec::new();
    };
    if script == TitleScript::Other
        || (script == TitleScript::Cyrillic && tag != "ru")
        || (script == TitleScript::Cjk && !matches!(tag, "ja" | "ko" | "zh"))
    {
        return Vec::new();
    }
    let mut profiles = vec![tag];
    if script == TitleScript::Latin && (tag == "de" || value.contains(['ä', 'ö', 'ü'])) {
        profiles.push("de-u-co-phonebk");
    }
    profiles
}

/// Lookup key for spelling discovery. Never use catalog-sort keys here: their
/// article handling has different semantics.
///
/// These bytes are only comparable against keys written by the same collation
/// data. Persisting them is sound only alongside
/// [`title_collation_data_version`], which fingerprints that data so a
/// projection written by another build is rebuilt rather than silently
/// mis-compared; `title_search_meta.collation_version` is where the projection
/// records it.
pub fn title_spelling_key(value: &str, profile: &'static str) -> Option<Vec<u8>> {
    let mut key = Vec::new();
    collator(profile)?.write_sort_key_to(value, &mut key).ok()?;
    Some(key)
}

/// `None` means no supported profile/data; callers must not turn that failure
/// into an unguarded fuzzy match. Inputs are complete normalized title strings.
pub fn compare_title_spelling(
    left: &str,
    right: &str,
    language: Option<&str>,
) -> Option<SpellingEquivalence> {
    if left == right {
        return Some(SpellingEquivalence::Exact);
    }
    let script = title_script(right);
    if title_script(left) != script || script == TitleScript::Other {
        return None;
    }
    let profiles = title_spelling_profiles(right, language);
    if profiles.is_empty() {
        return None;
    }
    for tag in profiles {
        if collator(tag)?.compare(left, right).is_eq() {
            return Some(SpellingEquivalence::Locale(tag));
        }
    }
    if let Some(right_key) = japanese_romanization_key(right, language)
        && romanized_japanese_spelling(left) == right_key
    {
        return Some(SpellingEquivalence::Locale(JAPANESE_ROMANIZATION_TAG));
    }
    Some(SpellingEquivalence::Different)
}

/// The comparison-only romanization key of a Japanese-romanized name, or
/// `None` when the language tag does not mark the name as one. Indexes that
/// need to find every spelling of a name must key on this as well as on the
/// literal and collation keys: romanization variance is not a bounded edit
/// distance, so a Levenshtein-shaped candidate filter can miss it.
pub fn japanese_romanization_key(value: &str, language: Option<&str>) -> Option<String> {
    (title_script(value) == TitleScript::Latin && is_japanese_romanization(language))
        .then(|| romanized_japanese_spelling(value))
}

/// The locale reported for two spellings that agree only once romanization
/// variance is folded away.
pub const JAPANESE_ROMANIZATION_TAG: &str = "ja-latn";

/// Whether a catalog language tag marks a Latin-script name as a romanization
/// of a Japanese one. Catalogs write these as `ja`, `jpn`, `ja-Latn`, or the
/// AniDB/TVDB transliteration tag `x-jat`.
fn is_japanese_romanization(language: Option<&str>) -> bool {
    let language = language
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-");
    let root = language.split('-').next().unwrap_or("");
    matches!(root, "ja" | "jpn") || language.starts_with("x-jat")
}

/// Fold the romanization variance a catalog and a release group can each pick
/// for one Japanese name: long vowels written with a macron, doubled, or bare
/// (`Gasshō` / `Gasshou` / `Gassho`), the `wo`/`o` particle, and `m` before a
/// labial (`Shimbun` / `Shinbun`).
///
/// This is a comparison-only reduction. It is applied to both sides of one
/// comparison and only to Japanese-romanized names, so it can equate two
/// spellings of the same name and nothing else; the caller still has to prove
/// no other library identity answers to that spelling.
fn romanized_japanese_spelling(value: &str) -> String {
    let words = value
        .split_whitespace()
        .map(|word| if word == "wo" { "o" } else { word })
        .collect::<Vec<_>>()
        .join(" ");
    let characters = words.chars().collect::<Vec<_>>();
    let mut folded = String::with_capacity(words.len());
    let mut index = 0;
    while index < characters.len() {
        let character = match characters[index] {
            'ā' => 'a',
            'ī' => 'i',
            'ū' => 'u',
            'ē' => 'e',
            'ō' => 'o',
            other => other,
        };
        match (character, characters.get(index + 1).copied()) {
            ('o', Some('u' | 'o')) => {
                folded.push('o');
                index += 2;
                continue;
            }
            ('u', Some('u')) => {
                folded.push('u');
                index += 2;
                continue;
            }
            ('m', Some('b' | 'p')) => {
                folded.push('n');
                index += 1;
                continue;
            }
            _ => {}
        }
        folded.push(character);
        index += 1;
    }
    folded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_romanization_variance_is_one_spelling() {
        for (left, right) in [
            (
                "hagane no renkinjutsushi saigo no gasshou wo utau toki no hikari to kage no uta",
                "hagane no renkinjutsushi saigo no gassho o utau toki no hikari to kage no uta",
            ),
            ("yuusha no shimbun", "yusha no shinbun"),
            ("toukyou monogatari", "tōkyō monogatari"),
        ] {
            for language in ["x-jat", "ja", "jpn", "ja-Latn"] {
                assert_eq!(
                    compare_title_spelling(left, right, Some(language)),
                    Some(SpellingEquivalence::Locale(JAPANESE_ROMANIZATION_TAG)),
                    "{language}: {left} / {right}"
                );
            }
        }
    }

    #[test]
    fn roman_numerals_are_read_from_the_source_spelling() {
        // Upper case in the source is the signal.
        assert_eq!(title_numbers("Rocky II"), vec!["roman:ii".to_string()]);
        assert_eq!(title_numbers("Part III"), vec!["roman:iii".to_string()]);
        assert_eq!(title_numbers("Season IV"), vec!["roman:iv".to_string()]);
        // Lower case needs a counting word in front of a single-letter run.
        assert_eq!(title_numbers("part ii"), vec!["roman:ii".to_string()]);
        assert_eq!(title_numbers("vol iii"), vec!["roman:iii".to_string()]);
        assert_eq!(title_numbers("chapter x"), vec!["roman:x".to_string()]);
        // Ordinary words are not numerals, whatever the pattern says. `mix`
        // parses as 1009 and used to poison the guard.
        for word in [
            "Mix",
            "Did",
            "Mid",
            "Dim",
            "Civil",
            "The Mix Tape",
            "A Civil Action",
        ] {
            assert!(
                title_numbers(word).is_empty(),
                "{word} must not read as a numeral"
            );
        }
        // Neither is a lower-case numeral with no counting word.
        assert!(title_numbers("rocky ii").is_empty());
        // Digits never depend on case.
        assert_eq!(title_numbers("Rocky 4"), vec!["4".to_string()]);
        assert_eq!(title_numbers("Blade Runner 2049"), vec!["2049".to_string()]);
        // The guard that started all this: II cannot be edited into I.
        assert_ne!(title_numbers("Rocky II"), title_numbers("Rocky I"));
    }

    #[test]
    fn the_lenient_form_folds_eszett_and_the_lookup_form_keeps_it() {
        assert_eq!(title_search_lenient_form("Straße"), "strasse");
        assert_eq!(title_search_lenient_form("Strasse"), "strasse");
        assert_eq!(title_lookup_form("Straße"), "straße");
        assert_ne!(title_lookup_form("Strasse"), title_lookup_form("Straße"));
        // Folding is confined to ß; other German spellings stay distinct in
        // the lookup form and are equated by the phonebook collation instead.
        assert_eq!(title_search_lenient_form("Grüße"), "grusse");
    }

    #[test]
    fn romanization_folding_stays_off_other_languages_and_other_names() {
        // Folding is scoped to Japanese-romanized names.
        assert_eq!(
            compare_title_spelling("yuusha no shimbun", "yusha no shinbun", Some("eng")),
            Some(SpellingEquivalence::Different)
        );
        // And it never equates two different names.
        assert_eq!(
            compare_title_spelling("hikari no uta", "kage no uta", Some("x-jat")),
            Some(SpellingEquivalence::Different)
        );
    }

    #[test]
    fn multilingual_equivalents_and_distinctions() {
        for (language, left, right) in [
            ("en", "Ｔｈｅ　Harbor", "the harbor"),
            ("de", "Die zwei Paepste", "Die zwei Päpste"),
            ("de", "Goetter ueber der Strasse", "Götter über der Straße"),
            ("fr", "Le coeur de Chloe", "Le cœur de Chloé"),
            ("es", "El ultimo dia", "El último día"),
            ("it", "L’amore in città", "L'amore in citta"),
            ("pt", "Coracao de acucar", "Coração de açúcar"),
            ("ru", "Маи\u{306}скии\u{306} вечер", "Майский вечер"),
            ("zh", "流浪地球２", "流浪地球2"),
            ("ja", "ｶﾞﾗｽの城", "ガラスの城"),
            ("ko", "한글", "한글"),
        ] {
            let result = compare_title_spelling(
                &normalize_title_spelling(left),
                &normalize_title_spelling(right),
                Some(language),
            );
            assert!(
                matches!(
                    result,
                    Some(SpellingEquivalence::Exact | SpellingEquivalence::Locale(_))
                ),
                "{language}: {left} / {right}: {result:?}"
            );
        }
        for (language, left, right) in [
            ("es", "ano", "año"),
            ("ru", "маи", "май"),
            ("ja", "かく", "がく"),
            ("ja", "つき", "っき"),
            ("ko", "달", "탈"),
            ("zh", "大地", "天地"),
        ] {
            assert_eq!(
                compare_title_spelling(left, right, Some(language)),
                Some(SpellingEquivalence::Different),
                "{language}: {left} / {right}"
            );
        }
        assert_eq!(compare_title_spelling("harbor", "hаrbor", Some("en")), None);
        assert_eq!(compare_title_spelling("かく", "がく", Some("ru")), None);
        assert_eq!(compare_title_spelling("маи", "май", Some("ja")), None);
    }
}
