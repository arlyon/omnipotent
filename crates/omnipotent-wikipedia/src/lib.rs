//! Converts local Structured Wikipedia Parquet shards into shared articles.
//! No downloading, embedding, or package storage happens here.

mod source;

#[cfg(test)]
mod tests;

use std::{fs::File, num::NonZeroU32};

use anyhow::{Context, Result};
use omnipotent_format::*;
use parquet::arrow::{ProjectionMask, arrow_reader::ParquetRecordBatchReaderBuilder};
use percent_encoding::percent_decode_str;
use serde::de::DeserializeOwned;
use url::Url;

/// Reads a shard incrementally. Only consumed columns and a small batch of rows
/// are decoded; each returned article owns its contents independently of the file.
/// Errors are returned, never silently skipped. Language comes from each record.
pub fn articles(file: File) -> Result<impl Iterator<Item = Result<Article>>> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let columns = [
        "identifier",
        "name",
        "url",
        "description",
        "abstract",
        "date_modified",
        "in_language",
        "version",
        "license",
        "sections",
        "infoboxes",
        "tables",
        "references",
        "image",
    ];
    let projection = ProjectionMask::roots(
        builder.parquet_schema(),
        builder
            .schema()
            .fields()
            .iter()
            .enumerate()
            .filter(|(_, field)| columns.contains(&field.name().as_str()))
            .map(|(index, _)| index),
    );
    let batches = builder
        .with_projection(projection)
        .with_batch_size(64)
        .build()?;
    let mut batches = batches.enumerate();
    let mut rows = Vec::<source::Row>::new().into_iter();
    Ok(std::iter::from_fn(move || {
        loop {
            if let Some(row) = rows.next() {
                let id = row.identifier;
                return Some(convert(row).with_context(|| format!("Wikipedia page {id}")));
            }
            let (index, batch) = batches.next()?;
            let decoded = batch.map_err(anyhow::Error::from).and_then(|batch| {
                serde_arrow::from_record_batch::<Vec<source::Row>>(&batch).map_err(Into::into)
            });
            match decoded {
                Ok(batch) => rows = batch.into_iter(),
                Err(error) => return Some(Err(error.context(format!("Parquet batch {index}")))),
            }
        }
    }))
}

fn json_list<T: DeserializeOwned>(value: Option<String>) -> Result<Vec<T>> {
    value
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map(|v| v.unwrap_or_default())
        .map_err(Into::into)
}

fn convert(row: source::Row) -> Result<Article> {
    let source = SourceId::new(format!("wikipedia:{}", row.in_language.identifier))?;
    let mut converter = Converter {
        article: Article {
            id: ResourceId {
                source,
                key: ResourceKey::new(row.identifier.to_string())?,
            },
            title: row.name,
            description: row.description,
            abstract_text: row.r#abstract.map(Text::plain),
            provenance: Provenance {
                origin_uri: Some(row.url),
                language: Some(row.in_language.identifier),
                revision: row
                    .version
                    .map(|v| {
                        Ok::<_, anyhow::Error>(Revision {
                            id: RevisionId::new(v.identifier.to_string())?,
                            modified: row.date_modified,
                        })
                    })
                    .transpose()?,
                licenses: row
                    .license
                    .unwrap_or_default()
                    .into_iter()
                    .map(|l| License {
                        identifier: l.identifier,
                        name: l.name,
                        url: l.url,
                        attribution: None,
                    })
                    .collect(),
                extra: Default::default(),
            },
            parts: vec![],
            tables: vec![],
            references: vec![],
            assets: vec![],
            links: vec![],
            extra: Default::default(),
        },
        next_node: 1,
    };
    for reference in row.references.unwrap_or_default() {
        let text = converter.text(reference.text)?;
        converter.article.references.push(Reference {
            id: LocalId::new(reference.identifier)?,
            text,
            original: reference.metadata.map(|s| OpaqueData {
                media_type: "application/json".into(),
                bytes: s.into_bytes(),
            }),
        });
    }
    for table in json_list::<source::Table>(row.tables).context("tables")? {
        let headers = converter.cells(table.headers)?;
        let rows = converter.cells(table.rows)?;
        converter.article.tables.push(Table {
            id: LocalId::new(table.identifier)?,
            caption: table.caption.map(Text::plain),
            headers,
            rows,
        });
    }
    if let Some(image) = row.image {
        converter.image(image)?;
    }
    let mut parts = json_list::<source::Part>(row.infoboxes).context("infoboxes")?;
    parts.extend(json_list::<source::Part>(row.sections).context("sections")?);
    converter.article.parts = converter.parts(parts)?;
    Ok(converter.article)
}

struct Converter {
    article: Article,
    next_node: u32,
}

impl Converter {
    fn node(&mut self, content: Part) -> Result<Node> {
        let id = NodeId(NonZeroU32::new(self.next_node).context("too many article nodes")?);
        self.next_node = self
            .next_node
            .checked_add(1)
            .context("too many article nodes")?;
        Ok(Node { id, content })
    }

    fn text(&mut self, text: source::Text) -> Result<Text> {
        let mut associated = Vec::new();
        for link in text.links {
            let id = LocalId::new(format!("link-{}", self.article.links.len() + 1))?;
            let target = self.link_target(&link.url)?;
            self.article.links.push(Link {
                id: id.clone(),
                original_target: Some(link.url),
                label: link.text,
                target,
            });
            associated.push(Association::Link(id));
        }
        for citation in text.citations {
            associated.push(Association::Citation {
                reference: LocalId::new(citation.identifier)?,
                label: citation.text,
            });
        }
        Ok(Text {
            value: text.value,
            spans: vec![],
            associated,
        })
    }

    fn link_target(&self, value: &str) -> Result<LinkTarget> {
        if let Some(fragment) = value.strip_prefix('#') {
            return Ok(LinkTarget::Fragment(FragmentTarget::Unresolved {
                source: fragment.into(),
            }));
        }
        let Ok(url) = Url::parse(value) else {
            return Ok(LinkTarget::Unresolved {
                reason: "invalid source URL".into(),
            });
        };
        let host = format!(
            "{}.wikipedia.org",
            self.article.provenance.language.as_deref().unwrap_or("")
        );
        if url.host_str() != Some(host.as_str()) {
            return Ok(LinkTarget::External);
        }
        let Some(title) = url.path().strip_prefix("/wiki/") else {
            return Ok(LinkTarget::External);
        };
        let title = percent_decode_str(title).decode_utf8()?.replace('_', " ");
        let fragment = url.fragment().map(str::to_owned);
        if title == self.article.title {
            return Ok(LinkTarget::Fragment(match fragment {
                Some(source) => FragmentTarget::Unresolved { source },
                None => FragmentTarget::Absent,
            }));
        }
        Ok(LinkTarget::Article(ArticleTarget::NotInstalled {
            locator: ArticleLocator::Title {
                source: self.article.id.source.clone(),
                title,
            },
            source_fragment: fragment,
        }))
    }

    fn image(&mut self, image: source::Image) -> Result<LocalId> {
        let id = LocalId::new(
            image
                .identifier
                .unwrap_or_else(|| format!("image-{}", self.article.assets.len() + 1)),
        )?;
        if !self.article.assets.iter().any(|asset| asset.id == id) {
            self.article.assets.push(Asset {
                id: id.clone(),
                origin_uri: Some(image.content_url),
                media_type: image
                    .encoding_format
                    .unwrap_or_else(|| "application/octet-stream".into()),
                caption: image.caption.map(Text::plain),
                width: image.width,
                height: image.height,
                availability: AssetAvailability::NotInstalled,
            });
        }
        Ok(id)
    }

    fn cells(&mut self, rows: Vec<Vec<source::Cell>>) -> Result<Vec<Vec<Cell>>> {
        rows.into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| {
                        Ok(Cell {
                            text: self.text(cell.text)?,
                            row_span: cell.rowspan,
                            column_span: cell.colspan,
                        })
                    })
                    .collect()
            })
            .collect()
    }

    fn parts(&mut self, parts: Vec<source::Part>) -> Result<Vec<Node>> {
        let mut nodes = Vec::new();
        for part in parts {
            nodes.extend(self.part(part)?);
        }
        Ok(nodes)
    }

    fn part(&mut self, part: source::Part) -> Result<Vec<Node>> {
        use source::Kind;
        let text = self.text(part.text)?;
        let mut children = self.parts(part.has_parts)?;
        let images = part
            .images
            .into_iter()
            .map(|i| self.image(i))
            .collect::<Result<Vec<_>>>()?;
        let mut attachments = Vec::new();
        if matches!(part.kind, Kind::Gallery) {
            attachments.push(self.node(Part::Gallery(images))?);
        } else {
            for image in images {
                attachments.push(self.node(Part::Image(image))?);
            }
        }
        for table in part.table_references {
            attachments.push(self.node(Part::Table(LocalId::new(table.identifier)?))?);
        }
        let heading = part.name.map(Text::plain);
        let content = match part.kind {
            Kind::Section | Kind::Infobox | Kind::Field => {
                if !text.value.is_empty() || !text.associated.is_empty() {
                    children.insert(0, self.node(Part::Paragraph(text))?);
                }
                children.extend(attachments);
                match part.kind {
                    Kind::Infobox => Part::Infobox { heading, children },
                    Kind::Field => Part::Field {
                        label: heading.unwrap_or_default(),
                        children,
                    },
                    _ => Part::Section { heading, children },
                }
            }
            Kind::List | Kind::OrderedList | Kind::DefinitionList => {
                let mut items = Vec::new();
                for child in children {
                    match child.content {
                        Part::Section { heading, children } => items.push(ListItem {
                            label: heading,
                            children,
                        }),
                        other => items.push(ListItem {
                            label: None,
                            children: vec![Node {
                                id: child.id,
                                content: other,
                            }],
                        }),
                    }
                }
                if !text.value.is_empty() || !text.associated.is_empty() {
                    items.insert(
                        0,
                        ListItem {
                            label: None,
                            children: vec![self.node(Part::Paragraph(text))?],
                        },
                    );
                }
                if !attachments.is_empty() {
                    items.push(ListItem {
                        label: None,
                        children: attachments,
                    });
                }
                let list = self.node(Part::List {
                    style: match part.kind {
                        Kind::OrderedList => ListStyle::Ordered { start: 1 },
                        Kind::DefinitionList => ListStyle::Definition,
                        _ => ListStyle::Unordered,
                    },
                    items,
                })?;
                return if heading.is_some() {
                    Ok(vec![self.node(Part::Section {
                        heading,
                        children: vec![list],
                    })?])
                } else {
                    Ok(vec![list])
                };
            }
            Kind::ListItem | Kind::DefinitionTerm => {
                let label = if matches!(part.kind, Kind::DefinitionTerm) {
                    Some(text)
                } else {
                    children.insert(0, self.node(Part::Paragraph(text))?);
                    None
                };
                children.extend(attachments);
                Part::Section {
                    heading: label,
                    children,
                }
            }
            Kind::Paragraph | Kind::Definition | Kind::Image | Kind::Gallery | Kind::Table => {
                let mut nodes = Vec::new();
                if !text.value.is_empty() || !text.associated.is_empty() {
                    nodes.push(self.node(Part::Paragraph(text))?);
                }
                nodes.extend(attachments);
                nodes.extend(children);
                return Ok(nodes);
            }
        };
        Ok(vec![self.node(content)?])
    }
}
