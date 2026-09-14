use scryer_release_parser::{
    ContextAlias, ContextFacetHint, ContextTitle, ParseDisposition, ParsedReleaseMetadata,
    ReleaseParseContext, StereoEncoding, StereoLayout, StereoPresentation, StereoSampling,
    analyze_release_for_target, best_parse_for_target,
};

fn context(title: &str) -> ReleaseParseContext {
    ReleaseParseContext {
        facet_hint: ContextFacetHint::Movie,
        title: ContextTitle { name: title.into() },
        aliases: vec![],
        known_years: vec![],
        imdb_ids: vec![],
        episodes: vec![],
    }
}

fn parse(raw: &str) -> ParsedReleaseMetadata {
    best_parse_for_target(raw, &context("Known Title"))
}

#[test]
fn stereo_multi_target_scoring_preserves_identity() {
    let targets = [context("Known Title"), context("Different Title")];
    let before = scryer_release_parser::analyze_release_against_targets(
        "Known Title 2020 [1080p x264]",
        &targets,
    );
    let after = scryer_release_parser::analyze_release_against_targets(
        "Known Title 2020 [1080p 3D HSBS x264]",
        &targets,
    );
    assert_eq!(before.best_target_index, Some(0));
    assert_eq!(after.best_target_index, before.best_target_index);
    let before = before.targets[0].analysis.best_candidate().unwrap();
    let after = after.targets[0].analysis.best_candidate().unwrap();
    assert_eq!(after.projected.disposition, before.projected.disposition);
    assert_eq!(
        after.projected.normalized_title,
        before.projected.normalized_title
    );
    assert!(after.projected.stereoscopy.unwrap().has_3d());
}

#[test]
fn stereo_prefix_source_and_identity_recovery_boundaries() {
    for raw in [
        "[3D] Known Title 2020 [1080p x264]",
        "Known Title [3D HSBS] 2020 [1080p x264]",
        "Known Title 2020 [1080p BD3D x264]",
        "Known Title 2020 [1080p BluRay3D x264]",
    ] {
        let parsed = parse(raw);
        assert!(
            parsed.stereoscopy.is_some_and(|s| s.has_3d()),
            "{raw}: {parsed:?}"
        );
        if raw.contains("BD3D") || raw.contains("BluRay3D") {
            assert_eq!(
                parsed.source,
                Some(scryer_release_parser::ReleaseSource::BluRay)
            );
        }
    }
    let mut target = context("Known Title 3D");
    target.aliases.push(ContextAlias {
        name: "Another Title".into(),
    });
    assert!(
        best_parse_for_target("Known Title (Another Title) 3D [1080p]", &target)
            .stereoscopy
            .is_none()
    );
    assert!(
        best_parse_for_target("Known Title (Unrelated Words) 3D [1080p]", &target)
            .stereoscopy
            .is_some()
    );
}

#[test]
fn stereo_bounds_and_evidence_survive_untrusted_input_shapes() {
    let prefix = "Known Title 2020 1080p ";
    let truncated = format!("{prefix}{}3DAnimation", " ".repeat(4096 - prefix.len() - 2));
    assert!(parse(&truncated).stereoscopy.is_none());
    for raw in [
        format!("Known Title {}3D HSBS{}", "[".repeat(50), "]".repeat(50)),
        format!("Known Title [1080p {}]", "3D HSBS ".repeat(300)),
        format!("Known Title [1080p {}]", "字".repeat(3000)),
    ] {
        let analysis = analyze_release_for_target(&raw, &context("Known Title"));
        assert!(analysis.tokens.len() <= 256);
        if let Some(candidate) = analysis.best_candidate() {
            for evidence in &candidate.metadata.stereo_evidence {
                assert!(evidence.tokens.start_token < evidence.tokens.end_token);
                assert!(evidence.tokens.end_token <= analysis.tokens.len());
            }
        }
    }
}

#[test]
fn stereo_wire_labels_round_trip_without_erasing_unknown_attributes() {
    for marker in ["2D", "3D", "2D+3D", "3D HSBS", "3D MVC"] {
        let value = parse(&format!("Known Title [1080p {marker}]"))
            .stereoscopy
            .unwrap();
        let wire = serde_json::to_value(value).unwrap();
        assert_eq!(wire["presentation"], value.presentation.as_str());
        assert_eq!(
            serde_json::from_value::<scryer_release_parser::ParsedStereoscopy>(wire).unwrap(),
            value
        );
    }
    assert!(
        serde_json::from_str::<scryer_release_parser::StereoLayout>("\"unsupported\"").is_err()
    );
}

#[test]
fn stereo_reviewed_release_shapes() {
    let mut cases: serde_json::Value =
        serde_json::from_str(include_str!("corpus/stereoscopy/reviewed.json")).unwrap();
    let additional: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("corpus/stereoscopy/additional.json")).unwrap();
    cases.as_array_mut().unwrap().extend(additional);
    let mut failures = Vec::new();
    for case in cases.as_array().unwrap() {
        let target: ReleaseParseContext = serde_json::from_value(case["context"].clone()).unwrap();
        let parsed = best_parse_for_target(case["release"].as_str().unwrap(), &target);
        let actual = serde_json::to_value(parsed.stereoscopy).unwrap();
        if actual != case["expected"] {
            failures.push(format!(
                "{}: expected {}, actual {}; hints {:?}",
                case["id"], case["expected"], actual, parsed.parse_hints
            ));
        }
        if let Some(conflict) = case["conflict"].as_str() {
            let hint = format!("stereo:{conflict}_conflict");
            if !parsed.parse_hints.contains(&hint) {
                failures.push(format!("{}: missing {hint}", case["id"]));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn stereo_layout_and_sampling_aliases() {
    use StereoLayout::*;
    use StereoSampling::*;
    let cases = [
        ("3D FSBS", SideBySide, Some(Full)),
        ("3D Full SBS", SideBySide, Some(Full)),
        ("3D SBS Full", SideBySide, Some(Full)),
        ("3D.HSBS", SideBySide, Some(Half)),
        ("3D H-SBS", SideBySide, Some(Half)),
        ("3D SBS Half", SideBySide, Some(Half)),
        ("Half Side by Side", SideBySide, Some(Half)),
        ("3D SBS", SideBySide, None),
        ("Side by Side", SideBySide, None),
        ("3D HOU", TopBottom, Some(Half)),
        ("3D H-OU", TopBottom, Some(Half)),
        ("3D HTAB", TopBottom, Some(Half)),
        ("3D F-TAB", TopBottom, Some(Full)),
        ("Full Over Under", TopBottom, Some(Full)),
        ("3D TAB", TopBottom, None),
        ("Top and Bottom", TopBottom, None),
        ("3D.FA", FrameSequential, None),
        ("Frame Sequential", FrameSequential, None),
        ("3D Row Interlaced", RowInterleaved, None),
        ("3D Column Interleaved", ColumnInterleaved, None),
        ("3D Checkerboard", Checkerboard, None),
        ("Anaglyph", Anaglyph, None),
    ];
    for (marker, layout, sampling) in cases {
        let raw = format!("Known Title 2020 [1080p BluRay {marker} x264 AAC]");
        let parsed = parse(&raw);
        assert_eq!(
            parsed
                .stereoscopy
                .map(|s| (s.presentation, s.layout, s.sampling)),
            Some((StereoPresentation::ThreeD, Some(layout), sampling)),
            "{raw}: {parsed:?}"
        );
        assert_eq!(parsed.quality.as_deref(), Some("1080p"), "{raw}");
        assert_eq!(parsed.year, Some(2020), "{raw}");
        assert_eq!(parsed.normalized_title, "KNOWN TITLE", "{raw}");
    }
}

#[test]
fn stereo_unknown_two_d_and_mixed_are_distinct() {
    use StereoPresentation::*;
    for (marker, expected) in [
        ("", None),
        ("2D", Some(TwoD)),
        ("3D", Some(ThreeD)),
        ("stereoscopic", Some(ThreeD)),
        ("2D+3D", Some(Mixed2d3d)),
        ("2D/3D", Some(Mixed2d3d)),
        ("2D and 3D", Some(Mixed2d3d)),
        ("2D & 3D", Some(Mixed2d3d)),
        ("2D BD+3D BD", Some(Mixed2d3d)),
        ("4K+2D+3D", Some(Mixed2d3d)),
        ("OVA×3+Movie2D&3D+SP", Some(Mixed2d3d)),
        ("OVAx2+Movie3D&2D+SP", Some(Mixed2d3d)),
        ("Title+Movie2D&3D+SP", None),
        ("3D Ver. Viewable in 2D", Some(ThreeD)),
        ("2D to 3D conversion", Some(ThreeD)),
    ] {
        let parsed = parse(&format!("Known Title [1080p {marker} x264]"));
        assert_eq!(
            parsed.stereoscopy.map(|s| s.presentation),
            expected,
            "{marker}: {parsed:?}"
        );
    }
    let mixed = parse("Known Title [2D+3D 1080p HSBS]").stereoscopy.unwrap();
    assert!(mixed.has_3d());
    assert_eq!(mixed.layout, None);
    assert_eq!(mixed.sampling, None);
}

#[test]
fn stereo_conflicts_preserve_evidence_without_selecting_a_layout() {
    for (marker, hint) in [
        ("3D SBS Row Interlaced", "stereo:layout_conflict"),
        ("3D HSBS FSBS", "stereo:sampling_conflict"),
        ("2D 3D SBS", "stereo:presentation_conflict"),
    ] {
        let raw = format!("Known Title [1080p {marker} x264]");
        let analysis = analyze_release_for_target(&raw, &context("Known Title"));
        let candidate = analysis.best_candidate().unwrap();
        assert!(
            candidate.projected.parse_hints.iter().any(|h| h == hint),
            "{raw}: {candidate:?}"
        );
        assert!(candidate.metadata.stereo_evidence.len() >= 2);
        if hint == "stereo:presentation_conflict" {
            assert!(candidate.projected.stereoscopy.is_none());
        } else {
            let stereo = candidate.projected.stereoscopy.unwrap();
            assert!(stereo.has_3d());
            if hint == "stereo:layout_conflict" {
                assert!(stereo.layout.is_none());
            } else {
                assert!(stereo.sampling.is_none());
            }
        }
    }
}

#[test]
fn stereo_title_alias_and_group_collisions_are_protected() {
    for title in [
        "3D Known Title",
        "Known Title 3D",
        "SBS",
        "HSBS",
        "MVC",
        "Full SBS",
    ] {
        let raw = format!("{title} 2020 [1080p BluRay x264]");
        let parsed = best_parse_for_target(&raw, &context(title));
        assert!(parsed.stereoscopy.is_none(), "{raw}: {parsed:?}");
    }
    let mut target = context("Known Title");
    target.aliases.push(ContextAlias {
        name: "3D Another Title".into(),
    });
    let parsed = best_parse_for_target(
        "Known Title [1080p BluRay x264] | 3D Another Title",
        &target,
    );
    assert!(parsed.stereoscopy.is_none(), "{parsed:?}");
    for group in ["SBS", "HSBS", "MVC"] {
        let parsed = parse(&format!("[{group}] Known Title [1080p x264]"));
        assert!(parsed.stereoscopy.is_none(), "{group}: {parsed:?}");
    }
}

#[test]
fn stereo_search_noise_and_broadcasters_do_not_assert_video_stereo() {
    for marker in [
        "SBS 1280x720 x264 AAC",
        "MVC",
        "MVCH-12345",
        "3DA1B2C3",
        "BD3D1234",
        "MPEG-H 3D Audio",
        "3D Noise Reduction",
        "3D LIVE",
        "3D GRADUATION STREAM",
        "3D CGI",
        "3D models",
        "3DS",
        "IMAX 3D CAMRIP",
        "FA",
        "TAB",
        "OU",
    ] {
        let raw = format!("Known Title [1080p {marker}]");
        assert!(
            parse(&raw).stereoscopy.is_none(),
            "{raw}: {:?}",
            parse(&raw)
        );
    }
}

#[test]
fn stereo_compounds_do_not_cross_brackets_or_protected_aliases() {
    for raw in [
        "Known Title [1080p 3D H] [SBS x264]",
        "Known Title [1080p 3D Full] [SBS x264]",
        "Known Title [1080p 3D Side] [by Side]",
    ] {
        let stereo = parse(raw).stereoscopy.unwrap();
        assert_eq!(stereo.layout, None, "{raw}");
        assert_eq!(stereo.sampling, None, "{raw}");
    }
}

#[test]
fn stereo_mvc_and_terminal_extension_do_not_imply_disc_or_remux() {
    let parsed = parse("Known Title [1080p BluRay 3D AVC MVC]");
    let stereo = parsed.stereoscopy.unwrap();
    assert_eq!(stereo.encoding, Some(StereoEncoding::Mvc));
    assert_eq!(stereo.layout, None);
    assert!(!parsed.is_bd_disk);
    assert!(!parsed.is_remux);
    assert!(
        parse("Known Title.2020.1080p.mk3d")
            .stereoscopy
            .unwrap()
            .has_3d()
    );
    assert!(parse("Known Title.2020.1080p.mkv").stereoscopy.is_none());
}

#[test]
fn stereo_unicode_and_separators_keep_existing_quality_and_audio() {
    for marker in [
        "３Ｄ ＨＳＢＳ",
        "3d_h-sbs",
        "3D.Full.SBS",
        "3D&#32;SBS&#32;Half",
    ] {
        let raw = format!("Known Title 2020 [1080p BluRay {marker} x264 AAC]");
        let parsed = parse(&raw);
        assert!(parsed.stereoscopy.unwrap().has_3d(), "{raw}");
        assert_eq!(parsed.quality.as_deref(), Some("1080p"));
        assert_eq!(parsed.audio.map(|a| a.to_string()).as_deref(), Some("AAC"));
    }
}

#[test]
fn stereo_recovery_does_not_resolve_unknown_episode_identity() {
    let mut target = context("Unresolved Collection");
    target.facet_hint = ContextFacetHint::Anime;
    let parsed = best_parse_for_target("Unresolved Collection [1080p 3D HSBS x264]", &target);
    assert_eq!(parsed.disposition, ParseDisposition::Unparseable);
    assert_eq!(parsed.parse_confidence, 0.0);
    assert_eq!(
        parsed.stereoscopy.unwrap().layout,
        Some(StereoLayout::SideBySide)
    );
}
