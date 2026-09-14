//! Bounded release-name claims, independent of actual media stream properties.

use serde::{Deserialize, Serialize};

use crate::{ReleaseParseCandidate, SeparatorKind, Token, TokenRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StereoPresentation {
    TwoD,
    ThreeD,
    #[serde(rename = "mixed_2d_3d")]
    Mixed2d3d,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StereoLayout {
    SideBySide,
    TopBottom,
    FrameSequential,
    RowInterleaved,
    ColumnInterleaved,
    Checkerboard,
    Anaglyph,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StereoSampling {
    Half,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StereoEncoding {
    Mvc,
}

impl StereoPresentation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TwoD => "two_d",
            Self::ThreeD => "three_d",
            Self::Mixed2d3d => "mixed_2d_3d",
        }
    }
}

impl StereoLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SideBySide => "side_by_side",
            Self::TopBottom => "top_bottom",
            Self::FrameSequential => "frame_sequential",
            Self::RowInterleaved => "row_interleaved",
            Self::ColumnInterleaved => "column_interleaved",
            Self::Checkerboard => "checkerboard",
            Self::Anaglyph => "anaglyph",
        }
    }
}

impl StereoSampling {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Half => "half",
            Self::Full => "full",
        }
    }
}

impl StereoEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mvc => "mvc",
        }
    }
}

/// Only facts explicitly claimed by a release name. Unset attributes are unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedStereoscopy {
    pub presentation: StereoPresentation,
    pub layout: Option<StereoLayout>,
    pub sampling: Option<StereoSampling>,
    pub encoding: Option<StereoEncoding>,
}

impl ParsedStereoscopy {
    #[must_use]
    pub fn has_3d(&self) -> bool {
        matches!(
            self.presentation,
            StereoPresentation::ThreeD | StereoPresentation::Mixed2d3d
        )
    }
}

/// A complete claim and its original token range, retained even on conflict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StereoEvidence {
    pub tokens: TokenRange,
    pub claim: ParsedStereoscopy,
}

#[derive(Default)]
pub(crate) struct StereoAnalysis {
    pub metadata: Option<ParsedStereoscopy>,
    pub evidence: Vec<StereoEvidence>,
    pub hints: Vec<String>,
}

const MAX_MARKER_TOKENS: usize = 5;

pub(crate) fn analyze_candidate(
    tokens: &[Token],
    candidate: &ReleaseParseCandidate,
) -> StereoAnalysis {
    let last_title = crate::parse::protected_title_end(candidate);
    let leading_group = crate::parse::leading_release_group_token_range(tokens);
    let allowed = tokens
        .iter()
        .enumerate()
        .map(|(i, token)| {
            !leading_group.is_some_and(|range| i >= range.start_token && i < range.end_token)
                // A byte-truncated terminal token may be a prefix of a longer
                // word, such as `3DAnimation`, rather than a complete claim.
                && !(i + 1 == tokens.len()
                    && candidate.projected.raw_title.len() > crate::sanitize::MAX_INPUT_BYTES)
                && crate::parse::technical_recovery_token(candidate, i, token, last_title)
        })
        .collect::<Vec<_>>();
    analyze(tokens, &allowed)
}

/// Non-anchoring annotation suggestions. Candidate title/group protection is
/// applied separately, before any of these claims can enter the result.
pub(crate) fn marker_roles(tokens: &[Token]) -> Vec<bool> {
    let analysis = analyze(tokens, &vec![true; tokens.len()]);
    let mut roles = vec![false; tokens.len()];
    for evidence in analysis.evidence {
        roles[evidence.tokens.start_token..evidence.tokens.end_token].fill(true);
    }
    roles
}

fn same_scope(a: &Token, b: &Token) -> bool {
    a.group_id == b.group_id && a.bracket_depth == b.bracket_depth
}

fn excluded_dimension(tokens: &[Token], index: usize) -> bool {
    // These describe animation, audio, an event, image processing or a theatrical
    // presentation, rather than the representation of the released video.
    tokens.iter().skip(index + 1).take(3).any(|next| {
        same_scope(&tokens[index], next)
            && matches!(
                next.normalized.as_str(),
                "CG" | "CGI"
                    | "CGANIMATION"
                    | "ANIMATION"
                    | "AUDIO"
                    | "NOISE"
                    | "LIVE"
                    | "STREAM"
                    | "GRADUATION"
                    | "MODEL"
                    | "MODELS"
                    | "CAMRIP"
            )
    }) || index.checked_sub(1).is_some_and(|previous| {
        same_scope(&tokens[index], &tokens[previous]) && tokens[previous].normalized == "IMAX"
    })
}

fn suppressed_2d(tokens: &[Token], index: usize) -> bool {
    let start = index.saturating_sub(3);
    tokens[start..index].iter().any(|previous| {
        same_scope(previous, &tokens[index])
            && matches!(previous.normalized.as_str(), "VIEWABLE" | "COMPATIBLE")
    })
}

fn dimension(presentation: StereoPresentation) -> ParsedStereoscopy {
    ParsedStereoscopy {
        presentation,
        layout: None,
        sampling: None,
        encoding: None,
    }
}

fn layout(layout: StereoLayout, sampling: Option<StereoSampling>) -> ParsedStereoscopy {
    ParsedStereoscopy {
        layout: Some(layout),
        sampling,
        ..dimension(StereoPresentation::ThreeD)
    }
}

fn has_dimension_in_scope(tokens: &[Token], allowed: &[bool], index: usize) -> bool {
    tokens.iter().enumerate().any(|(i, token)| {
        allowed[i]
            && same_scope(token, &tokens[index])
            && !excluded_dimension(tokens, i)
            && matches!(
                token.normalized.as_str(),
                "3D" | "STEREOSCOPIC" | "BD3D" | "BLURAY3D"
            )
    })
}

fn mixed_movie_bundle(compact: &str) -> bool {
    // Collection components are complete technical labels, not arbitrary
    // substrings of a title: `OVA×3+Movie2D&3D+SP`.
    let mut mixed = false;
    for part in compact.split('+') {
        if matches!(part, "MOVIE2DAND3D" | "MOVIE3DAND2D") {
            mixed = true;
        } else if part != "SP" && part != "OVA" {
            // The lexer removes the multiplication glyph, retaining OVA3.
            let count = part
                .strip_prefix("OVAX")
                .or_else(|| part.strip_prefix("OVA"));
            if !count.is_some_and(|value| {
                !value.is_empty()
                    && value.len() <= 3
                    && value.bytes().all(|byte| byte.is_ascii_digit())
            }) {
                return false;
            }
        }
    }
    mixed
}

fn marker(
    tokens: &[Token],
    start: usize,
    end: usize,
    compact: &str,
    corroborated: bool,
) -> Option<ParsedStereoscopy> {
    use StereoLayout::*;
    use StereoPresentation::*;
    use StereoSampling::*;

    let mixed = matches!(
        compact,
        "2D+3D"
            | "3D+2D"
            | "4K+2D+3D"
            | "2DAND3D"
            | "3DAND2D"
            | "2DY3D"
            | "2DBD+3DBD"
            | "3DBD+2DBD"
    );
    let slash_mixed = matches!(compact, "2D3D" | "3D2D")
        && end == start + 2
        && tokens[start + 1].separator_before == SeparatorKind::Slash;
    if mixed || slash_mixed || mixed_movie_bundle(compact) {
        return Some(dimension(Mixed2d3d));
    }
    if compact == "2DTO3DCONVERSION" {
        return Some(dimension(ThreeD));
    }
    let value = match compact {
        "3D" | "STEREOSCOPIC" | "BLURAY3D" | "BD3D" if !excluded_dimension(tokens, start) => {
            dimension(ThreeD)
        }
        "2D" if !suppressed_2d(tokens, start) => dimension(TwoD),
        "MK3D"
            if start + 1 == tokens.len()
                && end == start + 1
                && tokens[start].separator_before == SeparatorKind::Dot =>
        {
            dimension(ThreeD)
        }
        "HSBS" | "HALFSBS" | "SBSHALF" | "HALFSIDEBYSIDE" | "SIDEBYSIDEHALF" => {
            layout(SideBySide, Some(Half))
        }
        "FSBS" | "FULLSBS" | "SBSFULL" | "FULLSIDEBYSIDE" | "SIDEBYSIDEFULL" => {
            layout(SideBySide, Some(Full))
        }
        "HOU" | "HTAB" | "HTB" | "HALFOU" | "HALFTAB" | "HALFTB" | "OUHALF" | "TABHALF"
        | "TBHALF" | "HALFTOPBOTTOM" | "HALFOVERUNDER" => layout(TopBottom, Some(Half)),
        "FOU" | "FTAB" | "FTB" | "FULLOU" | "FULLTAB" | "FULLTB" | "OUFULL" | "TABFULL"
        | "TBFULL" | "FULLTOPBOTTOM" | "FULLOVERUNDER" => layout(TopBottom, Some(Full)),
        "SIDEBYSIDE" => layout(SideBySide, None),
        "TOPBOTTOM" | "TOPANDBOTTOM" | "OVERUNDER" => layout(TopBottom, None),
        "SBS" if corroborated => layout(SideBySide, None),
        "OU" | "TAB" | "TB" if corroborated => layout(TopBottom, None),
        "FRAMESEQUENTIAL" | "FRAMEALTERNATIVE" | "FRAMEALTERNATE" => layout(FrameSequential, None),
        "FA" if corroborated => layout(FrameSequential, None),
        "ROWINTERLACED" | "ROWINTERLEAVED" => layout(RowInterleaved, None),
        "COLUMNINTERLACED" | "COLUMNINTERLEAVED" => layout(ColumnInterleaved, None),
        "CHECKERBOARD" if corroborated => layout(Checkerboard, None),
        "ANAGLYPH" => layout(Anaglyph, None),
        "MVC" | "AVCMVC" if corroborated => ParsedStereoscopy {
            encoding: Some(StereoEncoding::Mvc),
            ..dimension(ThreeD)
        },
        _ => return None,
    };
    Some(value)
}

fn analyze(tokens: &[Token], allowed: &[bool]) -> StereoAnalysis {
    let mut result = StereoAnalysis::default();
    let mut index = 0;
    while index < tokens.len() {
        if !allowed[index] || !could_start_marker(&tokens[index].normalized) {
            index += 1;
            continue;
        }
        let corroborated = has_dimension_in_scope(tokens, allowed, index);
        let mut compact = String::new();
        let mut longest = None;
        for end in index..tokens.len().min(index + MAX_MARKER_TOKENS) {
            if !allowed[end] || !same_scope(&tokens[index], &tokens[end]) {
                break;
            }
            // A pipe or other prose separator is not a compound word boundary.
            if end > index && matches!(tokens[end].separator_before, SeparatorKind::Other) {
                break;
            }
            compact.push_str(&tokens[end].normalized);
            if let Some(claim) = marker(tokens, index, end + 1, &compact, corroborated) {
                longest = Some((end + 1, claim));
            }
        }
        if let Some((end, claim)) = longest {
            result.evidence.push(StereoEvidence {
                tokens: TokenRange::new(index, end),
                claim,
            });
            index = end;
        } else {
            index += 1;
        }
    }
    resolve(&mut result);
    result
}

fn could_start_marker(token: &str) -> bool {
    if token.starts_with("MOVIE2D")
        || token.starts_with("MOVIE3D")
        || token.starts_with("OVA")
        || token.starts_with("SP+")
    {
        return true;
    }
    matches!(
        token,
        "3" | "3D"
            | "2D"
            | "STEREOSCOPIC"
            | "BLURAY3D"
            | "BD3D"
            | "MK3D"
            | "2D+3D"
            | "3D+2D"
            | "4K+2D+3D"
            | "2DAND3D"
            | "3DAND2D"
            | "2DY3D"
            | "2DBD+3DBD"
            | "3DBD+2DBD"
            | "2DTO3DCONVERSION"
            | "H"
            | "F"
            | "HALF"
            | "FULL"
            | "HSBS"
            | "FSBS"
            | "SBS"
            | "SIDE"
            | "HALFSBS"
            | "SBSHALF"
            | "HALFSIDEBYSIDE"
            | "SIDEBYSIDEHALF"
            | "FULLSBS"
            | "SBSFULL"
            | "FULLSIDEBYSIDE"
            | "SIDEBYSIDEFULL"
            | "SIDEBYSIDE"
            | "HOU"
            | "HTAB"
            | "HTB"
            | "HALFOU"
            | "HALFTAB"
            | "HALFTB"
            | "OUHALF"
            | "TABHALF"
            | "TBHALF"
            | "HALFTOPBOTTOM"
            | "HALFOVERUNDER"
            | "FOU"
            | "FTAB"
            | "FTB"
            | "FULLOU"
            | "FULLTAB"
            | "FULLTB"
            | "OUFULL"
            | "TABFULL"
            | "TBFULL"
            | "FULLTOPBOTTOM"
            | "FULLOVERUNDER"
            | "TOP"
            | "OVER"
            | "TOPBOTTOM"
            | "TOPANDBOTTOM"
            | "OVERUNDER"
            | "OU"
            | "TAB"
            | "TB"
            | "FRAME"
            | "FRAMESEQUENTIAL"
            | "FRAMEALTERNATIVE"
            | "FRAMEALTERNATE"
            | "FA"
            | "ROW"
            | "COLUMN"
            | "ROWINTERLACED"
            | "ROWINTERLEAVED"
            | "COLUMNINTERLACED"
            | "COLUMNINTERLEAVED"
            | "CHECKERBOARD"
            | "ANAGLYPH"
            | "MVC"
            | "AVCMVC"
            | "AVC"
    )
}

fn unique<T: Copy + PartialEq>(values: impl Iterator<Item = T>) -> (Option<T>, bool) {
    let mut selected = None;
    let mut conflict = false;
    for value in values {
        if selected.is_some_and(|old| old != value) {
            conflict = true;
        }
        selected = Some(value);
    }
    (if conflict { None } else { selected }, conflict)
}

fn resolve(result: &mut StereoAnalysis) {
    use StereoPresentation::*;
    let mixed = result
        .evidence
        .iter()
        .any(|e| e.claim.presentation == Mixed2d3d);
    let (presentation, conflict) = unique(result.evidence.iter().map(|e| e.claim.presentation));
    if conflict && !mixed {
        result.hints.push("stereo:presentation_conflict".into());
        return;
    }
    let Some(presentation) = (if mixed { Some(Mixed2d3d) } else { presentation }) else {
        return;
    };
    let (layout, layout_conflict) = unique(result.evidence.iter().filter_map(|e| e.claim.layout));
    let (sampling, sampling_conflict) =
        unique(result.evidence.iter().filter_map(|e| e.claim.sampling));
    let (encoding, encoding_conflict) =
        unique(result.evidence.iter().filter_map(|e| e.claim.encoding));
    for (conflict, hint) in [
        (layout_conflict, "stereo:layout_conflict"),
        (sampling_conflict, "stereo:sampling_conflict"),
        (encoding_conflict, "stereo:encoding_conflict"),
    ] {
        if conflict {
            result.hints.push(hint.into());
        }
    }
    let homogeneous = presentation == ThreeD;
    result.metadata = Some(ParsedStereoscopy {
        presentation,
        layout: homogeneous.then_some(layout).flatten(),
        sampling: (homogeneous
            && matches!(
                layout,
                Some(StereoLayout::SideBySide | StereoLayout::TopBottom)
            ))
        .then_some(sampling)
        .flatten(),
        encoding: homogeneous.then_some(encoding).flatten(),
    });
}
