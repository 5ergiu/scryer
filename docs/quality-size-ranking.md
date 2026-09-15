# Quality and size ranking

Mixed profiles prefer the highest admitted quality that is available. Lower
qualities are fallbacks. Eligibility and the profile's quality order precede
revision, numeric score, size fit, and listing tie-breakers. A finite point
bonus cannot enforce this on its own because custom scores are unrestricted.

## Resolution weight model

The bundled-policy model in `quality/size_ranking_tests.rs` evaluates real
parser output and rule contributions, then compares candidate resolution gaps.
For the Balanced persona, these are the higher resolution's remaining score
margins when the lower resolution has the indicated advantage:

| Lower-resolution advantage | +25 step | +100 step | +200 step | +300 step |
| --- | ---: | ---: | ---: | ---: |
| Otherwise identical | 25 | 100 | 200 | 300 |
| AV1 versus H.264 | 5 | 80 | 180 | 280 |
| TrueHD Atmos 7.1 versus DDP 5.1 | -10 | 65 | 165 | 265 |
| WEB-DL versus WEBRip | -15 | 60 | 160 | 260 |
| Gold group versus unknown group | -305 | -230 | -130 | -30 |

The 100-point step gives resolution more weight than the modeled codec/audio/source
differences. Even 300 cannot guarantee fallback ordering against group
reputation, so the profile's ordered tier remains authoritative. There is no
attempt to encode the entire recommendation order in a single score.

Resolution contributes 100 points per standard resolution step above the lowest
admitted quality. The steps are 480p, 576p, 720p, 1080p, 2160p and 4320p.
A mixed 720p/1080p/2160p profile contributes 0/100/200; a 1080p/2160p profile
contributes 0/100. Skipped tiers still count: 720p/2160p contributes 0/200.
Duplicate entries do not add points. Unlisted and unknown qualities, and an
unrestricted profile without a defined floor, earn no bonus. Changing the
profile floor rederives the contribution for both candidate and incumbent;
comparisons use the current profile rather than historical score totals.
Resolution points cannot recover a group/language block-threshold rejection.
Other numeric thresholds continue to operate on the displayed total, so their
effective baseline includes the resolution contribution. Same-tier score deltas
cancel that constant on both sides of an upgrade comparison.

## Size policy

Expected size retains the existing bitrate, codec, source, remux, runtime and
coverage model. Built-in size classifications now contribute zero points.
Custom-authored size rules remain explicit operator policy.

The Balanced persona treats 0.65–1.8 times expected size as equally plausible.
The compact preference uses 0.5–1.2; Audiophile and Compatible use 0.85–3.2.
Outside that interval, logarithmic distance supplies a search-only tie-breaker.
Neither size fit nor routine archive overhead changes the intrinsic score or
independently causes an upgrade. An unknown runtime or a plausible inferred
pack-member size supplies no size-ranking advantage or penalty.

Hard rejection uses a separate, deliberately broad delivery-media envelope:

| Resolution | H.264 lower bound (Mbps) | Total upper bound (Mbps) |
| --- | ---: | ---: |
| 480p | 0.04 | 40 |
| 576p | 0.05 | 40 |
| 720p | 0.10 | 100 |
| 1080p | 0.20 | 200 |
| 2160p | 0.25 | 600 |
| 4320p | 0.35 | 1800 |

These are conservative application heuristics, not codec specification limits.
They apply only with known positive runtime, recognized resolution and codec.
Missing metadata and disc payloads cannot justify size rejection. The lower
bound also exempts specials and ambiguous aggregates. It scales down by
0.75/1.1 for HEVC/VP9 and 0.5/1.1 for AV1; it is disabled for VVC and legacy
codecs without calibrated lower-size evidence. Stream-pointer byte counts are
withheld entirely. There is no fixed minimum file size, so short media scales
down with its runtime.

The upper bounds are independent of persona and codec efficiency; remuxes
receive 1.5 times the listed ceiling for additional preserved streams.
Better compression does not imply a smaller maximum file. This leaves room
for transparent encodes, grain, high frame rates, multiple/lossless audio and
archive overhead. The 1080p examples reject 100 MiB H.264 at 90 minutes
(0.155 Mbps) and 100 GiB AV1 at 20 minutes (716 Mbps), while retaining 6 GiB
AV1 episodes, 300 MiB 4K AV1 movies and short clips.

The 1080p H.264 lower bound is approximately 1.43 MiB/min, deliberately below
Sonarr's native WEB 1080p minimum of 4 MiB/min. Sonarr's broad
3–130/4–130 MiB/min WEB 720p/1080p bands and Radarr's 0–100 MiB/min WEB bands
inform plausibility, while Scryer's codec/runtime model remains authoritative.
No user-facing size configuration is introduced.

## Codec audit

The retained typical-size factors are H.264 1.10, HEVC/VP9 0.75 and AV1 0.50.
Explicit preference estimates replace the generic fallback for VC1 (1.30),
MPEG2 (1.80), MPEG4/Xvid/DivX (1.50), and VVC (0.50). The legacy estimates
provide more room than H.264; VVC shares AV1's estimate without assuming extra
uncalibrated savings. Unknown codecs retain a neutral 1.0 factor. These are
rough priors for tie-breaking and explanatory bands, not measured guarantees
of equal quality or grounds for hard rejection.

Compression savings vary with content and encoder settings. HandBrake's
[constant-quality documentation](https://handbrake.fr/docs/en/latest/technical/video-cq-vs-abr.html)
explains that bitrate changes to meet the quality target; AOMedia reports
[average AV1 compression gains](https://aomedia.org/press%20releases/the-alliance-for-open-media-kickstarts-video-innovation-era-with-av1-release/),
not maximum permitted bitrates. This is why a typical-size multiplier must
not shrink the maximum admission bitrate or compress the audio allowance.

The audit includes 2,880 codec/persona/resolution/runtime/category combinations,
audio-heavy and remux examples, missing evidence, codec ordering, and the
historical 1,728-case guard corpus to detect newly introduced upper rejections.

## Existing installations and presentation

At startup, tracked copies of the exact previous built-in size source receive
the zero-point replacement through the atomic rule-pack persistence path.
Enabled state, priority, facets, identity and custom-authored copies are
preserved. Failed persistence leaves the active engine and saved rules intact;
the correction is idempotent. The historical Rego file is a match-only fixture,
not a policy evaluated at runtime.

Search defaults to the backend's recommended order. Explicit score/size sorts
retain that order for ties. Sizes display fractional GiB/MiB rather than being
floored to whole numbers.

Validation covers the matching mixed-profile pair across all four personas,
weight modeling, codec-equivalent size ratios, normal-size plateaus, extreme
small/large examples, coverage ambiguity, stream pointers, language/group
exclusions, restart correction and frontend sorting. Full workspace tests and
Clippy belong to the repository's later integration checkpoint.
