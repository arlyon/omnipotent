use super::*;
use omnipotent_format::{
    Node, NodeId, Part, Provenance, ResourceId, ResourceKey, SourceId, Text,
    package::SearchLocation,
};
use std::num::NonZeroU32;

fn article() -> Article {
    Article {
        id: ResourceId {
            source: SourceId::new("fixture").unwrap(),
            key: ResourceKey::new("1").unwrap(),
        },
        title: "Example".into(),
        description: None,
        abstract_text: None,
        provenance: Provenance {
            origin_uri: None,
            language: None,
            revision: None,
            licenses: vec![],
            extra: Default::default(),
        },
        parts: vec![Node {
            id: NodeId(NonZeroU32::MIN),
            content: Part::Paragraph(Text::plain("café 世界 words ".repeat(500))),
        }],
        tables: vec![],
        references: vec![],
        assets: vec![],
        links: vec![],
        extra: Default::default(),
    }
}
#[test]
fn batch_budget_uses_padded_length() {
    assert_eq!(batch_len(std::iter::repeat_n(384, 20), 128).unwrap(), 5);
    assert_eq!(batch_len([10, 10, 384, 384, 384, 384], 128).unwrap(), 5);
    assert_eq!(batch_len([10, 20, 30], 2).unwrap(), 2);
    assert!(batch_len([385], 128).is_err());
    assert!(batch_len([10], 0).is_err());
}
#[test]
fn article_chunks_retain_exact_utf8_ranges() {
    let tokenizer = Tokenizer::from_bytes(br#"{"version":"1.0","truncation":null,"padding":null,"added_tokens":[],"normalizer":null,"pre_tokenizer":{"type":"Whitespace"},"post_processor":null,"decoder":null,"model":{"type":"WordLevel","vocab":{"[UNK]":0},"unk_token":"[UNK]"}}"#).unwrap();
    let chunker = Chunker { tokenizer };
    let article = article();
    let prepared = chunker
        .chunk(ResourceKind::Article, &article, DocumentId(7))
        .unwrap();
    assert!(prepared.len() > 1);
    let Part::Paragraph(text) = &article.parts[0].content else {
        panic!()
    };
    let mut end = 0;
    for passage in prepared {
        assert!(passage.tokens.len() <= MAX_INPUT_TOKENS);
        let SearchLocation::Article {
            node,
            start,
            end: next,
        } = passage.entry.location
        else {
            panic!()
        };
        assert_eq!(node, Some(article.parts[0].id));
        assert_eq!(start as usize, end);
        assert_eq!(
            passage.entry.text,
            text.value[start as usize..next as usize]
        );
        end = next as usize;
    }
    assert_eq!(end, text.value.len());
    assert!(
        chunker
            .chunk(ResourceKind::Publication, &article, DocumentId(0))
            .is_err()
    );
}
