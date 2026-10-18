//! The long file names a grab's NZB or torrent carries, kept so an import can
//! still read a name the download client had to cut to fit the filesystem.
//!
//! Scryer holds the NZB or torrent only until the client takes it. Before it
//! goes, the router reads the names long enough to be at risk and they are
//! stored with the grab. At import a file whose name looks cut is matched back
//! to one of them only when its name is literally the start of that name; a
//! file the client renamed (par2, deobfuscation, an operator's rename) no
//! longer starts that way and keeps the name the client gave it.

use std::borrow::Cow;

use quick_xml::{Reader, events::Event};
use scryer_domain::PostedFileName;
use serde::{Deserialize, Serialize};

/// Download clients cut a file name to fit their own limit (Weaver at 240
/// bytes, SABnzbd at 245), and the cut lands just under it once a split
/// character and trailing separators are dropped. A name at least this long
/// was probably cut.
pub const LIKELY_CUT_FILE_NAME_MIN_BYTES: usize = 224;

/// A grab keeps at most this many names; a release with more long names than
/// this is not one the import can sort out by name anyway.
const MAX_GRABBED_FILE_NAMES: usize = 512;

/// An NZB's segment sizes count the encoded article, a little larger than the
/// file it decodes to. A file this much smaller than its posted size, or any
/// larger, is not that file.
const MIN_DECODED_SHARE_OF_POSTED_SIZE: f64 = 0.95;

pub fn file_name_looks_cut(name: &str) -> bool {
    name.len() >= LIKELY_CUT_FILE_NAME_MIN_BYTES
}

/// One file name a grab's NZB or torrent carried that a client might cut.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrabbedFileName {
    pub name: String,
    /// The torrent's own size for the file, exact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_size_bytes: Option<u64>,
    /// The NZB's total segment size for the file: the encoded articles, so
    /// slightly more than the file itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted_size_bytes: Option<u64>,
}

/// The long names an NZB's file subjects carry, each with its posted size.
///
/// A subject names its file in double quotes (`[01/12] - "name.mkv" yEnc
/// (1/50)`); a subject without one names no file and is skipped. An NZB that
/// does not parse yields what was read before the fault.
pub fn long_file_names_from_nzb(bytes: &[u8]) -> Vec<GrabbedFileName> {
    let mut reader = Reader::from_reader(bytes);
    let mut buffer = Vec::new();
    let mut names = Vec::new();
    let mut current: Option<GrabbedFileName> = None;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                match local_name(element.name().as_ref()) {
                    "file" => {
                        current = element
                            .attributes()
                            .flatten()
                            .find(|attribute| local_name(attribute.key.as_ref()) == "subject")
                            .and_then(|attribute| {
                                file_name_from_subject(&attribute_text(&attribute))
                            })
                            .filter(|name| file_name_looks_cut(name))
                            .map(|name| GrabbedFileName {
                                name,
                                exact_size_bytes: None,
                                posted_size_bytes: Some(0),
                            });
                    }
                    "segment" => {
                        if let Some(file) = current.as_mut() {
                            let size = element
                                .attributes()
                                .flatten()
                                .find(|attribute| local_name(attribute.key.as_ref()) == "bytes")
                                .and_then(|attribute| {
                                    attribute_text(&attribute).trim().parse::<u64>().ok()
                                })
                                .unwrap_or(0);
                            file.posted_size_bytes = file
                                .posted_size_bytes
                                .map(|total| total.saturating_add(size));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(element)) if local_name(element.name().as_ref()) == "file" => {
                if let Some(mut file) = current.take() {
                    file.posted_size_bytes = file.posted_size_bytes.filter(|size| *size > 0);
                    names.push(file);
                    if names.len() >= MAX_GRABBED_FILE_NAMES {
                        break;
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buffer.clear();
    }
    names
}

/// The long names a torrent's file list carries, each with its exact size.
///
/// Only the last path element is a file name a client writes; a torrent that
/// does not decode yields nothing.
pub fn long_file_names_from_torrent(bytes: &[u8]) -> Vec<GrabbedFileName> {
    let Some((Bencode::Dict(root), _)) = Bencode::parse(bytes, 0) else {
        return Vec::new();
    };
    let Some(Bencode::Dict(info)) = dict_get(&root, b"info") else {
        return Vec::new();
    };
    let mut files = Vec::new();
    match dict_get(info, b"files") {
        Some(Bencode::List(entries)) => {
            for entry in entries {
                let Bencode::Dict(entry) = entry else {
                    continue;
                };
                let path = dict_get(entry, b"path.utf-8").or_else(|| dict_get(entry, b"path"));
                let Some(Bencode::List(path)) = path else {
                    continue;
                };
                let Some(Bencode::Bytes(name)) = path.last() else {
                    continue;
                };
                files.push((*name, dict_get(entry, b"length")));
            }
        }
        _ => {
            let name = dict_get(info, b"name.utf-8").or_else(|| dict_get(info, b"name"));
            if let Some(Bencode::Bytes(name)) = name {
                files.push((*name, dict_get(info, b"length")));
            }
        }
    }
    files
        .into_iter()
        .filter_map(|(name, length)| {
            let name = std::str::from_utf8(name).ok()?;
            file_name_looks_cut(name).then(|| GrabbedFileName {
                name: name.to_string(),
                exact_size_bytes: match length {
                    Some(Bencode::Int(length)) => u64::try_from(*length).ok(),
                    _ => None,
                },
                posted_size_bytes: None,
            })
        })
        .take(MAX_GRABBED_FILE_NAMES)
        .collect()
}

/// Pair each file whose name looks cut with the grabbed name it was cut from.
///
/// `files` are the file names on disk with their sizes. A file takes a grabbed
/// name only when its own name, less the extension, is the start of that name
/// with the same extension: a client that renamed the file wrote something
/// else and keeps its name. Several grabbed names starting the same way (a
/// season pack whose shared title runs past the cut) are told apart by size,
/// and a file still matching more than one, or a grabbed name claimed by more
/// than one file, takes none.
pub fn match_cut_file_names(
    grabbed: &[GrabbedFileName],
    files: &[(String, u64)],
) -> Vec<PostedFileName> {
    let mut pairs: Vec<(usize, PostedFileName)> = Vec::new();
    for (name, size) in files {
        if !file_name_looks_cut(name) {
            continue;
        }
        let candidates: Vec<usize> = grabbed
            .iter()
            .enumerate()
            .filter(|(_, grabbed)| is_cut_of(name, &grabbed.name))
            .map(|(index, _)| index)
            .collect();
        let chosen = match candidates.as_slice() {
            [only] => Some(*only),
            [] => None,
            several => {
                let by_size: Vec<usize> = several
                    .iter()
                    .copied()
                    .filter(|index| size_fits(&grabbed[*index], *size))
                    .collect();
                match by_size.as_slice() {
                    [only] => Some(*only),
                    _ => None,
                }
            }
        };
        if let Some(index) = chosen {
            pairs.push((
                index,
                PostedFileName {
                    name: name.clone(),
                    posted_name: grabbed[index].name.clone(),
                },
            ));
        }
    }
    pairs
        .iter()
        .filter(|(index, _)| pairs.iter().filter(|(other, _)| other == index).count() == 1)
        .map(|(_, pair)| pair.clone())
        .collect()
}

fn size_fits(grabbed: &GrabbedFileName, size: u64) -> bool {
    if let Some(exact) = grabbed.exact_size_bytes {
        return exact == size;
    }
    grabbed.posted_size_bytes.is_some_and(|posted| {
        size <= posted && size as f64 >= posted as f64 * MIN_DECODED_SHARE_OF_POSTED_SIZE
    })
}

/// Is `on_disk` what a client leaves of `grabbed` after cutting it?
///
/// Both keep the extension. The stem on disk must be a strict start of the
/// grabbed stem, reading any two characters that are neither letters nor
/// digits as alike, since clients replace characters a filesystem refuses.
/// SABnzbd numbers a second file cut to the same name (`name.1.mkv`), so a
/// trailing number is allowed to be that.
fn is_cut_of(on_disk: &str, grabbed: &str) -> bool {
    let (Some((disk_stem, disk_ext)), Some((grabbed_stem, grabbed_ext))) =
        (split_extension(on_disk), split_extension(grabbed))
    else {
        return false;
    };
    if !disk_ext.eq_ignore_ascii_case(grabbed_ext) || grabbed.len() <= on_disk.len() {
        return false;
    }
    if starts_like(grabbed_stem, disk_stem) {
        return true;
    }
    disk_stem
        .rsplit_once('.')
        .filter(|(_, number)| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
        .is_some_and(|(stem, _)| starts_like(grabbed_stem, stem))
}

fn split_extension(name: &str) -> Option<(&str, &str)> {
    name.rsplit_once('.')
        .filter(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
}

fn starts_like(whole: &str, start: &str) -> bool {
    let start = start.trim_end_matches(|c: char| !c.is_alphanumeric());
    if start.is_empty() {
        return false;
    }
    let mut whole = whole.chars();
    start.chars().all(|expected| {
        whole.next().is_some_and(|actual| {
            actual == expected || (!actual.is_alphanumeric() && !expected.is_alphanumeric())
        })
    })
}

fn file_name_from_subject(subject: &str) -> Option<String> {
    let (_, rest) = subject.split_once('"')?;
    let (name, _) = rest.split_once('"')?;
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn attribute_text(attribute: &quick_xml::events::attributes::Attribute<'_>) -> String {
    attribute
        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
        .map(Cow::into_owned)
        .unwrap_or_else(|_| attribute.value.clone().into_owned())
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Just enough bencode to read a torrent's file list.
enum Bencode<'a> {
    Int(i64),
    Bytes(&'a [u8]),
    List(Vec<Bencode<'a>>),
    Dict(Vec<(&'a [u8], Bencode<'a>)>),
}

impl<'a> Bencode<'a> {
    const MAX_DEPTH: usize = 32;

    fn parse(bytes: &'a [u8], depth: usize) -> Option<(Self, &'a [u8])> {
        if depth > Self::MAX_DEPTH {
            return None;
        }
        match *bytes.first()? {
            b'i' => {
                let end = bytes.iter().position(|b| *b == b'e')?;
                let value = std::str::from_utf8(&bytes[1..end]).ok()?.parse().ok()?;
                Some((Self::Int(value), &bytes[end + 1..]))
            }
            b'l' => {
                let mut rest = &bytes[1..];
                let mut items = Vec::new();
                while *rest.first()? != b'e' {
                    let (item, next) = Self::parse(rest, depth + 1)?;
                    items.push(item);
                    rest = next;
                }
                Some((Self::List(items), &rest[1..]))
            }
            b'd' => {
                let mut rest = &bytes[1..];
                let mut entries = Vec::new();
                while *rest.first()? != b'e' {
                    let (Self::Bytes(key), next) = Self::parse(rest, depth + 1)? else {
                        return None;
                    };
                    let (value, next) = Self::parse(next, depth + 1)?;
                    entries.push((key, value));
                    rest = next;
                }
                Some((Self::Dict(entries), &rest[1..]))
            }
            b'0'..=b'9' => {
                let colon = bytes.iter().position(|b| *b == b':')?;
                let len: usize = std::str::from_utf8(&bytes[..colon]).ok()?.parse().ok()?;
                let start = colon + 1;
                let end = start.checked_add(len)?;
                Some((Self::Bytes(bytes.get(start..end)?), &bytes[end..]))
            }
            _ => None,
        }
    }
}

fn dict_get<'d, 'a>(entries: &'d [(&'a [u8], Bencode<'a>)], key: &[u8]) -> Option<&'d Bencode<'a>> {
    entries
        .iter()
        .find(|(entry_key, _)| *entry_key == key)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_stem(tail: &str) -> String {
        format!(
            "Harbor Pals - {} - {tail}",
            "A Very Long Subtitle That Keeps Going "
                .repeat(6)
                .trim_end()
        )
    }

    fn cut(name: &str, limit: usize) -> String {
        let (stem, ext) = name.rsplit_once('.').expect("extension");
        let keep = limit - ext.len() - 1;
        let mut end = keep.min(stem.len());
        while !stem.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}.{ext}", stem[..end].trim_end_matches([' ', '-', '.']))
    }

    fn nzb(files: &[(&str, &[u64])]) -> String {
        let mut body = String::from(
            "<?xml version=\"1.0\"?>\n<nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n",
        );
        for (index, (name, segments)) in files.iter().enumerate() {
            body.push_str(&format!(
                "<file poster=\"poster@example.invalid\" date=\"1700000000\" subject=\"[{}/{}] - &quot;{}&quot; yEnc (1/{})\">\n<groups><group>alt.binaries.example</group></groups>\n<segments>",
                index + 1,
                files.len(),
                name.replace('&', "&amp;"),
                segments.len()
            ));
            for (number, bytes) in segments.iter().enumerate() {
                body.push_str(&format!(
                    "<segment bytes=\"{bytes}\" number=\"{}\">part{number}@example.invalid</segment>",
                    number + 1
                ));
            }
            body.push_str("</segments>\n</file>\n");
        }
        body.push_str("</nzb>\n");
        body
    }

    fn bencode_bytes(value: &str) -> String {
        format!("{}:{value}", value.len())
    }

    #[test]
    fn an_nzb_keeps_only_its_long_names_with_their_posted_sizes() {
        let long = format!("{}.mkv", long_stem("07"));
        let body = nzb(&[
            (long.as_str(), &[700_000, 300_000]),
            ("Harbor.Pals.S01E07.par2", &[1_000]),
        ]);

        assert_eq!(
            long_file_names_from_nzb(body.as_bytes()),
            vec![GrabbedFileName {
                name: long,
                exact_size_bytes: None,
                posted_size_bytes: Some(1_000_000),
            }]
        );
    }

    #[test]
    fn an_nzb_that_does_not_parse_yields_nothing() {
        assert!(long_file_names_from_nzb(b"not an nzb at all").is_empty());
    }

    #[test]
    fn a_torrent_keeps_the_long_file_names_of_its_list_with_exact_sizes() {
        let long = format!("{}.mkv", long_stem("07"));
        let torrent = format!(
            "d4:infod5:filesld6:lengthi1234e4:pathl{}{}eed6:lengthi9e4:pathl{}eee4:name{}12:piece lengthi16384eee",
            bencode_bytes("Season 1"),
            bencode_bytes(&long),
            bencode_bytes("notes.txt"),
            bencode_bytes("Harbor Pals")
        );

        assert_eq!(
            long_file_names_from_torrent(torrent.as_bytes()),
            vec![GrabbedFileName {
                name: long,
                exact_size_bytes: Some(1234),
                posted_size_bytes: None,
            }]
        );
    }

    #[test]
    fn a_single_file_torrent_names_its_file_after_itself() {
        let long = format!("{}.mkv", long_stem("07"));
        let torrent = format!(
            "d4:infod6:lengthi42e4:name{}12:piece lengthi16384eee",
            bencode_bytes(&long)
        );

        assert_eq!(
            long_file_names_from_torrent(torrent.as_bytes()),
            vec![GrabbedFileName {
                name: long,
                exact_size_bytes: Some(42),
                posted_size_bytes: None,
            }]
        );
    }

    #[test]
    fn a_torrent_that_does_not_decode_yields_nothing() {
        assert!(long_file_names_from_torrent(b"d4:infod4:name").is_empty());
        assert!(long_file_names_from_torrent(b"").is_empty());
    }

    #[test]
    fn a_cut_name_takes_the_name_it_was_cut_from() {
        let posted = format!("{}.mkv", long_stem("07 [1080p]"));
        let on_disk = cut(&posted, 240);
        let grabbed = [GrabbedFileName {
            name: posted.clone(),
            exact_size_bytes: None,
            posted_size_bytes: None,
        }];

        assert_eq!(
            match_cut_file_names(&grabbed, &[(on_disk.clone(), 10)]),
            vec![PostedFileName {
                name: on_disk,
                posted_name: posted,
            }]
        );
    }

    #[test]
    fn a_name_the_client_renamed_keeps_that_name() {
        // Obfuscated subjects: par2 named the file, then the client cut it.
        let grabbed = [GrabbedFileName {
            name: format!("{}.mkv", "a1b2c3d4e5f6".repeat(20)),
            exact_size_bytes: None,
            posted_size_bytes: None,
        }];
        let renamed = cut(&format!("{}.mkv", long_stem("07")), 245);

        assert!(match_cut_file_names(&grabbed, &[(renamed, 10)]).is_empty());
    }

    #[test]
    fn a_name_that_is_not_cut_is_left_alone() {
        let posted = format!("{}.mkv", long_stem("07"));
        let grabbed = [GrabbedFileName {
            name: posted.clone(),
            exact_size_bytes: None,
            posted_size_bytes: None,
        }];

        assert!(match_cut_file_names(&grabbed, &[(posted, 10)]).is_empty());
        assert!(
            match_cut_file_names(&grabbed, &[("Harbor Pals - 07.mkv".to_string(), 10)]).is_empty()
        );
    }

    #[test]
    fn a_character_the_client_replaced_still_matches() {
        let posted = format!("{}.mkv", long_stem("07: Finale"));
        let on_disk = cut(&posted.replace(" - A Very", "_-_A Very"), 240);
        let grabbed = [GrabbedFileName {
            name: posted.clone(),
            exact_size_bytes: None,
            posted_size_bytes: None,
        }];

        assert_eq!(
            match_cut_file_names(&grabbed, &[(on_disk, 10)])
                .first()
                .map(|pair| pair.posted_name.clone()),
            Some(posted)
        );
    }

    #[test]
    fn pack_files_cut_to_one_name_are_told_apart_by_exact_size() {
        let seventh = format!("{}.mkv", long_stem("07"));
        let eighth = format!("{}.mkv", long_stem("08"));
        let on_disk = cut(&seventh, 240);
        assert_eq!(on_disk, cut(&eighth, 240));
        let grabbed = [
            GrabbedFileName {
                name: seventh.clone(),
                exact_size_bytes: Some(700),
                posted_size_bytes: None,
            },
            GrabbedFileName {
                name: eighth.clone(),
                exact_size_bytes: Some(800),
                posted_size_bytes: None,
            },
        ];
        let numbered = on_disk.replace(".mkv", ".1.mkv");

        let pairs = match_cut_file_names(&grabbed, &[(on_disk, 800), (numbered, 700)]);

        assert_eq!(
            pairs
                .iter()
                .map(|pair| pair.posted_name.as_str())
                .collect::<Vec<_>>(),
            vec![eighth.as_str(), seventh.as_str()]
        );
    }

    #[test]
    fn pack_files_too_close_in_posted_size_take_no_name() {
        let seventh = format!("{}.mkv", long_stem("07"));
        let eighth = format!("{}.mkv", long_stem("08"));
        let on_disk = cut(&seventh, 240);
        let grabbed = [
            GrabbedFileName {
                name: seventh,
                exact_size_bytes: None,
                posted_size_bytes: Some(1_000_000),
            },
            GrabbedFileName {
                name: eighth,
                exact_size_bytes: None,
                posted_size_bytes: Some(1_010_000),
            },
        ];

        assert!(match_cut_file_names(&grabbed, &[(on_disk, 985_000)]).is_empty());
    }

    #[test]
    fn pack_files_apart_in_posted_size_are_told_apart() {
        let seventh = format!("{}.mkv", long_stem("07"));
        let eighth = format!("{}.mkv", long_stem("08"));
        let on_disk = cut(&seventh, 240);
        let grabbed = [
            GrabbedFileName {
                name: seventh.clone(),
                exact_size_bytes: None,
                posted_size_bytes: Some(1_000_000),
            },
            GrabbedFileName {
                name: eighth,
                exact_size_bytes: None,
                posted_size_bytes: Some(1_400_000),
            },
        ];

        assert_eq!(
            match_cut_file_names(&grabbed, &[(on_disk, 980_000)])
                .first()
                .map(|pair| pair.posted_name.clone()),
            Some(seventh)
        );
    }

    #[test]
    fn a_grabbed_name_claimed_by_two_files_goes_to_neither() {
        let posted = format!("{}.mkv", long_stem("07"));
        let on_disk = cut(&posted, 240);
        let grabbed = [GrabbedFileName {
            name: posted,
            exact_size_bytes: None,
            posted_size_bytes: None,
        }];
        let numbered = on_disk.replace(".mkv", ".1.mkv");

        assert!(match_cut_file_names(&grabbed, &[(on_disk, 10), (numbered, 10)]).is_empty());
    }
}
