use super::*;
use arrow_schema::FieldRef;
use parquet::arrow::ArrowWriter;
use serde_arrow::schema::{SchemaLike, TracingOptions};

fn row() -> source::Row {
    serde_json::from_str(r#"{
        "identifier": 42, "name": "Café", "url": "https://en.wikipedia.org/wiki/Caf%C3%A9",
        "in_language": {"identifier": "en"},
        "abstract": "A short summary.",
        "license": [{"identifier": "CC-BY-SA-4.0", "url": "https://creativecommons.org/licenses/by-sa/4.0/"}],
        "version": {"identifier": 123}, "date_modified": "2026-05-13T00:00:00Z",
        "image": {"content_url": "https://upload.wikimedia.org/main.jpg", "width": 200},
        "references": [{"identifier": "cite_note-1", "text": {"value": "A source"}, "metadata": "{}"}]
    }"#).unwrap()
}

#[test]
fn converts_structured_content_without_guessing_inline_offsets() -> Result<()> {
    let mut row = row();
    row.infoboxes = Some(
        r#"[{"type":"infobox","name":"Details","has_parts":[
        {"type":"field","name":"Type","value":"Building"}]}]"#
            .into(),
    );
    row.sections = Some(r##"[{"type":"section","name":"History","has_parts":[
        {"type":"paragraph","value":"Café in Paris.",
         "links":[{"url":"https://en.wikipedia.org/wiki/Paris#History","text":"Paris"},
                  {"url":"#History"},{"url":"https://example.org","text":"outside"}],
         "citations":[{"identifier":"cite_note-1","text":"[1]"}]},
        {"type":"list","has_parts":[{"type":"list_item","value":"First","has_parts":[
            {"type":"list","has_parts":[{"type":"list_item","value":"Nested"}]}]}]},
        {"type":"definition_list","has_parts":[{"type":"definition_term","value":"Term",
            "has_parts":[{"type":"definition","value":"Meaning"}]}]},
        {"type":"table","table_references":[{"identifier":"table1"}]},
        {"type":"gallery","images":[{"identifier":"picture","content_url":"https://upload.wikimedia.org/a.jpg",
            "encoding_format":"image/jpeg","caption":"A picture"}]}]}]"##.into());
    row.tables = Some(
        r#"[{"identifier":"table1","headers":[[{"value":"Heading"}]],
        "rows":[[{}, {"value":"Cell","rowspan":2,"colspan":3}]]}]"#
            .into(),
    );
    let article = convert(row)?;
    assert_eq!(article.id.source.as_str(), "wikipedia:en");
    assert_eq!(article.id.key.as_str(), "42");
    assert_eq!(article.provenance.revision.unwrap().id.as_str(), "123");
    assert_eq!(article.provenance.licenses.len(), 1);
    assert!(matches!(article.parts[0].content, Part::Infobox { .. }));
    let Part::Section { children, .. } = &article.parts[1].content else {
        panic!("section")
    };
    let Part::Paragraph(text) = &children[0].content else {
        panic!("paragraph")
    };
    assert_eq!(text.value, "Café in Paris.");
    assert!(text.spans.is_empty());
    assert_eq!(text.associated.len(), 4);
    assert!(matches!(
        article.links[0].target,
        LinkTarget::Article(ArticleTarget::NotInstalled { .. })
    ));
    assert!(matches!(article.links[1].target, LinkTarget::Fragment(_)));
    assert!(matches!(article.links[2].target, LinkTarget::External));
    let Part::List { items, .. } = &children[1].content else {
        panic!("list")
    };
    assert!(matches!(items[0].children[1].content, Part::List { .. }));
    let Part::List { style, items } = &children[2].content else {
        panic!("definition list")
    };
    assert_eq!(*style, ListStyle::Definition);
    assert_eq!(items[0].label.as_ref().unwrap().value, "Term");
    assert_eq!(article.tables[0].rows[0][1].row_span.get(), 2);
    assert_eq!(article.tables[0].rows[0][1].column_span.get(), 3);
    assert_eq!(article.tables[0].rows[0][0].text.value, "");
    assert_eq!(article.references[0].id.as_str(), "cite_note-1");
    assert!(matches!(children[4].content, Part::Gallery(_)));
    assert_eq!(article.assets.len(), 2);
    assert!(
        article
            .assets
            .iter()
            .all(|a| a.availability == AssetAvailability::NotInstalled)
    );
    Ok(())
}

#[test]
fn reads_across_parquet_batches_and_returns_conversion_errors() -> Result<()> {
    let fields = Vec::<FieldRef>::from_type::<source::Row>(TracingOptions::default())?;
    let mut rows = (0..66).map(|_| row()).collect::<Vec<_>>();
    rows[64].sections = Some(r#"[{"type":"unsupported"}]"#.into());
    let batch = serde_arrow::to_record_batch(&fields, &rows)?;
    let file = tempfile::tempfile()?;
    let mut writer = ArrowWriter::try_new(file.try_clone()?, batch.schema(), None)?;
    writer.write(&batch)?;
    writer.close()?;
    let mut articles = articles(file)?;
    for _ in 0..64 {
        assert_eq!(articles.next().unwrap()?.title, "Café");
    }
    let error = articles.next().unwrap().unwrap_err();
    assert!(format!("{error:#}").contains("unsupported"));
    assert!(articles.next().unwrap().is_ok());
    assert!(articles.next().is_none());
    Ok(())
}

#[test]
fn malformed_json_is_an_error_not_an_empty_article() {
    let mut row = row();
    row.sections = Some("not json".into());
    assert!(convert(row).is_err());
}
