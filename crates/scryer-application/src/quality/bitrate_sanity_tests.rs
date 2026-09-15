//! Conservative admission examples, intentionally independent of size preference.
use super::*;

const CODECS: [VideoCodec; 10] = [
    VideoCodec::H264,
    VideoCodec::H265,
    VideoCodec::Av1,
    VideoCodec::Vp9,
    VideoCodec::Vc1,
    VideoCodec::Mpeg2,
    VideoCodec::Mpeg4,
    VideoCodec::Xvid,
    VideoCodec::Divx,
    VideoCodec::Vvc,
];

#[test]
fn plausible_delivery_bitrates_remain_eligible_for_every_codec_and_persona() {
    let mut cases = 0;
    for codec in CODECS {
        for persona in [
            ScoringPersona::Balanced,
            ScoringPersona::Efficient,
            ScoringPersona::Audiophile,
            ScoringPersona::Compatible,
        ] {
            let mut profile = builtin_4k_profile();
            profile.criteria.scoring_persona = persona;
            for (quality, mbps) in [
                ("480P", 8.0),
                ("576P", 10.0),
                ("720P", 35.0),
                ("1080P", 90.0),
                ("2160P", 160.0),
                ("4320P", 320.0),
            ] {
                for runtime in [1, 20, 90, 240] {
                    for category in ["movie", "series", "anime"] {
                        let mut release = crate::parse_release_metadata("Example.S01E01.WEB-DL");
                        release.quality = Some(quality.into());
                        release.video_codec = Some(codec);
                        let mut decision = QualityProfileDecision::new();
                        let bytes = (mbps * 1_000_000.0 / 8.0 * f64::from(runtime) * 60.0) as i64;
                        apply_size_requirement(
                            &mut decision,
                            &profile,
                            &release,
                            Some(bytes),
                            Some(category),
                            CoverageSizeBasis::single(Some(runtime)),
                        );
                        assert!(
                            decision.allowed,
                            "{codec:?} {quality} {runtime} {category}: {:?}",
                            decision.block_codes
                        );
                        assert_eq!(decision.release_score, 0);
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 2880);
}

#[test]
fn codec_model_covers_every_parser_codec_without_inverting_efficiency() {
    let mut profile = builtin_4k_profile();
    profile.criteria.scoring_persona = ScoringPersona::Balanced;
    let h264 = codec_efficiency_factor(Some(&VideoCodec::H264));
    let hevc = codec_efficiency_factor(Some(&VideoCodec::H265));
    let av1 = codec_efficiency_factor(Some(&VideoCodec::Av1));
    assert!(av1 < hevc && hevc < h264);
    println!("codec,typical_1080p_web_episode_bitrate");
    for codec in CODECS {
        let factor = codec_efficiency_factor(Some(&codec));
        assert!(factor.is_finite() && factor > 0.0);
        if matches!(
            codec,
            VideoCodec::Vc1
                | VideoCodec::Mpeg2
                | VideoCodec::Mpeg4
                | VideoCodec::Xvid
                | VideoCodec::Divx
        ) {
            assert!(
                factor > h264,
                "{codec:?} needs more bitrate headroom than H.264"
            );
        }
        if codec == VideoCodec::Vvc {
            assert_eq!(factor, av1, "do not assume uncalibrated extra VVC savings");
        }
        println!("{codec},{:.2}", 8.5 * factor * 0.8);
        let mut release = crate::parse_release_metadata("Example.S01E01.1080p.WEB-DL");
        release.video_codec = Some(codec);
        let mut decision = QualityProfileDecision::new();
        let bytes = (8.5 * factor * 0.8 * 3600.0 / 8.0 / 1024.0 * 1_073_741_824.0) as i64;
        apply_size_requirement(
            &mut decision,
            &profile,
            &release,
            Some(bytes),
            Some("series"),
            CoverageSizeBasis::single(Some(60)),
        );
        assert_eq!(decision.size_fit_penalty, 0, "{codec:?}");
    }
}

#[test]
fn short_compact_and_audio_heavy_releases_are_not_mistaken_for_impossible_files() {
    let profile = builtin_4k_profile();
    for (title, mib, runtime) in [
        ("Example.S01E01.1080p.WEB-DL.AV1.AAC", 2.0, 1),
        ("Example.2024.2160p.WEB-DL.AV1.AAC", 300.0, 90),
        (
            "Example.S01E01.720p.BluRay.AV1.TrueHD.Atmos.7.1.Dual.Audio",
            5120.0,
            24,
        ),
        ("Example.S01E01.1080p.WEB-DL.AV1.FLAC", 6144.0, 24),
        (
            "Example.S01E01.1080p.BluRay.Remux.H.264.DTS-HD.MA",
            28672.0,
            45,
        ),
    ] {
        let release = crate::parse_release_metadata(title);
        let mut decision = QualityProfileDecision::new();
        apply_size_requirement(
            &mut decision,
            &profile,
            &release,
            Some((mib * 1_048_576.0) as i64),
            Some("series"),
            CoverageSizeBasis::single(Some(runtime)),
        );
        assert!(decision.allowed, "{title}: {:?}", decision.block_codes);
    }
}

#[test]
fn missing_evidence_and_disc_payloads_cannot_support_a_size_rejection() {
    let baseline = crate::parse_release_metadata("Example.S01E01.1080p.WEB-DL.AV1");
    for variation in 0..5 {
        let mut release = baseline.clone();
        let mut basis = CoverageSizeBasis::single(Some(20));
        match variation {
            0 => basis = CoverageSizeBasis::single(None),
            1 => release.quality = None,
            2 => release.video_codec = None,
            3 => release.is_bd_disk = true,
            _ => release.quality = Some("UNRECOGNIZED".into()),
        }
        for bytes in [100, 100 * 1024 * 1024 * 1024] {
            let mut decision = QualityProfileDecision::new();
            apply_implausible_size_limits(&mut decision, &release, bytes, basis);
            assert!(decision.allowed, "variation={variation} bytes={bytes}");
        }
    }
}

#[test]
fn hard_bounds_do_not_depend_on_persona_or_codec_upper_efficiency() {
    for codec in CODECS {
        for persona in [
            ScoringPersona::Balanced,
            ScoringPersona::Efficient,
            ScoringPersona::Audiophile,
            ScoringPersona::Compatible,
        ] {
            let mut profile = builtin_4k_profile();
            profile.criteria.scoring_persona = persona;
            let mut release = crate::parse_release_metadata("Example.S01E01.1080p.WEB-DL");
            release.video_codec = Some(codec);
            let mut decision = QualityProfileDecision::new();
            apply_size_requirement(
                &mut decision,
                &profile,
                &release,
                Some(100 * 1024 * 1024 * 1024),
                Some("series"),
                CoverageSizeBasis::single(Some(20)),
            );
            assert!(
                decision
                    .block_codes
                    .iter()
                    .any(|code| code == "size_implausible_for_quality"),
                "{codec:?}"
            );
        }
    }
}
