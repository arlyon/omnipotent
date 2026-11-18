use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub(crate) struct Row {
    pub identifier: u64,
    pub name: String,
    pub url: String,
    pub description: Option<String>,
    pub r#abstract: Option<String>,
    pub date_modified: Option<String>,
    pub in_language: Language,
    pub version: Option<Version>,
    pub license: Option<Vec<License>>,
    pub sections: Option<String>,
    pub infoboxes: Option<String>,
    pub tables: Option<String>,
    pub references: Option<Vec<Reference>>,
    pub image: Option<Image>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Language {
    pub identifier: String,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Version {
    pub identifier: u64,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct License {
    pub identifier: String,
    pub name: Option<String>,
    pub url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Section,
    Infobox,
    Field,
    Paragraph,
    List,
    OrderedList,
    ListItem,
    DefinitionList,
    DefinitionTerm,
    Definition,
    Table,
    Image,
    Gallery,
}

// Wikimedia uses the same recursive part shape throughout sections and infoboxes.
#[derive(Deserialize)]
pub(crate) struct Part {
    #[serde(rename = "type")]
    pub kind: Kind,
    pub name: Option<String>,
    #[serde(flatten)]
    pub text: Text,
    #[serde(default)]
    pub has_parts: Vec<Part>,
    #[serde(default)]
    pub images: Vec<Image>,
    #[serde(default)]
    pub table_references: Vec<Identifier>,
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Text {
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub citations: Vec<Citation>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Link {
    pub url: String,
    pub text: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Citation {
    pub identifier: String,
    pub text: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct Identifier {
    pub identifier: String,
}

#[derive(Deserialize)]
pub(crate) struct Table {
    pub identifier: String,
    pub caption: Option<String>,
    #[serde(default)]
    pub headers: Vec<Vec<Cell>>,
    #[serde(default)]
    pub rows: Vec<Vec<Cell>>,
}

#[derive(Deserialize)]
pub(crate) struct Cell {
    #[serde(flatten)]
    pub text: Text,
    #[serde(default = "one")]
    pub rowspan: std::num::NonZeroU32,
    #[serde(default = "one")]
    pub colspan: std::num::NonZeroU32,
}

fn one() -> std::num::NonZeroU32 {
    std::num::NonZeroU32::MIN
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Reference {
    pub identifier: String,
    pub text: Text,
    pub metadata: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Image {
    pub identifier: Option<String>,
    pub content_url: String,
    pub encoding_format: Option<String>,
    pub caption: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
