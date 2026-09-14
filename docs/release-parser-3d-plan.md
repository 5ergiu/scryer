# Full 3D release-parser support

Status: implemented in the worktree; focused validation and handoff recorded below.

Prepared 2026-09-12 against `origin/release-0.20.0` at
`7eb26ef39b53dc40042d42ce04f37cdaf0509b30`, in the worktree
`.worktrees/release-parser-3d-plan`, branch `feature/release-parser-3d-plan`.
There was no `0.20.0` tag; the existing `release-0.20.0` branch is the base.

## Outcome and scope

Parse stereoscopic presentation, packing layout, and half/full sampling as
structured metadata across movie, series, and anime release names. Preserve
technical evidence through candidate selection, uncertain identity recovery,
application projections, and existing rule/API metadata boundaries. Recognize
explicit mixed 2D/3D releases without treating every item as the same format.

The parser describes claims in a release name. It cannot establish the actual
video representation, playback compatibility, or whether a theatrical 3D film
was captured in stereo. Absence of a marker means unknown, not verified 2D.

Scope includes the parser, focused regression fixtures, minimal propagation to
existing metadata consumers, and search-result deduplication compatibility.
Playback, media probing, transcoding, library persistence/backfills, renaming,
import/replacement policy, new quality preferences, and UI controls are separate
features. No third-party dependency changes are needed. The work ends with
implementation, focused validation, and handoff.

## Research performed

The local research archive is at
[`tmp/release-parser-3d`](../tmp/release-parser-3d/README.md), ignored by the
existing `tmp` Git rule. It contains raw listing titles, categories, listing IDs
and URLs, capture times, query/page provenance, source HTML digests, and selected
listing descriptions. No torrent or media payloads were downloaded.

| Measure | Captured result |
| --- | ---: |
| Distinct listing IDs | 3,546 |
| Distinct exact title strings | 3,541 |
| Search queries / successful search pages | 35 / 90 |
| Listing descriptions inspected | 18 |
| Reviewed seed cases | 62 |
| Seeds with explicit 3D claims | 20, including one conflicting-layout case |
| Seeds explicitly combining 2D and 3D | 12 |
| Seeds requiring no affirmative 3D assertion | 25 |
| Seeds left unresolved | 5 |

The 62 seeds were reviewed from title syntax and selected descriptions, not
generated from parser output; they still require human fixture review. The
remaining records have provisional triage labels, not expected parser values.
The archive includes 1,620 checksum-only candidates, 396 title-collision
candidates, and 996 records found only by the broad `HOU` query. These are
distinct triage buckets, not claims about verified media contents.

Queries cover `3D`, quoted `3D`, `SBS`, `HSBS`, `H-SBS`, `FSBS`, `Full-SBS`,
side-by-side spellings, `OU`, `HOU`, `H-OU`, `HTAB`, top/bottom and over/under
spellings, `MVC`, `BD3D`, `ISO`, `stereoscopic`, `anaglyph`, `2D+3D`, and compound
queries with source/resolution markers. An exclusion query reaches older names.
The exact requests and counts are in `collection-manifest.json` and `summary.json`.

Several broad searches hit Nyaa's displayed 1,000-result window. This is a large,
deliberately varied candidate corpus, not an exhaustive export, random sample,
or 3,546 confirmed stereoscopic releases. Repeated groups/titles/resolutions also
limit independent diversity. Do not claim population accuracy from its counts.

### Observed patterns that determine the design

Examples below preserve technical syntax while using neutral identity text.
Original titles and listing evidence are confined to the local archive.

| Observed form | Required interpretation |
| --- | --- |
| `[Title][3D FSBS][BDRIP][10bit]` | 3D, side-by-side, full; retain source, codec, and bit depth independently |
| `[Title](3D BD 1080p Full SBS)` | Same format; prefix and suffix modifier order both occur |
| `[Title][BDRIP][3D.HSBS][x264][1920x1036]` | 3D, side-by-side, half; do not infer quality from eye dimensions |
| `[Title][3D SBS Half 60FPS 1080p AC3 5.1]` | Half is after SBS; FPS/audio must remain intact |
| `Title 3D-SBS` | Side-by-side with unspecified sampling; SBS does not imply half |
| `Title [Blu-ray-3D.FA.1080p.H264.FLAC]` | Frame-alternative form; description explicitly defines FA in this 3D context |
| `Title (3D SBS Row Interlaced 1920x1080 x264 AAC)` | Positive 3D evidence but conflicting layout claims; do not choose one silently |
| `Title 3D (stereoscopic)` / `Title [1080p 3D]` | 3D with unknown packing |
| `Title [3D BD 240p 3DS].moflex` | Explicit 3D; console name/extension does not specify packing or change supported quality values |
| `[Title][2D+3D]`, `(2D/3D)`, `[2D BD+3D BD]` | Mixed presentation; no single inferred layout or codec |
| `Title (3D Ver. Viewable in 2D)` | A 3D version with compatibility wording, not automatically a bundle |
| `Title 3D - Subtitle (1080p BD Remux FLAC 5.1)` | Protect 3D inside the known title; a remux label is not stereo evidence |
| `3D Title - 01 [1080p]` and a trailing `\| 3D Alias` | Protect primary titles and aliases, including metadata-like title prefixes |
| `Title (SBS 1280x720 x264 AAC)` / `[SBS] Title` | Broadcaster or group ambiguity; codec/resolution proximity alone is insufficient |
| `[3D2552C1]`, `[BD3D6436]`, `[MVCH-29046]` | Hash/product-code substrings must not become 3D, disc, or MVC facts |
| `MPEG-H 3D Audio`, `3D Noise Reduction`, `3D LIVE`, 3D model/game names | Other meanings of 3D must not become video stereoscopy |

Some full-SBS descriptions report ordinary AVC/HEVC alongside two views and
wide encoded dimensions. One mixed-disc description reports both AVC and MVC,
even though MVC is absent from its title. Keep filename expectations separate
from description-only evidence: the latter explains formats but must never
be supplied as hidden knowledge to a title-only parser test.

### Coverage gaps

No unambiguous OU/TAB, anaglyph, or MVC *title* example was established by these
queries. `MVC` returned a music catalog code; `BD3D` returned checksum matches.
This does not prove those formats are absent from Nyaa or other release sources.

For full naming support, include conservative, explicitly synthetic tests for
those aliases, with that provenance clearly marked. Use exact expanded words
or strongly corroborated technical markers; do not generalize from substring
hits. Add future real examples without weakening existing negative assertions.

The [Matroska StereoMode specification](https://www.matroska.org/technical/elements.html#StereoMode)
distinguishes side-by-side, top/bottom, row/column interleaving, checkerboard,
anaglyph, and eye ordering. Use it as terminology support, not as evidence that
a release title contains a particular mode. Matroska's file-level defaults
are not defaults for absent release-name evidence.

## Current implementation and extension points

Paths and behavior were inspected in the pinned worktree. The semantic MCP
rejected this newly created worktree as an unconfigured scope, so inspection
used the permitted local fallback rather than assuming the parent checkout's
index matched the base.

| Owner | Current behavior / required extension |
| --- | --- |
| `crates/scryer-release-parser/src/model.rs` | `ParsedReleaseMetadata`, `MetadataAst`, and `MetadataEnrichment` have no stereoscopy representation; add typed data, defaults, and evidence |
| `crates/scryer-release-parser/src/lex.rs`, `sanitize.rs` | Lossless tokens, separators, bracket groups and Unicode normalization; preserve these for compound marker matching |
| `crates/scryer-release-parser/src/parse.rs` | Annotation, candidate scoring/projection, title protection, and `recover_independent_technical_metadata`; share one stereo recognizer across these stages |
| `crates/scryer-release-parser/src/enrichment.rs` | `metadata_tokens_for_candidate`, prefix/gap filters and `project_final_metadata`; do not lose stereo facts or join nonadjacent scoped tokens |
| `crates/scryer-release-parser/src/lib.rs` | Export public typed results through both target-aware entry points |
| `crates/scryer-application/src/quality/release_parser.rs` | Compatibility wrapper synthesizes context and returns the best projection; verify conservative behavior when real title context is unavailable |
| `crates/scryer-application/src/quality/release_dedup.rs` | Episode-result key contains group, episode, quality, codec, upload flags, dual audio and edition; add stereo distinction so variants cannot collapse |
| `crates/scryer-application/src/rules/user_rule_input.rs`, `crates/scryer-rules/src/release.rs` | Explicit release-document mapping currently drops any new field unless updated |
| `crates/scryer-application/src/types.rs` | `IndexerSearchResult` already holds `parsed_release_metadata`; preserve that structured value through search annotation |
| `crates/scryer-interface-media-types/src/types/acquisition.rs`, `crates/scryer-interface-media/src/mappers/acquisition.rs` | Explicit API projections require additive typed stereo mapping and focused schema checks |
| `crates/scryer-release-parser/tests/sourced_release_corpus.rs`, `tests/parse_recovery.rs`, `src/tests.rs` | Extend established offline assertion and recovery testing; keep current accepted-failure signatures intact |

No runtime baseline was measured in this planning task. The existing README's
1,500-case measurements are historical, not fresh results for this base.

## Proposed data contract

Add `stereoscopy: Option<ParsedStereoscopy>` to `ParsedReleaseMetadata` and
corresponding enrichment/AST representation. Use typed enums internally and
explicit stable wire mappings at rules and GraphQL boundaries.

`ParsedStereoscopy` contains:

- `presentation`: `TwoD`, `ThreeD`, or `Mixed2d3d`. The outer `None` means no
  reliable technical assertion. `TwoD` requires an explicit technical 2D marker.
- `layout`: optional `SideBySide`, `TopBottom`, `FrameSequential`,
  `RowInterleaved`, `ColumnInterleaved`, `Checkerboard`, or `Anaglyph`.
- `sampling`: optional `Half` or `Full`, meaningful only for SBS/TAB layouts.
- `encoding`: optional `Mvc`. Keep the existing AVC/H.264 codec family intact
  when explicitly supported; MVC does not imply SBS, REMUX, or a complete disc.

Keep evidence as bounded token ranges in the AST/analysis, including competing
claims. Use existing `parse_hints` for field-specific conflicts such as
`stereo:layout_conflict` and `stereo:sampling_conflict`. Clear the conflicted
attribute while retaining independent positive 3D evidence. A `2D`/`3D`
contradiction without explicit bundle syntax stays unresolved, not `Mixed2d3d`.

For a mixed bundle, leave layout/sampling/encoding unset unless the syntax
unambiguously scopes a claim to the 3D component; the first implementation can
conservatively leave these unset and preserve the spans. Do not flatten two
different 3D encodes into one format. Conflicts must survive final enrichment.

Derive a convenience `has_3d()` value from this structure if consumers need it;
do not maintain a second independent boolean. Optional API fields preserve
unknown values. Do not map missing data to a proven 2D presentation.

## Recognition rules

1. Implement a bounded token-window recognizer, preferably in a small
   `stereoscopy.rs` module. Return claims, consumed token ranges and evidence
   strength. Work from the existing lossless stream, not an unrestricted regex
   over `raw_title`. Keep the 4,096-byte input, 256-token and bracket-depth limits.
2. Match longest compounds first, case-insensitively after existing normalization:
   `HSBS`, `H-SBS`, `Half SBS`, `SBS Half`; `FSBS`, `F-SBS`, `Full SBS`, `SBS Full`;
   `side by side`; and the corresponding `HOU`, `HTAB`, `F-OU`, `Full TAB`,
   `top bottom`, `top and bottom`, `over under` forms. Explicitly enumerate
   accepted variants and required context in table-driven tests.
3. Exact technical `3D`, `3-D`, `stereoscopic`, `BluRay3D`/`BD3D` and explicit MVC
   may establish 3D without a layout. Recognize compact forms only at full-token
   boundaries. Require technical scope for ambiguous abbreviations. `FA` needs
   explicit 3D corroboration; expanded frame-alternative/sequential language is
   stronger. Bare `SBS`, `OU`, `TB`, `TAB`, `HSBS`, `HOU`, or `MVC` in title/group
   text is not sufficient. Strong expanded layout phrases in technical scope
   can establish 3D even without a separate `3D` token.
4. Source/disc and stereoscopy are independent facts. An explicit `BluRay3D`
   compound can supply BluRay plus 3D; 3D alone cannot set `is_bd_disk` or
   `is_remux`. Audit exact terminal `.mk3d` support as format evidence; a
   substring inside a name, `.mkv`, `.moflex`, or `3DS` alone must not imply a
   particular layout. Avoid broad changes to ordinary extension handling.
5. Match multi-token forms only across actual adjacent tokens in compatible
   bracket groups/depths. Never assemble `H` and `SBS` across an alias, episode,
   release group, unrelated metadata group, or filtered-out tokens. Explicit
   evidence in different groups may combine when each claim is independently
   complete, but modifiers cannot jump groups.
6. Protect title, alias, episode-title and release-group spans before consuming
   markers. Preserve alternate title interpretations in the beam. Do not make
   bare `3D` or `SBS` universal strong anchors. The neutral wrapper must be
   conservative when it cannot separate a title word from a format claim.
7. Handle explicit mixed/version syntax, including `2D+3D`, `2D/3D`,
   `2D and 3D`, `2D & 3D`, and `[2D BD+3D BD]`. Distinguish combined versions
   from phrases such as `viewable in 2D`, `2D to 3D conversion`, and titles of
   different movies inside a pack. Conversion wording describes an output claim,
   not automatically a mixed bundle; uncertain scope remains unresolved.
8. Do not derive half/full, eye order, per-eye resolution, HDR, frame rate,
   actual stereo payload, or device compatibility from dimensions, codec,
   container, title identity, theatrical format, or site category alone.
   Preserve existing quality semantics even when full-SBS width is 3,840.
9. Reuse this recognizer for candidate enrichment and independent technical
   recovery. Recover complete technical stereo evidence when identity is
   ambiguous/unparseable, while preserving that identity disposition. A stereo
   marker cannot make an unresolved title eligible for automatic acquisition.

## Implementation sequence

### 1. Freeze the fixture contract and baseline

Build an offline `tests/corpus/stereoscopy` fixture set from the 62 seeds,
then adjudicate the 193 remaining semantic-review candidates. Retain the full
local negative pool for research. Each promoted case needs an independently
written target context, expected presentation/layout/sampling/encoding,
unchanged identity/quality/audio facts, and an explanation for uncertainty.

Follow `CONTRIBUTING.md` and the existing sourced-corpus anonymization contract.
Keep raw titles, contributors, source IDs, URLs, hashes and paths in ignored local
research. Preserve collisions such as a title starting with literal `3D` using
neutral sentinels. Mark manually constructed cases as synthetic. Do not turn
description-only details into filename expectations or silently label unknown
records as negatives. No original-site descriptions enter committed fixtures.

Run the existing parser corpus and recovery tests once to record the base tree's
actual behavior before parser changes. Store observed results separately from
expected values. Report existing accepted failures rather than fixing unrelated
parsing behavior in this feature.

### 2. Add the model and bounded grammar

Implement the typed contract, default/empty behavior, stable serialization,
module exports and recognizer. Add focused tests for every observed alias and
every intentionally supported synthetic alias, including required context and
negative counterparts. Record spans without widening lexer or beam bounds.

### 3. Integrate candidate selection and recovery

Wire claims into annotations, metadata consumption, candidate projection,
prefix/gap enrichment and independent recovery. Preserve known-title identity,
episode coordinates, groups, source, quality, codec and upload flags. Test
multiple candidate targets and ambiguous/unparseable results explicitly.
Use the same conflict resolution in every path.

### 4. Preserve downstream metadata and distinct releases

Carry structured stereo data through the application wrapper, existing release
rule documents and typed acquisition API projection. Update relevant frontend
query/types only if they consume the changed schema; a new display or preference
is outside scope. Keep existing default scoring unchanged.

Include normalized presentation/layout/sampling/encoding in the episodic search
dedup key. Unknown and explicit 2D must remain distinct, as must SBS-half,
SBS-full, TAB and MVC. For conflicted stereo claims, use the existing empty-key
behavior to retain both candidates rather than collapsing them. Do not expand
movie dedup behavior, which currently returns an empty key. This changes only
search-result grouping, not filesystem deletion or replacement decisions.

Audit constructors and explicit mappings for omitted fields. Additive wire
changes must not invent stereo facts for legacy results. Any additional
persistence, plugin capability, dependency, or import-policy work discovered
here needs a separately scoped proposal.

### 5. Validate and hand off

Require all reviewed 3D fixture assertions and negative controls to pass with
zero new accepted failures. Compare pre/post identity and non-stereo metadata
on the existing sourced corpus. Group measurements by real title family/group
as well as raw release count so repeated resolution variants do not dominate.

Keep a frozen holdout by title/group family before implementing grammar;
synthetic separator and case variants of one input belong to the same split.
Report unknown and unresolved cases separately from errors, and report exact
layout/sampling accuracy separately from detection. A filename omission is not
a false negative for a fact available only in a listing description.

## Focused validation and acceptance

Use `rtk` for execution. Request escalated execution on the first local Nextest
run because fixture servers may need port binding. During implementation:

```sh
rtk proxy cargo fmt --all --check
rtk proxy cargo nextest run -p scryer-release-parser --test stereoscopy --locked --no-fail-fast
rtk proxy cargo nextest run -p scryer-release-parser --test sourced_release_corpus --locked --no-fail-fast
rtk proxy cargo nextest run -p scryer-release-parser --test parse_recovery --locked --no-fail-fast
```

`stereoscopy` is a proposed new test target. Also run the named parser unit
tests, wrapper, rule mapping, dedup, and API tests added by this change with
focused Nextest filters. Use existing frontend compatibility checks only if
the corresponding query/schema files change. Do not run live indexers in tests.
Full workspace sweeps and Clippy follow the repository's later integration
checkpoint, not routine feature worktree validation.

Required behavior checks:

- All observed FSBS/HSBS/Full-SBS/SBS-Half/FA and generic 3D forms, plus explicit
  mixed bundles, preserve their independently established attributes.
- Nyaa checksum, broadcaster/group, primary/alternate/episode title, CGI,
  concert, audio and product-code controls do not assert stereoscopic video.
- Unknown, explicit 2D, 3D, mixed and conflicted results remain distinguishable.
- Conflicting half/full or layouts retain evidence without arbitrary precedence;
  ordinary interlaced scanning and stereo row interleaving stay separate.
- Separators, full-width Unicode, entity decoding, nesting, prefix/gap/suffix
  locations, repeated markers and token/input truncation cannot cross scope
  boundaries, panic, or cause unbounded work. Extend existing fuzz inputs for
  these grammar boundaries without introducing a new fuzz dependency.
- Multi-target parsing and recovery preserve title/episode uncertainty and all
  unrelated metadata assertions. Stereo labels do not inflate identity confidence.
- Serialization, rule documents and API mappings preserve null/unknown semantics;
  episodic dedup keeps 2D and differently packed 3D candidates separate.
- Test expectations are offline and independent of implementation output. No
  hidden relaxation of the existing sourced corpus's accepted-failure signatures.

## Planning validation completed

The worktree/base and source paths were inspected. The research capture contains
90 successful search pages and 18 successful detail pages. Corpus uniqueness,
manifest hashes, seed-case references, the ignored research path and plan links
were checked during planning. The implementation validation below supersedes
the original planning-only validation status.

## Implementation handoff

Implemented typed presentation, layout, sampling and MVC encoding; bounded
marker recognition; token evidence and conflict hints; conservative title/alias
and group protection; independent recovery; GraphQL projection; release-rule
fields and canonical tokens; and presentation-aware episodic deduplication.
User and managed release policies share these fields through the same input
builder. The editor reference and Rust validation contract are kept in sync.

Rules can match `"stereo:has_3d" in input.release.stereoscopy_tokens` for both
3D and mixed bundles, or match the separate `stereo:three_d`,
`stereo:mixed_2d_3d`, layout, sampling, MVC and conflict tokens.
The original lexer `normalized_tokens` retain their meaning.
See the fixture README for the complete token contract and a Rego example.

The second semantic review adjudicated all 193 remaining candidates:
12 explicit 3D, one explicit 2D, one mixed bundle, 111 without affirmative
stereoscopy, 43 outside video scope, and 25 unresolved. Together with the
62 seeds, 255 candidates have recorded review reasons in the local archive.
The larger checksum/title/broad-query pools retain provisional labels.
Promoted fixtures total 87: 32 explicit 3D (including a layout conflict),
13 mixed, one 2D, and 41 controls. Thirty unresolved research cases are excluded
from expected-output fixtures; no media verification or population accuracy is claimed.

The original frozen family-holdout goal was not met: the first 57 cases were
used during development. Thirty additional expected cases were frozen before
first execution; 29 passed and the collection form `OVA×3+Movie2D&3D+SP`
failed. The grammar was corrected without changing its oracle. All are now
regression cases, not an independent holdout. Sparse format families such as
OU/TAB, MVC and interleaving have explicitly synthetic coverage.

Final focused validation on this worktree:

- `rtk proxy cargo fmt --all -- --check`: passed.
- `rtk proxy cargo nextest run -p scryer-release-parser --locked --no-fail-fast`:
  235 passed, zero skipped. Includes the existing 1,500-case sourced corpus,
  all 87 stereo fixture cases and synthetic grammar/recovery controls.
  No accepted-failure signatures were changed.
- `rtk proxy cargo nextest run -p scryer-application -p scryer-interface-media -p scryer-rules -E 'test(stereo) | test(rule_input_contract_copies_are_byte_identical)' --locked --no-fail-fast`:
  five passed, 4,855 outside the focused filter. The application test executes
  seven parser-to-input cases through both user and managed Rego evaluation,
  including positive scoring and block-score tokens.
- `rtk proxy node --test lib/utils/rule-sets.test.ts` in the web package:
  five passed.
- `rtk proxy npm run typecheck` in the web package: blocked because this worktree
  lacks `node_modules/@typescript/native/bin/tsc`. Dependencies were not changed.
- `rtk proxy git diff --check`: passed. Rule-contract mirrors and the 87 unique
  fixture IDs were checked; fixtures represent 65 distinct context title strings,
  not 65 verified independent title families.

The build emitted existing unused-code warnings and a macOS linker unwind-table
warning. Full workspace tests and Clippy remain deferred under the repository's
feature worktree cadence. Fuzz campaigns were not run; two new seeds cover stereo
scope and alias boundaries. Package versions, dependencies, persistence and
default scoring policy were not changed in the release-parser phase.

## Follow-up: media detection boolean

The requested media scope is a boolean for display and maintenance comparisons.
Explicit Matroska StereoMode (1–14), MP4 st3d version-zero modes (1–4), or
H.264 Multiview High/Stereo High profiles establish the detected flag.
Unknown modes, missing tags, unexamined files and ordinary H.264 do not.
No dimensions-based inference, video decoding or transcoding is added.

Container evidence persists as `StreamMetadata.is_3d` in the existing analysis
JSON. `AnalysisDetails::is_3d()` combines video evidence and supplies both
`analysis.is3D` in GraphQL and `input.facts.files[].is_3d` in maintenance rules.
Both file-badge presentations display a 3D pill when true. False means
"3D not detected", not a verified 2D classification. Existing files need normal
re-analysis to discover previously unrecorded container tags; no scan is run
and no automatic re-analysis or package/schema revision bump is introduced here.

For example, a maintenance matcher can select subjects containing a detected file:

```rego
match if {
    some file in input.facts.files
    file.is_3d == true
}
```

Focused verification:

- Seven media-types/mediainfo/rules tests passed (522 outside the filter),
  covering MKV and MP4 declarations, malformed/truncated input, codec profiles,
  maintenance comparisons and the existing unknown-fact hold.
- Three API/application tests passed (4,612 outside the filter), including
  analysis JSON round-trip and API projection. Maintenance input mapping compiles.
- Forty existing frontend query/format-pill tests passed.
- The existing `export-graphql-schema` binary built and ran successfully;
  `api/graphql/schema.graphql` was regenerated with only the expected media
  boolean and release-parser stereo schema additions.
- Formatting and diff checks passed. Frontend GraphQL contract tests are blocked
  by the missing `graphql` package; typechecking and visual preview likewise lack
  this worktree's frontend dependencies. No third-party dependencies were changed.
