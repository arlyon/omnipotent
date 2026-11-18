use std::{collections::BTreeMap, num::NonZeroU32};

use omnipotent_format::*;

fn local(value: &str) -> LocalId {
    LocalId::new(value).unwrap()
}
fn node(value: u32, content: Part) -> Node {
    Node {
        id: NodeId(NonZeroU32::new(value).unwrap()),
        content,
    }
}
fn article_id(page: u64) -> ResourceId {
    ResourceId {
        source: SourceId::new("wikipedia:en").unwrap(),
        key: ResourceKey::new(page.to_string()).unwrap(),
    }
}
fn opaque() -> OpaqueData {
    OpaqueData {
        media_type: "application/json".into(),
        bytes: br#"{"future":{"value":42}}"#.to_vec(),
    }
}
fn fixture() -> Article {
    let text = Text {
        value: "Café water".into(),
        spans: vec![
            InlineSpan {
                start: 0,
                end: 5,
                target: InlineTarget::Emphasis,
            },
            InlineSpan {
                start: 6,
                end: 11,
                target: InlineTarget::Link(local("water")),
            },
        ],
        associated: vec![Association::Citation {
            reference: local("missing-citation"),
            label: Some("[7]".into()),
        }],
    };
    Article {
        id: article_id(7),
        title: "Synthetic reference fixture".into(),
        description: Some("Test data, not an imported publication".into()),
        abstract_text: Some(Text::plain("Summary")),
        provenance: Provenance {
            origin_uri: Some("https://example.invalid/wiki/Fixture".into()),
            language: Some("en".into()),
            revision: Some(Revision {
                id: RevisionId::new("11").unwrap(),
                modified: Some("2026-01-01T00:00:00Z".into()),
            }),
            licenses: vec![],
            extra: BTreeMap::from([(local("unmapped_source_metadata"), opaque())]),
        },
        parts: vec![node(
            1,
            Part::Section {
                heading: Some(Text::plain("Structure")),
                children: vec![
                    node(2, Part::Paragraph(text)),
                    node(
                        3,
                        Part::List {
                            style: ListStyle::Definition,
                            items: vec![ListItem {
                                label: Some(Text::plain("Term")),
                                children: vec![node(4, Part::Paragraph(Text::plain("Meaning")))],
                            }],
                        },
                    ),
                    node(
                        5,
                        Part::Infobox {
                            heading: Some(Text::plain("Facts")),
                            children: vec![node(
                                6,
                                Part::Field {
                                    label: Text::plain("Name"),
                                    children: vec![node(7, Part::Paragraph(Text::plain("Value")))],
                                },
                            )],
                        },
                    ),
                    node(8, Part::Table(local("table-1"))),
                    node(9, Part::Image(local("image-1"))),
                    node(10, Part::Gallery(vec![local("image-1")])),
                    node(
                        11,
                        Part::Unknown {
                            source_type: "future-widget".into(),
                            original: opaque(),
                        },
                    ),
                ],
            },
        )],
        tables: vec![Table {
            id: local("table-1"),
            caption: Some(Text::plain("Caption")),
            headers: vec![],
            rows: vec![vec![Cell {
                text: Text::plain("Cell"),
                row_span: NonZeroU32::new(2).unwrap(),
                column_span: NonZeroU32::new(1).unwrap(),
            }]],
        }],
        references: vec![Reference {
            id: local("ref-1"),
            text: Text::plain("Reference text"),
            original: Some(opaque()),
        }],
        assets: vec![Asset {
            id: local("image-1"),
            origin_uri: Some("https://example.invalid/image.jpg".into()),
            media_type: "image/jpeg".into(),
            caption: Some(Text::plain("Image caption")),
            width: Some(250),
            height: Some(100),
            availability: AssetAvailability::NotInstalled,
        }],
        links: vec![Link {
            id: local("water"),
            original_target: Some("https://en.wikipedia.org/wiki/Water#History".into()),
            label: Some("water".into()),
            target: LinkTarget::Article(ArticleTarget::NotInstalled {
                locator: ArticleLocator::Title {
                    source: SourceId::new("wikipedia:en").unwrap(),
                    title: "Water".into(),
                },
                source_fragment: Some("History".into()),
            }),
        }],
        extra: BTreeMap::from([(local("unmapped_source_field"), opaque())]),
    }
}

fn paragraph(article: &mut Article) -> &mut Text {
    let Part::Section { children, .. } = &mut article.parts[0].content else {
        panic!("fixture section")
    };
    let Part::Paragraph(text) = &mut children[0].content else {
        panic!("fixture paragraph")
    };
    text
}

#[test]
fn complete_structure_roundtrips_without_losing_unknown_data() {
    let original = fixture();
    original.validate(ModelLimits::default()).unwrap();
    let encoded = serde_json::to_vec(&original).unwrap();
    let decoded: Article = serde_json::from_slice(&encoded).unwrap();
    decoded.validate(ModelLimits::default()).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(
        decoded.extra[&local("unmapped_source_field")].bytes,
        opaque().bytes
    );
}

#[test]
fn uncertain_link_placement_stays_associated_not_invented() {
    let mut article = fixture();
    let text = paragraph(&mut article);
    text.value = "water water".into();
    text.spans.clear();
    text.associated.push(Association::Link(local("water")));
    article.validate(ModelLimits::default()).unwrap();
    let mut decoded: Article =
        serde_json::from_str(&serde_json::to_string(&article).unwrap()).unwrap();
    let text = paragraph(&mut decoded);
    assert!(text.spans.is_empty());
    assert_eq!(
        text.associated[0],
        Association::Citation {
            reference: local("missing-citation"),
            label: Some("[7]".into()),
        }
    );
    assert_eq!(
        decoded.links[0].original_target.as_deref(),
        Some("https://en.wikipedia.org/wiki/Water#History")
    );
}

#[test]
fn all_link_states_are_distinct_and_roundtrip() {
    let states = vec![
        LinkTarget::Article(ArticleTarget::Installed {
            id: article_id(99),
            matched_by: ArticleMatch::Canonical,
            fragment: FragmentTarget::Absent,
        }),
        LinkTarget::Article(ArticleTarget::Installed {
            id: article_id(99),
            matched_by: ArticleMatch::Alias {
                title: "H2O".into(),
            },
            fragment: FragmentTarget::Unresolved {
                source: "History".into(),
            },
        }),
        LinkTarget::Article(ArticleTarget::NotInstalled {
            locator: ArticleLocator::Id(article_id(99)),
            source_fragment: None,
        }),
        LinkTarget::Article(ArticleTarget::Unresolved {
            locator: ArticleLocator::Title {
                source: SourceId::new("wikipedia:en").unwrap(),
                title: "Alias".into(),
            },
            source_fragment: None,
            reason: "redirect metadata unavailable".into(),
        }),
        LinkTarget::Fragment(FragmentTarget::Resolved {
            source: "Structure".into(),
            node: fixture().parts[0].id,
        }),
        LinkTarget::Fragment(FragmentTarget::Unresolved {
            source: "Original_anchor".into(),
        }),
        LinkTarget::Asset(local("image-1")),
        LinkTarget::External,
        LinkTarget::Unresolved {
            reason: "unsupported target".into(),
        },
        LinkTarget::Blocked {
            reason: "unsupported scheme".into(),
        },
    ];
    for target in states {
        let mut article = fixture();
        article.links[0].target = target.clone();
        article.validate(ModelLimits::default()).unwrap();
        let json = serde_json::to_vec(&article).unwrap();
        let decoded: Article = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.links[0].target, target);
    }
}

#[test]
fn source_identity_does_not_depend_on_local_document_id() {
    let article = fixture();
    assert_eq!(article.id.source.as_str(), "wikipedia:en");
    assert_eq!(article.id.key.as_str(), "7");
    assert_ne!(DocumentId(0), DocumentId(1));
    assert_eq!(
        serde_json::from_str::<ResourceId>(&serde_json::to_string(&article.id).unwrap()).unwrap(),
        article.id
    );
}

#[test]
fn missing_tables_references_and_assets_are_explicit_and_allowed() {
    let mut article = fixture();
    article.tables.clear();
    article.assets.clear();
    article.validate(ModelLimits::default()).unwrap();
    let missing = local("missing-citation");
    assert!(matches!(
        article.reference(&missing),
        Resolution::Missing(_)
    ));
    let table = local("table-1");
    assert!(matches!(article.table(&table), Resolution::Missing(_)));
    let asset = local("image-1");
    assert!(matches!(article.asset(&asset), Resolution::Missing(_)));
    let reference = local("ref-1");
    assert!(matches!(
        article.reference(&reference),
        Resolution::Found(_)
    ));
}

#[test]
fn invalid_utf8_empty_reversed_and_out_of_bounds_spans_fail() {
    for (start, end) in [(4, 5), (3, 3), (8, 3), (0, 100), (u32::MAX, u32::MAX)] {
        let mut article = fixture();
        paragraph(&mut article).spans = vec![InlineSpan {
            start,
            end,
            target: InlineTarget::Strong,
        }];
        assert_eq!(
            article.validate(ModelLimits::default()),
            Err(ModelError::InvalidSpan { start, end })
        );
    }
}

#[test]
fn unresolved_anchor_is_allowed_but_a_false_resolved_anchor_fails() {
    let mut article = fixture();
    let nonexistent = NodeId(NonZeroU32::new(999).unwrap());
    article.links[0].target = LinkTarget::Fragment(FragmentTarget::Resolved {
        source: "missing".into(),
        node: nonexistent,
    });
    assert_eq!(
        article.validate(ModelLimits::default()),
        Err(ModelError::MissingAnchor(nonexistent))
    );
    article.links[0].target = LinkTarget::Fragment(FragmentTarget::Unresolved {
        source: "missing".into(),
    });
    article.validate(ModelLimits::default()).unwrap();
}

#[test]
fn duplicate_nodes_and_relationship_ids_are_rejected() {
    let mut article = fixture();
    article.parts.push(article.parts[0].clone());
    assert!(matches!(
        article.validate(ModelLimits::default()),
        Err(ModelError::DuplicateNode(_))
    ));
    let mut article = fixture();
    article.links.push(article.links[0].clone());
    assert!(matches!(
        article.validate(ModelLimits::default()),
        Err(ModelError::DuplicateId { kind: "link", .. })
    ));
    let mut article = fixture();
    article.tables.push(article.tables[0].clone());
    assert!(matches!(
        article.validate(ModelLimits::default()),
        Err(ModelError::DuplicateId { kind: "table", .. })
    ));
}

#[test]
fn dangling_internal_link_ids_are_invalid_not_missing_source_references() {
    let mut article = fixture();
    article.links.clear();
    assert_eq!(
        article.validate(ModelLimits::default()),
        Err(ModelError::MissingLink(local("water")))
    );
}

#[test]
fn structural_budgets_are_enforced() {
    let original = fixture();
    for (limits, expected) in [
        (
            ModelLimits {
                max_depth: 1,
                ..ModelLimits::default()
            },
            ModelError::DepthLimit,
        ),
        (
            ModelLimits {
                max_nodes: 1,
                ..ModelLimits::default()
            },
            ModelError::NodeLimit,
        ),
        (
            ModelLimits {
                max_items: 0,
                ..ModelLimits::default()
            },
            ModelError::ItemLimit,
        ),
        (
            ModelLimits {
                max_text_bytes: 0,
                ..ModelLimits::default()
            },
            ModelError::TextLimit,
        ),
        (
            ModelLimits {
                max_spans: 0,
                ..ModelLimits::default()
            },
            ModelError::SpanLimit,
        ),
    ] {
        assert_eq!(original.validate(limits), Err(expected));
    }
}

#[test]
fn empty_rows_and_gallery_members_count_toward_item_limits() {
    let mut article = fixture();
    article.tables[0].rows = vec![vec![]; 50];
    let limits = ModelLimits {
        max_items: 32,
        ..ModelLimits::default()
    };
    assert_eq!(article.validate(limits), Err(ModelError::ItemLimit));
    let mut article = fixture();
    article
        .parts
        .push(node(100, Part::Gallery(vec![local("image-1"); 50])));
    assert_eq!(article.validate(limits), Err(ModelError::ItemLimit));
}

#[test]
fn claimed_self_article_anchors_must_exist_in_this_revision() {
    let mut article = fixture();
    let missing = NodeId(NonZeroU32::new(500).unwrap());
    article.links[0].target = LinkTarget::Article(ArticleTarget::Installed {
        id: article.id.clone(),
        matched_by: ArticleMatch::Canonical,
        fragment: FragmentTarget::Resolved {
            source: "lost".into(),
            node: missing,
        },
    });
    assert_eq!(
        article.validate(ModelLimits::default()),
        Err(ModelError::MissingAnchor(missing))
    );
}

#[test]
fn present_provenance_and_explicit_failure_reasons_are_checked() {
    let mut article = fixture();
    article.provenance.origin_uri = Some(String::new());
    assert_eq!(
        article.validate(ModelLimits::default()),
        Err(ModelError::EmptyField("origin_uri"))
    );
    let mut article = fixture();
    article.provenance.language = Some(String::new());
    assert_eq!(
        article.validate(ModelLimits::default()),
        Err(ModelError::EmptyField("language"))
    );
    let mut article = fixture();
    article.links[0].target = LinkTarget::Blocked {
        reason: String::new(),
    };
    assert_eq!(
        article.validate(ModelLimits::default()),
        Err(ModelError::EmptyField("link.reason"))
    );
}

#[test]
fn malformed_identifiers_zero_ids_and_unknown_schema_fields_fail() {
    assert!(SourceId::new("../enwiki").is_err());
    assert!(SourceId::new("").is_err());
    assert!(LocalId::new("").is_err());
    assert!(ResourceKey::new("").is_err());
    assert!(RevisionId::new("").is_err());
    assert!(serde_json::from_str::<SourceId>("\"/enwiki\"").is_err());
    assert!(serde_json::from_str::<LocalId>("\"\"").is_err());
    assert!(serde_json::from_str::<ResourceKey>("\"\"").is_err());
    assert!(serde_json::from_str::<RevisionId>("0").is_err());
    assert!(serde_json::from_str::<NodeId>("0").is_err());
    let mut json = serde_json::to_value(fixture()).unwrap();
    json.as_object_mut()
        .unwrap()
        .insert("unversioned_field".into(), serde_json::json!(true));
    assert!(serde_json::from_value::<Article>(json).is_err());
}

#[test]
fn source_neutral_keys_and_upstream_revisions_roundtrip_exactly() {
    for (source, key, revision) in [
        ("wikipedia:en", "7", "11"),
        ("publisher:manuals", "water-treatment", "edition-3"),
        (
            "owner:documents",
            "chapter/入門?ref=β#intro",
            "W/\"etag-42\"",
        ),
        ("owner:documents", "0", "0"),
    ] {
        let mut article = fixture();
        article.id = ResourceId {
            source: SourceId::new(source).unwrap(),
            key: ResourceKey::new(key).unwrap(),
        };
        article.provenance.revision = Some(Revision {
            id: RevisionId::new(revision).unwrap(),
            modified: None,
        });
        article.validate(ModelLimits::default()).unwrap();
        let decoded: Article =
            serde_json::from_slice(&serde_json::to_vec(&article).unwrap()).unwrap();
        assert_eq!(decoded, article);
        assert_eq!(decoded.id.key.as_str(), key);
        assert_eq!(decoded.provenance.revision.unwrap().id.as_str(), revision);
    }
}

#[test]
fn resource_identity_is_source_qualified_and_has_an_explicit_wire_shape() {
    let original = article_id(7);
    assert_eq!(
        serde_json::to_string(&original).unwrap(),
        r#"{"source":"wikipedia:en","key":"7"}"#
    );
    let mut other_source = original.clone();
    other_source.source = SourceId::new("publisher:manuals").unwrap();
    assert_ne!(original, other_source);

    // Keys remain exact: no case folding, percent decoding, or Unicode normalization.
    for (left, right) in [("Water", "water"), ("a/b", "a%2Fb"), ("é", "e\u{301}")] {
        assert_ne!(
            ResourceKey::new(left).unwrap(),
            ResourceKey::new(right).unwrap()
        );
    }
    assert_ne!(
        SourceId::new("publisher:manuals").unwrap(),
        SourceId::new("Publisher:manuals").unwrap()
    );
    assert!(serde_json::from_str::<ResourceId>(r#"{"project":"enwiki","page":7}"#).is_err());
    assert!(serde_json::from_str::<RevisionId>("11").is_err());
    assert!(serde_json::from_str::<RevisionId>(r#""""#).is_err());
}

#[test]
fn identity_survives_title_mirror_and_revision_changes() {
    let original = fixture();
    let mut updated = original.clone();
    updated.title = "Renamed document".into();
    updated.provenance.origin_uri = Some("https://mirror.invalid/renamed".into());
    updated.provenance.revision = Some(Revision {
        id: RevisionId::new("next-edition").unwrap(),
        modified: None,
    });
    updated.validate(ModelLimits::default()).unwrap();
    assert_eq!(updated.id, original.id);
    assert_ne!(updated.provenance.revision, original.provenance.revision);
}

#[test]
fn cross_source_link_states_roundtrip_without_a_url() {
    let mut origin = fixture();
    let mut destination = fixture();
    destination.id.source = SourceId::new("publisher:manuals").unwrap();
    // Same key and title deliberately do not imply the same resource.
    assert_eq!(origin.id.key, destination.id.key);
    assert_eq!(origin.title, destination.title);
    let target_id = destination.id.clone();
    origin.links[0].original_target = None;
    let targets = [
        ArticleTarget::Installed {
            id: target_id.clone(),
            matched_by: ArticleMatch::Canonical,
            fragment: FragmentTarget::Absent,
        },
        ArticleTarget::NotInstalled {
            locator: ArticleLocator::Id(target_id.clone()),
            source_fragment: Some("chapter-1".into()),
        },
        ArticleTarget::Unresolved {
            locator: ArticleLocator::Title {
                source: target_id.source.clone(),
                title: destination.title.clone(),
            },
            source_fragment: None,
            reason: "source catalogue unavailable".into(),
        },
    ];
    for target in targets {
        origin.links[0].target = LinkTarget::Article(target.clone());
        origin.validate(ModelLimits::default()).unwrap();
        let decoded: Article =
            serde_json::from_slice(&serde_json::to_vec(&origin).unwrap()).unwrap();
        assert_eq!(decoded.links[0].target, LinkTarget::Article(target));
        assert_eq!(decoded.links[0].original_target, None);
    }
    let library = BTreeMap::from([
        (origin.id.clone(), origin),
        (destination.id.clone(), destination),
    ]);
    assert_eq!(library.len(), 2);
    assert_eq!(library[&target_id].id, target_id);
}

#[test]
fn same_key_in_another_source_does_not_claim_a_local_anchor() {
    let mut article = fixture();
    let mut target_id = article.id.clone();
    target_id.source = SourceId::new("publisher:manuals").unwrap();
    article.links[0].target = LinkTarget::Article(ArticleTarget::Installed {
        id: target_id,
        matched_by: ArticleMatch::Canonical,
        fragment: FragmentTarget::Resolved {
            source: "chapter-2".into(),
            node: NodeId(NonZeroU32::new(999).unwrap()),
        },
    });
    // Cross-resource node existence is the resolver's responsibility.
    article.validate(ModelLimits::default()).unwrap();
}

#[test]
fn unknown_origins_language_and_revision_need_no_fabricated_values() {
    let mut article = fixture();
    article.provenance.origin_uri = None;
    article.provenance.language = None;
    article.provenance.revision = None;
    article.assets[0].origin_uri = None;
    article.links[0].original_target = None;
    article.validate(ModelLimits::default()).unwrap();
    let mut json = serde_json::to_value(&article).unwrap();
    for key in ["origin_uri", "language", "revision"] {
        json["provenance"].as_object_mut().unwrap().remove(key);
    }
    json["assets"][0]
        .as_object_mut()
        .unwrap()
        .remove("origin_uri");
    json["links"][0]
        .as_object_mut()
        .unwrap()
        .remove("original_target");
    let decoded: Article = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, article);
    decoded.validate(ModelLimits::default()).unwrap();
}

#[test]
fn external_links_require_destinations_and_present_origins_must_be_nonempty() {
    let mut article = fixture();
    article.links[0].target = LinkTarget::External;
    article.links[0].original_target = None;
    assert_eq!(
        article.validate(ModelLimits::default()),
        Err(ModelError::EmptyField("link.original_target"))
    );
    article.links[0].original_target = Some("https://example.invalid/resource".into());
    article.validate(ModelLimits::default()).unwrap();
    article.links[0].original_target = Some(String::new());
    assert_eq!(
        article.validate(ModelLimits::default()),
        Err(ModelError::EmptyField("link.original_target"))
    );
    let mut article = fixture();
    article.assets[0].origin_uri = Some(String::new());
    assert_eq!(
        article.validate(ModelLimits::default()),
        Err(ModelError::EmptyField("asset.origin_uri"))
    );
}
