use super::*;

#[tokio::test]
async fn graphql_get_returns_non_500() {
    let ctx = TestContext::new().await;
    let resp = ctx
        .http_client()
        .get(format!("{}/graphql", ctx.app_url))
        .send()
        .await
        .unwrap();
    // GET on a POST-only endpoint — should not crash
    assert_ne!(resp.status().as_u16(), 500);
}

// ---------------------------------------------------------------------------
// Introspection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn graphql_introspection_query_type() {
    let ctx = TestContext::new().await;
    let body = gql(&ctx, "{ __schema { queryType { name } } }", json!({})).await;
    assert_eq!(body["data"]["__schema"]["queryType"]["name"], "QueryRoot");
}

#[tokio::test]
async fn graphql_introspection_mutation_type() {
    let ctx = TestContext::new().await;
    let body = gql(&ctx, "{ __schema { mutationType { name } } }", json!({})).await;
    assert_eq!(
        body["data"]["__schema"]["mutationType"]["name"],
        "MutationRoot"
    );
}

fn graphql_description_is_blank(value: &Value) -> bool {
    value
        .as_str()
        .is_none_or(|description| description.trim().is_empty())
}

fn graphql_description_contains_em_dash(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|description| description.contains('\u{2014}'))
}

fn record_graphql_description(
    missing: &mut Vec<String>,
    em_dashes: &mut Vec<String>,
    path: String,
    description: &Value,
) {
    if graphql_description_is_blank(description) {
        missing.push(path);
    } else if graphql_description_contains_em_dash(description) {
        em_dashes.push(path);
    }
}

#[tokio::test]
async fn graphql_http_schema_is_fully_documented() {
    let ctx = TestContext::new().await;
    let body = gql(
        &ctx,
        r#"
        {
          __schema {
            queryType { name }
            mutationType { name }
            types {
              kind
              name
              description
              fields(includeDeprecated: true) {
                name
                description
                isDeprecated
                deprecationReason
                args(includeDeprecated: true) {
                  name
                  description
                  isDeprecated
                  deprecationReason
                  type {
                    kind
                    name
                    ofType {
                      kind
                      name
                      ofType {
                        kind
                        name
                        ofType { kind name }
                      }
                    }
                  }
                }
                type {
                  kind
                  name
                  ofType {
                    kind
                    name
                    ofType {
                      kind
                      name
                      ofType { kind name }
                    }
                  }
                }
              }
              interfaces { name }
              inputFields(includeDeprecated: true) {
                name
                description
                isDeprecated
                deprecationReason
                type {
                  kind
                  name
                  ofType {
                    kind
                    name
                    ofType {
                      kind
                      name
                      ofType { kind name }
                    }
                  }
                }
              }
              enumValues(includeDeprecated: true) {
                name
                description
                isDeprecated
                deprecationReason
              }
              possibleTypes { name }
            }
          }
        }
        "#,
        json!({}),
    )
    .await;
    assert_no_errors(&body);

    let schema = &body["data"]["__schema"];
    let query_root = schema["queryType"]["name"]
        .as_str()
        .expect("GraphQL query root name");
    let mutation_root = schema["mutationType"]["name"]
        .as_str()
        .expect("GraphQL mutation root name");
    let type_by_name: BTreeMap<&str, &Value> = schema["types"]
        .as_array()
        .expect("GraphQL schema types")
        .iter()
        .filter_map(|ty| ty["name"].as_str().map(|name| (name, ty)))
        .collect();

    let built_in_scalars = ["Boolean", "Float", "ID", "Int", "String"];
    let mut pending = vec![query_root.to_string(), mutation_root.to_string()];
    let mut visited = std::collections::BTreeSet::new();
    let mut missing = Vec::new();
    let mut em_dashes = Vec::new();

    while let Some(type_name) = pending.pop() {
        if type_name == "SubscriptionRoot" || type_name.starts_with("__") {
            continue;
        }
        if !visited.insert(type_name.clone()) {
            continue;
        }
        let ty = type_by_name
            .get(type_name.as_str())
            .unwrap_or_else(|| panic!("reachable GraphQL type {type_name} must exist"));
        let kind = ty["kind"].as_str().expect("GraphQL type kind");
        let is_root = type_name == query_root || type_name == mutation_root;
        let is_built_in_scalar = kind == "SCALAR" && built_in_scalars.contains(&type_name.as_str());

        if !is_root && !is_built_in_scalar {
            record_graphql_description(
                &mut missing,
                &mut em_dashes,
                format!("type {type_name}"),
                &ty["description"],
            );
        }

        match kind {
            "OBJECT" | "INTERFACE" => {
                for field in ty["fields"]
                    .as_array()
                    .unwrap_or_else(|| panic!("fields for GraphQL type {type_name}"))
                {
                    let field_name = field["name"].as_str().expect("GraphQL field name");
                    let field_path = format!("{type_name}.{field_name}");
                    record_graphql_description(
                        &mut missing,
                        &mut em_dashes,
                        field_path.clone(),
                        &field["description"],
                    );
                    if field["isDeprecated"].as_bool() == Some(true) {
                        record_graphql_description(
                            &mut missing,
                            &mut em_dashes,
                            format!("{field_path} deprecation reason"),
                            &field["deprecationReason"],
                        );
                    }
                    for argument in field["args"]
                        .as_array()
                        .unwrap_or_else(|| panic!("arguments for GraphQL field {field_path}"))
                    {
                        let argument_name =
                            argument["name"].as_str().expect("GraphQL argument name");
                        record_graphql_description(
                            &mut missing,
                            &mut em_dashes,
                            format!("{field_path}({argument_name}:)"),
                            &argument["description"],
                        );
                        if argument["isDeprecated"].as_bool() == Some(true) {
                            record_graphql_description(
                                &mut missing,
                                &mut em_dashes,
                                format!("{field_path}({argument_name}:) deprecation reason"),
                                &argument["deprecationReason"],
                            );
                        }
                        if let Some(argument_type) = graphql_type_leaf_name(&argument["type"])
                            && !built_in_scalars.contains(&argument_type)
                        {
                            pending.push(argument_type.to_string());
                        }
                    }
                    if let Some(field_type) = graphql_type_leaf_name(&field["type"])
                        && !built_in_scalars.contains(&field_type)
                    {
                        pending.push(field_type.to_string());
                    }
                }
                if let Some(implemented_types) = ty["interfaces"].as_array() {
                    for implemented_type in implemented_types {
                        pending.push(
                            implemented_type["name"]
                                .as_str()
                                .expect("GraphQL interface name")
                                .to_string(),
                        );
                    }
                }
                if kind == "INTERFACE" {
                    for possible_type in ty["possibleTypes"].as_array().unwrap_or_else(|| {
                        panic!("possible types for GraphQL interface {type_name}")
                    }) {
                        pending.push(
                            possible_type["name"]
                                .as_str()
                                .expect("GraphQL interface member name")
                                .to_string(),
                        );
                    }
                }
            }
            "INPUT_OBJECT" => {
                for field in ty["inputFields"]
                    .as_array()
                    .unwrap_or_else(|| panic!("input fields for GraphQL type {type_name}"))
                {
                    let field_name = field["name"].as_str().expect("GraphQL input field name");
                    record_graphql_description(
                        &mut missing,
                        &mut em_dashes,
                        format!("{type_name}.{field_name}"),
                        &field["description"],
                    );
                    if field["isDeprecated"].as_bool() == Some(true) {
                        record_graphql_description(
                            &mut missing,
                            &mut em_dashes,
                            format!("{type_name}.{field_name} deprecation reason"),
                            &field["deprecationReason"],
                        );
                    }
                    if let Some(field_type) = graphql_type_leaf_name(&field["type"])
                        && !built_in_scalars.contains(&field_type)
                    {
                        pending.push(field_type.to_string());
                    }
                }
            }
            "ENUM" => {
                for value in ty["enumValues"]
                    .as_array()
                    .unwrap_or_else(|| panic!("enum values for GraphQL type {type_name}"))
                {
                    let value_name = value["name"].as_str().expect("GraphQL enum value name");
                    let value_path = format!("{type_name}.{value_name}");
                    record_graphql_description(
                        &mut missing,
                        &mut em_dashes,
                        value_path.clone(),
                        &value["description"],
                    );
                    if value["isDeprecated"].as_bool() == Some(true) {
                        record_graphql_description(
                            &mut missing,
                            &mut em_dashes,
                            format!("{value_path} deprecation reason"),
                            &value["deprecationReason"],
                        );
                    }
                }
            }
            "UNION" => {
                for possible_type in ty["possibleTypes"]
                    .as_array()
                    .unwrap_or_else(|| panic!("possible types for GraphQL union {type_name}"))
                {
                    pending.push(
                        possible_type["name"]
                            .as_str()
                            .expect("GraphQL union member name")
                            .to_string(),
                    );
                }
            }
            "SCALAR" => {}
            unexpected => panic!("unsupported reachable GraphQL type kind {unexpected}"),
        }
    }

    missing.sort();
    em_dashes.sort();
    assert!(
        missing.is_empty() && em_dashes.is_empty(),
        "public HTTP GraphQL schema documentation is incomplete\nmissing descriptions ({}):\n{}\ndescriptions containing em dashes ({}):\n{}",
        missing.len(),
        missing.join("\n"),
        em_dashes.len(),
        em_dashes.join("\n"),
    );
}

#[tokio::test]
async fn private_account_schema_has_owner_queries_and_never_exposes_credentials() {
    let ctx = TestContext::new().await;
    let sdl = schema_sdl(&ctx);
    for field in [
        "myListAccounts",
        "myListSubscriptions",
        "startListAccountLink",
        "linkListAccountWithKey",
        "completeListAccountLink",
        "unlinkListAccount",
    ] {
        assert!(sdl.contains(field));
    }
    let type_body = |name: &str| {
        sdl.split(&format!("type {name} {{"))
            .nth(1)
            .unwrap()
            .split('}')
            .next()
            .unwrap()
            .to_string()
    };
    let payload = type_body("ListAccountPayload");
    for secret in [
        "accessToken",
        "refreshToken",
        "refreshHandle",
        "credential",
        "clientSecret",
        "appConfig",
    ] {
        assert!(!payload.contains(secret), "account schema exposes {secret}");
    }
    // A member's key goes in as an argument and is described, never returned.
    for name in ["ListAccountKeyLinkPayload", "ListMemberKeyPayload"] {
        let body = type_body(name);
        for line in body.lines().filter(|line| !line.trim().is_empty()) {
            let field = line.trim().split(':').next().unwrap_or_default();
            assert!(
                !["key", "apiKey", "token", "accessToken", "credential"].contains(&field),
                "{name} returns {field}"
            );
        }
    }
    let body = gql(
        &ctx,
        "{ myListAccounts { id provider externalUserId } myListSubscriptions { id } }",
        json!({}),
    )
    .await;
    assert_no_errors(&body);
    assert_eq!(body["data"]["myListAccounts"], json!([]));
    assert_eq!(body["data"]["myListSubscriptions"], json!([]));
}

#[tokio::test]
async fn graphql_introspection_schema_census_matches_contract_baseline() {
    let ctx = TestContext::new().await;
    let sdl = schema_sdl(&ctx);
    assert_eq!(scryer_interface::export_schema_sdl(), sdl);
    assert!(sdl.contains("type QueryRoot"));
    assert!(sdl.contains("type MutationRoot"));
    assert!(sdl.contains("type SubscriptionRoot"));
    assert!(sdl.contains("scalar Date"));
    assert!(sdl.contains("scalar DateTime"));
    assert!(sdl.contains("scalar Long"));

    let body = gql(
        &ctx,
        r#"
        {
          __schema {
            queryType { fields { name } }
            mutationType { fields { name } }
            subscriptionType { fields { name } }
            types { name kind }
          }
        }
        "#,
        json!({}),
    )
    .await;
    assert_no_errors(&body);

    let query_field_count = body["data"]["__schema"]["queryType"]["fields"]
        .as_array()
        .expect("query fields")
        .len();
    let mutation_field_count = body["data"]["__schema"]["mutationType"]["fields"]
        .as_array()
        .expect("mutation fields")
        .len();
    let subscription_field_count = body["data"]["__schema"]["subscriptionType"]["fields"]
        .as_array()
        .expect("subscription fields")
        .len();

    let public_types: Vec<&Value> = body["data"]["__schema"]["types"]
        .as_array()
        .expect("schema types")
        .iter()
        .filter(|ty| {
            ty["name"]
                .as_str()
                .is_some_and(|name| !name.starts_with("__"))
        })
        .collect();
    let kind_count = |kind: &str| -> usize {
        public_types
            .iter()
            .filter(|ty| ty["kind"].as_str() == Some(kind))
            .count()
    };
    let query_field_names: Vec<&str> = body["data"]["__schema"]["queryType"]["fields"]
        .as_array()
        .expect("query fields")
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    let mutation_field_names: Vec<&str> = body["data"]["__schema"]["mutationType"]["fields"]
        .as_array()
        .expect("mutation fields")
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    let public_type_names: Vec<&str> = public_types
        .iter()
        .filter_map(|ty| ty["name"].as_str())
        .collect();

    // cutover: the wanted/cutoff/search surface changed — the top-level
    // `wantedItems` became the derived Missing/Upgrades view, the unpaged
    // `cutoffUnmetTitles` query was dropped, the four per-item trigger mutations +
    // `resetWantedItem` were replaced by `triggerAcquisitionSearch` /
    // `cancelAcquisitionSearch` / `acquisitionSearchJob`, and the payloads gained
    // convergence/recency fields (with new enums + the job payload).
    // 0.17.0 re-pinned this census for the intentionally API-breaking release:
    // query fields moved 119 -> 121 with the added Query.episodeById /
    // Query.collectionById id-anchored lookups; the other counts are unchanged.
    // 0.17.0 API surface trim (root wave): removed 5 dead query roots
    // (discoverySyncStatus, libraryScanSession, mediaServerConnection,
    // outboundRateLimitSnapshot, upstreamSchedulerSnapshot), 1 dead mutation
    // (queueReplacementRelease), and the 5 exclusive snapshot payload OBJECT types.
    // Two of those have since been reinstated with real consumers, and the
    // counts below carry the reinstatement rather than the trim:
    // queueReplacementRelease (mutation) and libraryScanSession (query).
    // 0.17.0 API surface trim (field wave): removed dead output fields inside
    // consumed types plus 4 never-selected OBJECT types (DiscoverySyncRunPayload and the
    // ExternalImportLibrarySetting{Application,Evidence,Value}Payload trio); the trio's
    // exclusive enums (ExternalImportLibrarySetting{Confidence,Disposition,Key}) drop with
    // it. Root-field counts unchanged; OBJECT 259->255, ENUM 80->77, public types 498->491.
    // 0.17.0 semantic waves (slice 5): stringly String fields became real enums
    // (+12 ENUM), and QueueDownloadScopePayload / ProviderConfigFieldValue became unions
    // (+2 UNION with 6 scope + 5 config-value member OBJECT types, +11 OBJECT).
    // Root-field counts unchanged; ENUM 77->89, OBJECT 255->266, public types 491->516.
    // Library catalog filter options add one query and two payload OBJECT types;
    // external subtitle listing and blocklist lookup add two query roots and eight public types
    // (five OBJECT, two INPUT_OBJECT, and one ENUM support types).
    // Catalog bootstrap no longer exposes filesystem reachability as a blocking query.
    // User login suspension adds one mutation and its INPUT_OBJECT.
    // 2026-07-23 hotfix 0.17.1: server-side interactive release-search jobs
    // (start/cancel mutations + poll query) replace the single join-all
    // `searchReleases` round-trip for the UI so per-indexer results stream in.
    // Adds 1 query root, 2 mutation roots, 3 payload OBJECTs
    // (InteractiveReleaseSearch{,Indexer}Payload, CancelInteractiveReleaseSearchPayload)
    // and 2 ENUMs (state + indexer status): query 118->119, mutation 164->166,
    // OBJECT 273->276, ENUM 90->92, public types 527->532.
    // Async Prowlarr discovery adds one status query, one start mutation, and
    // one input object: query 119->120, mutation 166->167, public types 532->533.
    // The kind-neutral externalImportWarmupStatus supersedes
    // externalImportArrSourceWarmupStatus; deprecating the old field hides it
    // from default (includeDeprecated: false) introspection: query 120->119.
    // The direct-path previewManualImportPath query was removed in favor of
    // server-owned manual-import selections: query 119->118.
    // Server-owned manual-import selection and cancellable external-source
    // warmup add two mutation roots: mutation 167->169.
    // The current selection and classification contract adds six payload
    // objects and one enum: OBJECT 276->282, ENUM 92->93, public types 533->540.
    // Cached queue paging and revision sync add two payload objects and one
    // sort enum: OBJECT 282->284, ENUM 93->94, public types 540->543. The new
    // roots replace deprecated fields in default introspection, so root field
    // counts remain unchanged.
    // Explicit indexer download-client mapping adds one query root, one mutation
    // root, and three payload objects: query 118->119, mutation 169->170,
    // OBJECT 284->287, INPUT_OBJECT 153->154, public types 543->547.
    // Episode media availability adds one object and one enum: public types
    // 547->549, OBJECT 287->288, ENUM 94->95.
    // Restore the operator-selected replacement mutation: mutation 170->171.
    // Provider-level indexer/download-client compatibility adds one object so
    // unsaved indexer drafts can use the same server-derived routing contract.
    // Cached title credits add one payload object behind the new `Title.credits`
    // resolver; the field hangs off an existing type, so root counts are
    // unchanged: OBJECT 291->292, public types 560->561.
    // Plugin auto-update settings add one query root, one mutation root, one
    // payload object, and one input object: query 119->120, mutation 175->176,
    // OBJECT 292->293, INPUT_OBJECT 158->159, public types 561->563.
    // The season-scoped panel's `Collection.episodeRecordsTotal` hangs off an
    // existing type, so no census counts change.
    // Dashboard landing page adds the dashboardActivityStats and storageRoots
    // query roots plus their three payload objects
    // (DashboardActivityStatsPayload, ActivityWindowCountsPayload,
    // StorageRootUsagePayload): query 120->122, OBJECT 293->296,
    // public types 563->566.
    // Manual-imports dashboard panel adds PendingImportReasonClassValue beside
    // the free-text reason (plus createdAt/sizeBytes on the existing item
    // payload, which add no types), and recently-imported enrichment adds
    // libraryId/sizeBytes to TitleHistoryEventPayload: ENUM 99->100,
    // public types 566->567. Root-field counts unchanged.
    // Torrent seeding profiles add two query roots, five mutation roots, three
    // payload objects, four inputs, and two enums: query 119->121, mutation
    // 175->180, OBJECT 291->294, INPUT_OBJECT 158->162, ENUM 99->101, public
    // types 560->569.
    // Queue seeding progress adds six nullable fields on DownloadQueueItemPayload
    // and the enum behind one of them: ENUM 101->102, public types 569->570.
    // Root-field and OBJECT counts are unchanged — the fields are additive on an
    // existing type.
    // Post-import handoff adds one enum plus one field on the seeding-profile
    // payload and both of its inputs: ENUM 102->103, public types 570->571.
    // Root-field, OBJECT and INPUT_OBJECT counts are unchanged — the fields are
    // additive on existing types.
    // (Merged with release-0.18.17: both branches' additions above are
    // additive from the shared 119/175/291/158/99/560 base.)
    // Batched rename preview adds the mediaRenamePreviewBulk query root and its
    // input object, reusing the existing plan payload: query 124->125,
    // INPUT_OBJECT 163->164, public types 578->579.
    // Renaming as a background job adds the renameTitles mutation root, its
    // input object and payload, and a TITLE_RENAME value on the existing job
    // key enum: mutation 181->182, OBJECT 299->300, INPUT_OBJECT 164->165,
    // public types 579->581. ENUM is unchanged: the value joins an enum that
    // already exists.
    // Deprecating applyMediaRename and applyMediaRenameBulk in favour of that
    // job drops both from this count: introspection omits deprecated fields
    // unless asked for them, so the census measures live surface. The fields
    // and their input types are still served: mutation 182->180.
    // Minimum-seeder admission adds the setMinimumSeedersFloor mutation root
    // and its input object: mutation 180->181, INPUT_OBJECT 165->166, public
    // types 581->582. The threshold itself rides on existing types as
    // SeedingProfilePayload.minimumSeeders and
    // DefaultSeedingProfilePayload.minimumSeedersFloor, so OBJECT is unchanged.
    // Login-factor verification then adds five mutations for enrollment and
    // passkey/TOTP completion: mutation 181->186, public types 582->585,
    // OBJECT 300->301, and INPUT_OBJECT 166->168.
    // Temporary-password replacement adds one mutation and its input object:
    // mutation 186->187, INPUT_OBJECT 168->169, public types 585->586.
    // Durable indexer HTTP error history adds the indexerErrors and indexerError
    // query roots: query 127->129. The title-id work changes existing metadata
    // query inputs and payload fields only, so it adds no roots or named types.
    // OAuth client registrations add their create, update, and delete mutation
    // roots and six named types: mutation 187->190, public types 593->599.
    // API-key lifecycle adds two queries, two mutations, and five named types:
    // query 129->131, mutation 190->192, public types 599->604.
    // Application upgrade status adds one query root, one payload object, and
    // two enum types: query +1, OBJECT +1, ENUM +2, public types +3.
    // Starting an application upgrade adds one mutation root, its input, and
    // its acceptance payload. The status run fields reuse JobRunPayload and
    // APPLICATION_UPGRADE joins the existing JobKeyValue enum: mutation +1,
    // OBJECT +1, INPUT_OBJECT +1, public types +2.
    // Media-server playback links add one object behind existing title, episode,
    // and calendar payloads: OBJECT 314->315, public types 609->610.
    // Query, mutation, subscription, input-object, and enum counts are unchanged.
    // Manual-import video facts add one object: OBJECT 315->316, public types 610->611.
    // Live import activity adds one query, one mutation, one subscription, two
    // payload objects, and one phase enum: query 132->133, mutation 193->194,
    // subscription 13->14, OBJECT 316->318, ENUM 110->111, public types 611->614.
    // Local movie-entity detail adds one query. In the combined release schema,
    // its payload graph also makes one existing enum reachable: query 133->134,
    // ENUM 111->112, public types 614->615.
    // Plugin config preset overrides add one key/value payload object behind the
    // existing config option type: OBJECT 318->319, public types 615->616.
    // Account-security reauthentication adds three mutation roots without
    // changing the established factor-mutation payload contracts: mutation 194->197.
    // The maintenance-rule authoring surface adds four query roots (list, get,
    // revisions, action catalog) and six mutation roots (create, matcher edit,
    // metadata edit, delete, validate, preview), with nine payload objects, six
    // inputs, and six enums: query 134->138, mutation 197->203, OBJECT 319->328,
    // INPUT_OBJECT 173->179, ENUM 112->118, public types 616->637.
    // The maintenance dark evaluator adds four query roots (candidates,
    // evaluation runs, instance gates, exclusions) and five mutation roots
    // (rule mode, instance gates, exclude, remove exclusion, run now), with six
    // payload objects, three inputs, and one candidate-state enum:
    // query 138->142, mutation 203->208, OBJECT 328->334, INPUT_OBJECT
    // 179->182, ENUM 118->119, public types 637->647. MAINTENANCE_RULE_EVALUATION
    // joins the existing job key enum, so it adds no type.
    // The maintenance action executor adds the maintenanceActionRuns query
    // root, the setMaintenanceRuleArming and runMaintenanceActionHandlerNow
    // mutation roots, the action-run payload object, the arming input, and the
    // arming enum: query 142->143, mutation 208->210, OBJECT 334->335,
    // INPUT_OBJECT 182->183, ENUM 119->120, public types 647->650.
    // LIFECYCLE_ACTION_HANDLING joins the existing job key enum.
    // Media-server watch-signal sync adds no GraphQL surface of its own:
    // MEDIA_SERVER_SIGNAL_SYNC joins the existing job key enum, so every count
    // below is unchanged.
    //
    // ── The library-location feature line, merged in beside the maintenance
    // line above. Its paragraphs below narrate that branch's own history, so
    // their running totals are the location branch's totals; the asserted
    // numbers at the bottom are the union of both branches over the shared
    // base (query 134 + 9 maintenance + 7 location = 150; mutation 197 + 13 +
    // 5 = 215; OBJECT 319 + 16 + 50 = 385; INPUT_OBJECT 173 + 10 + 10 = 193;
    // ENUM 112 + 8 + 20 = 140; public types 616 + 34 + 80 = 730). ──
    //
    // Folder-match correction adds the changeTitleFolderPreview query root and
    // the applyTitleFolderChange mutation root, four payload objects (preview,
    // apply, title ref, displaced-title repair), two inputs, and three enums
    // (ownership state, resolution, outcome): query +1, mutation +1, OBJECT +4,
    // INPUT_OBJECT +2, ENUM +3, public types +9.
    // The stated baseline had also drifted one feature behind before this change
    // — an earlier query root, mutation root, payload object, input object, and
    // enum landed without updating these numbers — so the totals below absorb
    // that drift as well: query 134->136, mutation 197->199, OBJECT 319->324,
    // INPUT_OBJECT 173->176, ENUM 112->116, public types 616->628.
    // Root-move location operations add the locationOperationPreview and
    // locationOperation query roots and the startLocationOperation /
    // cancelLocationOperation / resumeLocationOperation mutation roots, with
    // seventeen payload objects (preview, plan counts, per-kind count, plan
    // section, plan item, selection classification, classification group,
    // classified title, free-space estimate, verification statement, plan
    // confirmation, operation, operation counters, title checkpoint, start,
    // cancel, resume), three inputs (destination, preview, start), and seven
    // enums (operation type, execution mode, operation state, title class, plan
    // item kind, checkpoint state, confirmation requirement): query 136->138,
    // mutation 199->202, OBJECT 324->341, INPUT_OBJECT 176->179, ENUM 116->123,
    // public types 628->655. The LOCATION_OPERATION job key joins the existing
    // JobKeyValue enum, so it adds no type.
    // The dedup/rename asset listing (T090, FR-091, US8.1/US8.4) adds the
    // locationOperationAssets query root and four payload objects (the listing,
    // one title's assets, one renamed asset, one deduplicated asset): query
    // 138->139, OBJECT 352->356, public types 673->677. The per-title state
    // reuses the existing LocationTitleCheckpointStateValue, so ENUM is
    // unchanged, and nothing is added to the operation payload itself: the
    // per-file identities live in the stored plan, which a progress poll has no
    // reason to load.
    // The two root-scoped workflows (T064, US4 and US5, FR-020 to FR-029) add
    // the locationRootChangePreview and locationRootConsolidationPreview query
    // roots, thirteen payload objects (root-change preview, consolidation
    // preview, title accounting, blocked title, root identity retention,
    // content inventory, content bucket, content entry, sampled paths,
    // retirement contract, retirement blocker, consolidation classification,
    // default-root transfer), four inputs (the two preview inputs and the two
    // start targets), and one enum for FR-027's three content classes:
    // query 139->141, OBJECT 356->369, INPUT_OBJECT 179->183, ENUM 131->132,
    // public types 678->696. No new mutation: both workflows confirm through
    // the existing startLocationOperation, whose input gained the two
    // root-scoped destination variants beside the selection it already carried.
    //
    // FR-020 is one settings action with two destinations, and the surface was
    // folded to match it: `locationRootChangePreview` and
    // `locationRootConsolidationPreview` became one `locationRootScopePreview`
    // (query 150->149), and the four inputs became two —
    // `LocationRootScopePreviewInput` and `LocationRootScopeTargetInput`
    // replace `LocationRootChangePreviewInput`,
    // `LocationRootConsolidationPreviewInput`, `LocationRootChangeTargetInput`
    // and `LocationRootConsolidationTargetInput` (INPUT_OBJECT 191->189,
    // public types 727->725). No object and no enum changed:
    // `LocationRootScopePreviewPayload` was already one payload for both
    // branches, and `StartLocationOperationInput` swapped its `rootChange` and
    // `rootConsolidation` fields for one `rootScope`.
    // OAuth-bound Jellyfin account linking (merged from main) adds one mutation
    // root and reuses the existing linked-account payload: mutation +1, and its
    // main-side census (OBJECT +1, INPUT_OBJECT +1) rides along.
    // ── 0.19.9 through 0.19.11, merged from the release line on top of the
    // totals above (the OAuth-bound Jellyfin link was already counted). ──
    // Multi-episode file deletion adds the deleteEpisodeFilesPreview query and
    // the deleteEpisodeFiles mutation, three payload objects, and two inputs:
    // query +1, mutation +1, OBJECT +3, INPUT_OBJECT +2, public types +5.
    // Advanced monitoring adds the season/series-movie selection payload pair,
    // its input pair, and the anime-movie metadata payload behind the existing
    // metadata series type: OBJECT +3, INPUT_OBJECT +2, public types +5.
    // Query, mutation, and subscription roots are unchanged; ADVANCED joins the
    // existing MonitorTypeValue enum.
    // Stored OAuth client kinds add one enum behind the existing registration
    // payload and create input: ENUM +1, public types +1.
    // Totals: query 149->150, mutation 219->220, OBJECT 381->387,
    // INPUT_OBJECT 194->198, ENUM 139->140, public types 726->737.
    // The srrdb filename recovery switch is an additive field on the existing
    // general settings payload and update input, so no census count moves.
    // The instance-wide feature switches add the actor-only `instanceFeatures`
    // query and its `InstanceFeaturesPayload`: query 150->151, OBJECT 387->388,
    // public types 737->738. The two switches themselves are additive fields on
    // the existing general settings payload and update input, so INPUT_OBJECT,
    // ENUM, mutation, and subscription counts are unchanged.
    // Admin-defined title tags add the `titleTagDefinitions` registry read,
    // which any authenticated caller may make because the tag picker and the
    // catalog filter both need the vocabulary: query 151->152.
    // Request rules (spec 0003 section 7) add nine query roots: the three
    // authoring reads (`requestRuleSets`, `requestRuleSet`,
    // `requestRuleRevisions`), the instance gate, the two decision reads
    // (`requestRuleDecision`, `requestRuleDecisions`), the Rules Context
    // Reference document, the requester pre-flight, and `titleClaims`.
    // Query 151->160.
    // In-library pending-import candidates add
    // MetadataSearchItemPayload.existingTitleId and
    // ResolvePendingImportInput.attachToExistingTitle. Both hang off existing
    // types, so no census counts change.
    // Caps-driven routing categories add IndexerConfigPayload.capsCategories
    // and its IndexerCapsCategoryPayload element type: OBJECT 407->408,
    // public types 779->780. Root-field, input-object, and enum counts are
    // unchanged.
    // Title-aware rule-editor previews add the `ruleSet` read and tracked rule
    // packs add `trackedRulePacks`: query 161->163. Schema-2 maintenance
    // sequences add the `maintenanceActionStepDescriptors` catalog read:
    // query 163->164.
    // Live transfer progress (0.20.0) adds four reads: the aggregate
    // `locationTransferSummary`, the paged `locationTransferPage` and
    // `locationTransferFiles`, and `locationTransferTitleDetail` for one
    // title's exception. Failed-move recovery adds
    // `locationOperationRetrySelection`, the prefilled Plan again selection.
    // Query 164->169.
    // Reinstate `libraryScanSession`, the by-id scan snapshot read: query
    // 169->170. The 0.17.0 root wave trimmed it as *dead*, not as a duplicate,
    // and it now has a consumer again - the web client resolves a session that
    // dropped out of `activeLibraryScans` by id, to learn whether it completed
    // or merely went invisible. Nothing else answers that: `activeLibraryScans`
    // takes no argument and lists only live sessions, and `libraryScanState` is
    // a push stream. Same shape as the `queueReplacementRelease` reinstatement.
    // Public lists add ten reads: the provider catalog, followed lists, one
    // list, its titles, its sync history, three previews, exclusions and
    // member policies. Query 174->184. Server-wide list provider settings add
    // one more read: 184->185. Keeping the jobs view current adds
    // `latestJobRuns`, the newest run per job in one read: 185->186.
    assert!(query_field_names.contains(&"latestJobRuns"));
    // Canonical list vocabulary adds one authenticated registry query.
    // Scheduled scripts add `validateScriptSchedule`, the schedule check and
    // next-run preview the script editor calls before saving: 191->192.
    assert!(query_field_names.contains(&"validateScriptSchedule"));
    // Rebased to the release tree: work that landed without moving this
    // census (installed plugin settings, download password retry, and others)
    // brings query fields to 195. Scheduled scripts as jobs add no root field;
    // they ride `jobs`, `jobRuns`, and `triggerJob` with a `customJobId`.
    assert_eq!(
        query_field_count, 195,
        "query fields: {query_field_names:?}"
    );
    // First-class proxies (WP4) add one mutation, resetProxyHostKey: SSH host
    // keys are pinned on trust-on-first-use, so a legitimate server rekey needs
    // an explicit operator-driven way to forget the pin. Every other proxy and
    // download-client change in that work package is additive fields on types
    // that already existed. 215->216.
    // Indexer search (spec 0002) adds two mutations on the interactive-search
    // job: issueInteractiveReleaseCandidateToken and queueUnlinkedRelease.
    // 216->218.
    // Admin-defined title tags add four mutations: the per-title patch
    // (updateTitleTags) plus the three registry writes beside the delay
    // profiles (create/update/deleteTitleTagDefinition). 220->224.
    // Series-movie tags add a fifth, updateSeriesMovieTags, beside
    // setSeriesMovieMonitored: a series movie is a link row rather than a
    // title, so its tag patch takes link ids and cannot ride updateTitleTags.
    // 224->225.
    // Request rules add eleven mutations: six authoring roots (create, matcher
    // edit, metadata edit, mode, delete, validate), the author-side preview,
    // the instance gate, and the three administrator claim operations.
    // 220->231.
    // Title-aware rule-editor previews add `testRuleSet`; tracked rule packs
    // add install, update preview, update, settings, copy, and delete actions:
    // mutation 236->243.
    // Failed-move recovery adds `abandonLocationOperation`, the way out of a
    // stalled operation whose storage is not coming back: mutation 247->248.
    // Public lists add nine mutations: follow, edit, enable, sync one, sync
    // all, unfollow, add and remove an exclusion, and set a member's list
    // policy. 250->259. Changing a list provider's server-wide settings adds
    // one more: 259->260.
    // Reopening a dismissed request and saving one catalog layout add
    // `reopenMediaRequest` and `setMyCatalogView`: 265->267.
    // Rebased to the release tree, as with the query count above: 267->271.
    // Linking a list account with a key the member supplies adds
    // `linkListAccountWithKey`: 271->272.
    // Abandoning an account link the member walked away from adds
    // `cancelListAccountLink`: 272->273.
    assert_eq!(
        mutation_field_count, 273,
        "mutation fields: {mutation_field_names:?}"
    );
    // Cross-library transfer (T082, FR-055/FR-056) surfaces destination-title
    // detection on the existing classified-title payload: five additive fields
    // and one new enum for the match outcome (unique, none, ambiguous,
    // same-name-without-identity), so ENUM 123->124 and public types 655->656.
    // No new object, input, query, or mutation.
    // Series↔anime facet conversion (T083, FR-057/FR-058) adds one field on the
    // same classified-title payload plus the conversion payload, its per-setting
    // payload, and the setting-disposition enum (becomes invalid, resets,
    // changes meaning): OBJECT 341->343, ENUM 124->125, public types 656->659.
    // The FR-060/FR-062 link and collection dispositions ride the existing plan
    // items behind new reason codes, so they add no type.
    // Merging into an existing destination title (T085, US7, FR-063 to FR-071)
    // adds the FR-071 preview summary on the existing preview payload and the
    // named candidates behind the ids-only ambiguous list. Nine payload objects
    // (merge preview, blocked record, destination-wins entry, table
    // disposition, role change, reserved-tag conflict, media-request repoint,
    // dropped category, ambiguous candidate) and five enums (disposition,
    // block reason, media role, role-change reason, post-merge work):
    // OBJECT 343->352, ENUM 125->130, public types 659->673. No new query,
    // mutation, or input: `merges` and `ambiguousDestinationCandidates` are
    // additive fields on payloads that already exist.
    // Retiring the direct root write (T093, FR-077/SC-009) deprecates the
    // TitleOptionsInput.rootFolderId input field. Deprecated input fields drop
    // out of default introspection the way deprecated output fields do, but
    // this census counts types and root fields only, so every count below is
    // unchanged.
    // Reaching the "files are already there" adoption engine (T052, US3,
    // FR-050 to FR-053) adds one enum, LocationExecutionModeInput, and an
    // optional `mode` field on each of the two existing location inputs:
    // ENUM 130->131, public types 677->678. The requestable enum is narrower
    // than the reported LocationExecutionModeValue on purpose: CATALOG_ONLY is
    // derived from a fileless selection (FR-076) and is never requestable.
    // Query, mutation, subscription, OBJECT, and INPUT_OBJECT counts are
    // unchanged: both fields join input objects that already exist.
    // Descoping the merge engine to "media file records and history, everything
    // else retires with the title" (FR-063 to FR-071) removes five merge payload
    // objects (table disposition, reserved-tag conflict, media-request repoint,
    // dropped category, destination-wins entry) and two merge enums (disposition,
    // post-merge work); folding US4's root change and US5's consolidation onto
    // one root-scoped planner replaces their two preview payloads with one
    // (`LocationRootScopePreviewPayload`): OBJECT 385->379, ENUM 140->138,
    // public types 730->722.
    // Folding the surface to match — one `locationRootScopePreview` query and
    // one `rootScope` start target in place of the change/consolidation pair —
    // then removes two inputs: `LocationRootScopePreviewInput` and
    // `LocationRootScopeTargetInput` replace the four
    // `LocationRootChange*`/`LocationRootConsolidation*` inputs, so
    // INPUT_OBJECT 193->191 and public types 722->720. Query fields drop by one
    // (150->149) with the second preview root; OBJECT and ENUM are unchanged.
    // Live transfer progress (0.20.0) adds three subscriptions that mirror the
    // summary, page, and files reads so Activity can follow a transfer without
    // polling: subscription 14->17.
    assert_eq!(subscription_field_count, 17);
    // Indexer search (spec 0002): the query subject on the interactive-search
    // job and the unlinked grab add the types below on top of the proxies
    // census. Combined with the location-surface fold above, the totals are
    // public types 720->724, OBJECT 379->380, INPUT_OBJECT 191->193, and
    // ENUM 138->139.
    // Admin-defined title tags add four objects (the definition, the rewrite
    // counts, and the mutation and deletion payloads that carry them) and three
    // inputs (create, update, and the per-title tag patch): OBJECT 388->392,
    // INPUT_OBJECT 198->201, public types 738->745. `TitleCatalogFilterInput.tags`
    // and the `updateTitleTags` result are additive on types that already
    // exist, and no enum joins the schema.
    // Series-movie tags and the maintenance tag actions add one input,
    // UpdateSeriesMovieTagsInput: INPUT_OBJECT 201->202, public types 745->746.
    // Everything else in that work is additive on types that already exist -
    // `tags` on the series-movie link payload, the action spec and the action
    // input, `seriesMovieCount` on the tag definition, `seriesMovies` on the
    // rewrite counts, `requiresTags` on the action descriptor - and the two new
    // action kinds are values inside the existing MaintenanceActionKind enum,
    // so OBJECT and ENUM are unchanged.
    // Request rules add fourteen objects (rule set, revision, detail, delete
    // payload, validation payload, reason, vote, decision, author preview,
    // requester pre-flight, instance gates, title claim, media-request lease,
    // and the request's submit-time metadata), eleven inputs (six authoring,
    // the preview pair, the gate, and the three claim operations), and six
    // enums (evaluation mode, decision outcome, vote, and the three lifecycle
    // claim enums): OBJECT 388->402, INPUT_OBJECT 198->209, ENUM 140->146,
    // public types 738->769. The additive fields on the existing media-request
    // payload and its three inputs add no type.
    // Provider config schemas add the field-condition payload (visibleWhen /
    // requiredWhen on a plugin config field) and its operator enum:
    // OBJECT 406->407, ENUM 146->147, public types 777->779. The `advanced`
    // flag is an additive field on the existing config-field payload.
    // Rule-editor scoring previews add seven payload objects and two inputs;
    // tracked rule packs add four payload objects and one input. Together they
    // move OBJECT 408->419, INPUT_OBJECT 213->216, and public types 780->794;
    // the additions introduce no enum or scalar types.
    // Native media analysis adds 25 payload objects, two disc-selection inputs,
    // and three enums for diagnostics and provenance: public types 794->824.
    // Four mutations list episode targets, inspect diagnostics, select disc
    // titles, and map episodes; query and subscription roots are unchanged.
    // Schema-2 maintenance sequences add seven objects (the sequence, its
    // steps, descriptors, progress, checkpoints, and job receipts), three
    // input objects, and two enums for step kind and search condition:
    // public types 824->836, OBJECT 444->451, INPUT_OBJECT 218->221, and
    // ENUM 150->152.
    // Live transfer progress adds the snapshot, title, and file payloads
    // (`LocationTransferSnapshotPayload`, `LocationTransferTitlePayload`,
    // `LocationTransferFilePayload`); conflict resolution during transfers
    // adds the per-title `LocationTitleFoldersPayload` on the preview; and
    // failed-move recovery adds `LocationRetrySelectionPayload` with its
    // `LocationRetryTitlePayload` rows and `AbandonLocationOperationPayload`:
    // OBJECT 451->458, public types 836->843. Reason codes and the third
    // requestable mode are additive fields and enum values on types that
    // already existed, so INPUT_OBJECT and ENUM counts are unchanged.
    // Unified automatic-search semantics add the one-variant
    // `AcquisitionSearchIntentValue` enum, which names why a search was
    // started: ENUM 152->153, public types 843->844. Nothing else in the
    // schema moved with it - it is an additive enum on existing types.
    // Parsed stereoscopy adds one payload object and four enums for
    // presentation, layout, sampling, and encoding: public types 844->849.
    // The release-numbering bridge adds the `ReleaseNumberingValue` enum, which
    // names the episode order a title's releases are read in. It is surfaced as
    // an additive field on `TitleOptionsInput` and `TitlePayload`, both of which
    // already existed: ENUM 157->158, public types 849->850, and no change to
    // OBJECT or INPUT_OBJECT.
    // The library-scoped Manage Subtitles permission restructures the subtitle
    // search reply: `searchSubtitles` now returns `SubtitleSearchPayload`
    // (status, resolved language, available languages, results) instead of a
    // bare result list, so provider readiness is the server's answer rather
    // than something the client sniffs. That adds the payload object and the
    // `SubtitleSearchStatus` enum: OBJECT 459->460, ENUM 158->159, public types
    // 850->852. Root-field and INPUT_OBJECT counts are unchanged - the mutation
    // already existed, `SearchSubtitlesInput` only made `language` optional, and
    // MANAGE_SUBTITLES is a value on the existing `LibraryPermissionValue`.
    // Which import actions a download offers is now the server's answer rather
    // than something each surface re-derives, so `DownloadQueueItemPayload`
    // carries a `DownloadImportActionsPayload`: OBJECT 460->461, public types
    // 852->853. It is an additive field on a type that already existed, so no
    // other count moves.
    // The catalogue's file and attention filters add two fields to the existing
    // `TitleCatalogFilterInput`, four counts to the existing
    // `TitleCatalogFilterCountsPayload`, and the `TitleCatalogPresenceValue` enum
    // the first of them needs: ENUM 162->163 and public types 864->865. Both
    // other changes are additive fields on types that already existed, so no
    // OBJECT or INPUT_OBJECT count moves.
    // Explicit grab destinations add indexerGrabClients and downloadClientCategories,
    // queueIndexerSearchAssignment, two objects, and IndexerGrabSelectionInput.
    // Dashboard summary and recent imports add two queries, three objects, and
    // DashboardImportKindValue. Together: queries 170->174, mutations 248->249,
    // OBJECT 461->466, INPUT_OBJECT 221->222, ENUM 159->160, public types 853->860.
    for name in [
        "indexerGrabClients",
        "downloadClientCategories",
        "dashboardSummary",
        "dashboardRecentImports",
    ] {
        assert!(query_field_names.contains(&name), "missing query {name}");
    }
    assert!(mutation_field_names.contains(&"queueIndexerSearchAssignment"));
    for name in [
        "IndexerGrabClientPayload",
        "DownloadClientCategoriesPayload",
        "IndexerGrabSelectionInput",
        "DashboardSummaryPayload",
        "DashboardIndexerStatsPayload",
        "DashboardRecentImportPayload",
        "DashboardImportKindValue",
    ] {
        assert!(public_type_names.contains(&name), "missing type {name}");
    }
    // External intake adds one mutation, one input, one payload, and two enums.
    assert!(mutation_field_names.contains(&"submitExternalRelease"));
    for name in [
        "SubmitExternalReleaseInput",
        "ExternalReleasePayload",
        "ExternalReleaseProtocolValue",
        "ExternalReleaseStatusValue",
    ] {
        assert!(public_type_names.contains(&name), "missing type {name}");
    }
    // Public lists add ten queries and nine mutations with their payload,
    // input and enum types, and give media requests an origin: public types
    // 864->909, OBJECT 467->492, INPUT_OBJECT 223->230, ENUM 162->175.
    for name in [
        "listProviders",
        "listSubscriptions",
        "listSubscription",
        "listSubscriptionMemberships",
        "listSyncRuns",
        "listSubscriptionPreview",
        "listUrlPreview",
        "listSourcePreview",
        "listExclusions",
        "listMemberPolicies",
    ] {
        assert!(query_field_names.contains(&name), "missing query {name}");
    }
    for name in [
        "subscribeList",
        "updateListSubscription",
        "setListSubscriptionEnabled",
        "syncListSubscription",
        "syncAllLists",
        "unsubscribeList",
        "addListExclusion",
        "removeListExclusion",
        "setMemberListPolicy",
    ] {
        assert!(
            mutation_field_names.contains(&name),
            "missing mutation {name}"
        );
    }
    for name in [
        "ListProviderPayload",
        "ListSubscriptionPayload",
        "ListMembershipPagePayload",
        "ListSyncRunPayload",
        "ListPreviewPayload",
        "ListExclusionPayload",
        "MemberListPolicyPayload",
        "MediaRequestOriginPayload",
        "SubscribeListInput",
        "UpdateListSubscriptionInput",
        "ListSourceInput",
        "AddListExclusionInput",
        "ListScope",
        "ListMode",
        "ListOnLeave",
        "ListSyncState",
        "ListPolicy",
        "ListExclusionScope",
        "ListSyncRunOutcome",
        "ListMembershipState",
        "ListFilterKind",
        "ListAuthBadge",
        "ListSourceParamType",
        "ListNoteTone",
        "MediaRequestOriginKind",
    ] {
        assert!(public_type_names.contains(&name), "missing type {name}");
    }
    // Server-wide list provider settings add one query, one mutation, the
    // settings and field payloads and the change input; a title's public-list
    // provenance adds one payload on an additive title field: public types
    // 909->913, OBJECT 492->495, INPUT_OBJECT 230->231.
    assert!(query_field_names.contains(&"listProviderSettings"));
    assert!(mutation_field_names.contains(&"updateListProviderSettings"));
    for name in [
        "ListProviderSettingsPayload",
        "ListProviderSettingFieldPayload",
        "ListProviderSettingChangeInput",
        "TitleListMembershipPayload",
    ] {
        assert!(public_type_names.contains(&name), "missing type {name}");
    }
    // The rule tester's listing facts add one input and one payload, and the
    // media file's frozen listing adds one payload:
    // public types 913->916, OBJECT 495->497, INPUT_OBJECT 231->232.
    assert!(public_type_names.contains(&"RuleSetTestListingInput"));
    assert!(public_type_names.contains(&"RuleSetTestListingPayload"));
    assert!(public_type_names.contains(&"ReleaseListingPayload"));
    // The catalogue's file and attention filters add the
    // `TitleCatalogPresenceValue` enum on top: ENUM 175->176, public types
    // 916->917; their other changes are additive fields on existing types.
    assert!(public_type_names.contains(&"TitleCatalogPresenceValue"));
    // Moving recycle bin entries with a changed location reports the move and
    // its failures in two payloads: public types 917->919, OBJECT 497->499.
    assert!(public_type_names.contains(&"RecycleBinRelocationPayload"));
    assert!(public_type_names.contains(&"RecycleBinRelocationFailurePayload"));
    // Private account management adds six credential-free output objects.
    // Per-device catalog layouts add one payload, two inputs and two enums:
    // public types 925->930, OBJECT 505->506, INPUT_OBJECT 232->234,
    // ENUM 176->178.
    assert!(public_type_names.contains(&"UiCatalogViewSettingPayload"));
    assert!(public_type_names.contains(&"SetMyCatalogViewInput"));
    // Facet ratings and canonical vocabulary add three payload objects and
    // one rating-minimum input. Existing filter enums gain values, not types.
    assert!(query_field_names.contains(&"canonicalTagVocabulary"));
    for name in [
        "CanonicalTagVocabularyPayload",
        "CanonicalTagVocabularyEntryPayload",
        "ListRatingMinimumPayload",
        "ListRatingMinimumInput",
    ] {
        assert!(public_type_names.contains(&name), "missing type {name}");
    }
    // Scheduled scripts add the language, trigger, schedule kind and weekday
    // enums, the schedule input, and the stored-schedule and schedule-check
    // payloads: public types 934->941, OBJECT 509->511, INPUT_OBJECT 235->236,
    // ENUM 178->182.
    for name in [
        "ScriptLanguageValue",
        "ScriptTriggerValue",
        "ScriptScheduleKindValue",
        "ScheduleWeekdayValue",
        "ScriptScheduleInput",
        "ScriptSchedulePayload",
        "ScriptScheduleValidationPayload",
    ] {
        assert!(public_type_names.contains(&name), "missing type {name}");
    }
    // Rebased to the release tree: public types 941->954, OBJECT 511->516,
    // INPUT_OBJECT 236->239, ENUM 182->187. `CUSTOM_JOB` is a value on the
    // existing job-key enum and `customJobId` an additive field, so scheduled
    // scripts as jobs add no type.
    // Stringly-typed 0.21.15 fields became enums: public types 954->962,
    // ENUM 187->195. No object, input, or root field was added.
    for name in [
        "UserListAccountStatusValue",
        "ListAccountPollStatusValue",
        "PasskeyImpactValue",
        "ConfigValueSourceValue",
        "PasskeyRelyingPartySourceValue",
        "PublicUrlErrorCodeValue",
        "DownloadPasswordFailureCodeValue",
        "CanonicalTagCategoryValue",
    ] {
        assert!(public_type_names.contains(&name), "missing type {name}");
    }
    // Linking a list account with a key the member supplies adds the key
    // description on a provider and the link result: public types 962->964,
    // OBJECT 516->518.
    for name in ["ListMemberKeyPayload", "ListAccountKeyLinkPayload"] {
        assert!(public_type_names.contains(&name), "missing type {name}");
    }
    assert!(public_type_names.contains(&"MovieAvailabilityPayload"));
    assert_eq!(public_types.len(), 965);
    assert_eq!(kind_count("OBJECT"), 519);
    assert_eq!(kind_count("INPUT_OBJECT"), 239);
    assert_eq!(kind_count("ENUM"), 195);
