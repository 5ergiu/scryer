# Stereoscopy release shapes

`reviewed.json` contains 57 manually anonymized release-shaped regression cases:
20 explicit 3D claims (including one conflicting layout), 12 explicit mixed
2D/3D releases, and 25 cases without affirmative video stereoscopy evidence.
`additional.json` adds 30 shapes from the subsequent review: 12 explicit 3D,
one mixed bundle, one explicit 2D and 16 controls, for 87 cases in total.
Across both local research passes, 30 unresolved cases have no reliable filename
oracle and were not promoted. The larger research archive remains local.

The expected presentation, layout, sampling and encoding fields were specified
from release syntax before executing the parser on these fixtures. Descriptions
helped interpret format terminology but cannot supply fields absent from an
input name. A null expected result is no reliable *filename assertion*, not
proof that the actual video is 2D. Known title/alias and episode-title context
protects semantic identity without supplying expected format metadata.

Title and contributor words use alphabetic sentinels; hashes and catalog
numbers are replaced while retaining collision syntax. Primary and alternate
title structure, punctuation, technical labels and title-internal `3D` are
preserved. The committed fixtures contain no source-site names, original
listing IDs/URLs, original hashes, descriptions, or personal paths.

These cases were used during development and are regression coverage, not a
blind holdout or a population accuracy estimate. Multiple encodes of one title
are related examples. The local pool's unreviewed/heuristic records are not
treated as ground truth. Do not infer 3D prevalence from search-result counts.
The second set's expectations were frozen before its first execution: 29/30
passed initially; the mixed collection form exposed a missing grammar rule.
That failure was corrected without changing its expected result. This is
iterative corpus validation, not a pre-implementation family holdout.

`tests/stereoscopy.rs` separately contains synthetic OU/TAB, MVC, anaglyph,
interleaving, conflict, prefix, Unicode, scope and truncation controls. It checks
serialization and preservation of unrelated title/quality/audio fields.
The reviewed harness accumulates all field mismatches without accepted failures.

```sh
rtk proxy cargo nextest run -p scryer-release-parser --test stereoscopy --locked --no-fail-fast
```

Runtime contract: `stereoscopy` is optional. Present values distinguish
`two_d`, `three_d`, and `mixed_2d_3d`; layout, sampling and encoding may remain
null. A mixed bundle does not promise a homogeneous format. Conflicting claims
retain token evidence and `stereo:*_conflict` hints instead of choosing a value.
Consumers can inspect `input.release.stereoscopy` in release rules and the
nullable `parsedRelease.stereoscopy` GraphQL object. No default scoring or
media-file replacement policy is introduced by this metadata.

Both user and managed release rules also receive `input.release.stereoscopy_tokens`:
`stereo:two_d`, `stereo:three_d`, or `stereo:mixed_2d_3d`; `stereo:has_3d`
for the latter two; `stereo:layout:<layout>`, `stereo:sampling:half/full`,
`stereo:encoding:mvc`, and any `stereo:<field>_conflict` tokens.
Unknown presentation produces an empty list unless there is a conflict.
Raw `normalized_tokens` remain lexer tokens. For example, a rule can use:

```rego
score_entry["prefer_half_sbs"] := 100 if {
    "stereo:layout:side_by_side" in input.release.stereoscopy_tokens
    "stereo:sampling:half" in input.release.stereoscopy_tokens
}
```
