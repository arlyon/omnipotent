use crate::{Chunker, MAX_INPUT_TOKENS, PreparedPassage};
use anyhow::{Context, Result, ensure};
use omnipotent_format::{
    Article, DocumentId, Node, NodeId, Part,
    package::{SearchEntry, SearchLocation},
};

pub fn chunk(
    chunker: &Chunker,
    article: &Article,
    document: DocumentId,
) -> Result<Vec<PreparedPassage>> {
    let mut blocks = Vec::new();
    if let Some(text) = &article.abstract_text {
        blocks.push((None, String::new(), text.value.as_str()));
    }
    walk(&article.parts, "", &mut blocks);
    let mut prepared = Vec::new();
    for (node, heading, text) in blocks {
        let context = if heading.is_empty() {
            String::new()
        } else {
            format!("{heading}\n\n")
        };
        let prefix = format!("title: {} | text: {context}", article.title);
        let mut start = 0;
        while start < text.len() {
            if text[start..].trim().is_empty() {
                break;
            }
            let input = format!("{prefix}{}", &text[start..]);
            let encoding = chunker
                .tokenizer
                .encode(input, true)
                .map_err(|e| anyhow::anyhow!("tokenizing: {e}"))?;
            let mut end = text.len();
            if encoding.len() > MAX_INPUT_TOKENS {
                let offset = encoding.get_offsets()[MAX_INPUT_TOKENS - 2].1;
                let available = offset.saturating_sub(prefix.len());
                ensure!(
                    available > 0,
                    "article title/heading leaves no room for passage text"
                );
                end = start + text[start..].floor_char_boundary(available.min(text.len() - start));
                if let Some((offset, _)) = text[start..end]
                    .char_indices()
                    .rev()
                    .find(|(offset, ch)| *offset > (end - start) / 2 && ch.is_whitespace())
                {
                    end = start + offset;
                }
            }
            let tokens = loop {
                ensure!(
                    end > start,
                    "cannot fit article passage within {MAX_INPUT_TOKENS} tokens"
                );
                let tokens = if end == text.len() && encoding.len() <= MAX_INPUT_TOKENS {
                    encoding.get_ids().to_vec()
                } else {
                    chunker.tokens(&format!("{prefix}{}", &text[start..end]))?
                };
                if tokens.len() <= MAX_INPUT_TOKENS {
                    break tokens;
                }
                end = text[..end]
                    .char_indices()
                    .next_back()
                    .context("empty passage")?
                    .0;
            };
            prepared.push(PreparedPassage {
                entry: SearchEntry {
                    resource: document,
                    location: SearchLocation::Article {
                        node,
                        start: start.try_into()?,
                        end: end.try_into()?,
                    },
                    heading: heading.clone(),
                    text: text[start..end].into(),
                },
                tokens,
            });
            start = end;
        }
    }
    Ok(prepared)
}

fn walk<'a>(nodes: &'a [Node], heading: &str, blocks: &mut Vec<(Option<NodeId>, String, &'a str)>) {
    let join = |name: &str| {
        if heading.is_empty() {
            name.to_owned()
        } else if name.is_empty() {
            heading.to_owned()
        } else {
            format!("{heading} > {name}")
        }
    };
    for node in nodes {
        match &node.content {
            Part::Section {
                heading: name,
                children,
            }
            | Part::Infobox {
                heading: name,
                children,
            } => walk(
                children,
                &join(name.as_ref().map(|t| t.value.as_str()).unwrap_or("")),
                blocks,
            ),
            Part::Field { label, children } => walk(children, &join(&label.value), blocks),
            Part::Paragraph(text) => blocks.push((Some(node.id), heading.into(), &text.value)),
            Part::List { items, .. } => {
                for item in items {
                    walk(
                        &item.children,
                        &join(item.label.as_ref().map(|t| t.value.as_str()).unwrap_or("")),
                        blocks,
                    );
                }
            }
            // Tables/media remain in the article; their text has no paragraph byte locator.
            _ => {}
        }
    }
}
