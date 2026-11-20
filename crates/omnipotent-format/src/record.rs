//! Canonical article record: fixed-layout nodes and borrowed UTF-8 data in one
//! buffer. `Record::nodes()` returns `&[Node]` directly; no Article is decoded.
use crate::{model::Node as OwnedNode, *};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec,
    vec::Vec,
};
use core::{cell::Cell as BudgetCell, fmt, mem::size_of, num::NonZeroU32};
use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned, byteorder::little_endian::U32,
};
#[path = "record/schema.rs"]
mod schema;

const MAGIC: [u8; 8] = *b"OMNIART\0";
const VERSION: u32 = 1;
const HEADER_SIZE: usize = 32;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    Truncated,
    Version,
    Size,
    Invalid,
    Utf8,
    Limit,
}
impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "article record: {self:?}")
    }
}
impl core::error::Error for RecordError {}
type Result<T> = core::result::Result<T, RecordError>;

#[repr(C)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
struct Ref {
    a: U32,
    b: U32,
}
impl Ref {
    const EMPTY: Self = Self {
        a: U32::ZERO,
        b: U32::ZERO,
    };
    fn new(a: usize, b: usize) -> Result<Self> {
        Ok(Self {
            a: U32::new(a.try_into().map_err(|_| RecordError::Size)?),
            b: U32::new(b.try_into().map_err(|_| RecordError::Size)?),
        })
    }
}
#[repr(C)]
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
struct Header {
    magic: [u8; 8],
    version: U32,
    length: U32,
    root: Ref,
    nodes: Ref,
}
const _: [(); HEADER_SIZE] = [(); size_of::<Header>()];

/// The stored node, not the heap-owning construction type in `model`.
/// Kind tags: section=0, paragraph=1, list=2, infobox=3, field=4,
/// table=5, image=6, gallery=7, unknown=8, abstract=9.
/// Nodes are in source/preorder. Parent is a 1-based node-array index; zero is root.
#[repr(C)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct Node {
    pub id: U32,
    pub parent: U32,
    pub kind: U32,
    text_offset: U32,
    text_length: U32,
    data_offset: U32,
    data_count: U32,
}
impl Node {
    /// Borrow text directly from the same record buffer.
    pub fn text<'a>(&self, record: &Record<'a>) -> Result<&'a str> {
        record.decoder().string(Ref {
            a: self.text_offset,
            b: self.text_length,
        })
    }
    fn data(&self) -> Ref {
        Ref {
            a: self.data_offset,
            b: self.data_count,
        }
    }
}

struct Encoder {
    bytes: Vec<u8>,
    strings: BTreeMap<String, Ref>,
    nodes: Vec<Node>,
    ids: BTreeSet<u32>,
    parent: u32,
}
impl Encoder {
    fn bytes(&mut self, bytes: &[u8]) -> Result<Ref> {
        let r = Ref::new(self.bytes.len(), bytes.len())?;
        self.bytes.extend_from_slice(bytes);
        if self.bytes.len() > package::MAX_ARTICLE_BYTES {
            return Err(RecordError::Size);
        }
        Ok(r)
    }
    fn records(&mut self, fields: &[Ref]) -> Result<Ref> {
        if fields.is_empty() {
            return Ok(Ref::EMPTY);
        }
        let r = Ref::new(self.bytes.len(), fields.len())?;
        for f in fields {
            self.bytes(f.as_bytes())?;
        }
        Ok(r)
    }
}
#[derive(Clone, Copy)]
struct Decoder<'a> {
    bytes: &'a [u8],
    nodes: &'a [Node],
    depth: usize,
    budget: Option<&'a BudgetCell<usize>>,
}
impl<'a> Decoder<'a> {
    fn range(&self, r: Ref, width: usize) -> Result<&'a [u8]> {
        let offset = r.a.get() as usize;
        let len = (r.b.get() as usize)
            .checked_mul(width)
            .ok_or(RecordError::Size)?;
        if len != 0 && offset < HEADER_SIZE {
            return Err(RecordError::Invalid);
        }
        self.bytes
            .get(offset..offset.checked_add(len).ok_or(RecordError::Size)?)
            .ok_or(RecordError::Truncated)
    }
    fn records(&self, r: Ref) -> Result<&'a [Ref]> {
        <[Ref]>::ref_from_bytes(self.range(r, size_of::<Ref>())?).map_err(|_| RecordError::Invalid)
    }
    fn fields(&self, r: Ref, count: usize) -> Result<&'a [Ref]> {
        let f = self.records(r)?;
        if f.len() != count {
            return Err(RecordError::Invalid);
        }
        Ok(f)
    }
    fn string(&self, r: Ref) -> Result<&'a str> {
        core::str::from_utf8(self.range(r, 1)?).map_err(|_| RecordError::Utf8)
    }
    fn charge(&self, size: usize) -> Result<()> {
        if let Some(b) = self.budget {
            b.set(b.get().checked_sub(size).ok_or(RecordError::Limit)?);
        }
        Ok(())
    }
    fn read<T: Wire>(&self, r: Ref) -> Result<T> {
        if self.depth >= 256 {
            return Err(RecordError::Limit);
        }
        T::decode(
            Self {
                depth: self.depth + 1,
                ..*self
            },
            r,
        )
    }
    fn node(&self, id: u32) -> Result<&'a Node> {
        self.nodes
            .iter()
            .find(|n| n.id.get() == id)
            .ok_or(RecordError::Invalid)
    }
}
// Private construction/owned-conversion helpers. The read API below does not use
// these to materialise a node tree: its nodes are already the stored node array.
trait Wire: Sized {
    fn encode(&self, e: &mut Encoder) -> Result<Ref>;
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self>;
}
impl Wire for u32 {
    fn encode(&self, _: &mut Encoder) -> Result<Ref> {
        Ok(Ref {
            a: U32::new(*self),
            b: U32::ZERO,
        })
    }
    fn decode(_: Decoder<'_>, r: Ref) -> Result<Self> {
        if r.b.get() != 0 {
            return Err(RecordError::Invalid);
        }
        Ok(r.a.get())
    }
}
impl Wire for u64 {
    fn encode(&self, _: &mut Encoder) -> Result<Ref> {
        Ok(Ref {
            a: U32::new(*self as u32),
            b: U32::new((self >> 32) as u32),
        })
    }
    fn decode(_: Decoder<'_>, r: Ref) -> Result<Self> {
        Ok(u64::from(r.a.get()) | (u64::from(r.b.get()) << 32))
    }
}
impl Wire for NonZeroU32 {
    fn encode(&self, e: &mut Encoder) -> Result<Ref> {
        self.get().encode(e)
    }
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self> {
        Self::new(d.read(r)?).ok_or(RecordError::Invalid)
    }
}
impl Wire for String {
    fn encode(&self, e: &mut Encoder) -> Result<Ref> {
        if let Some(r) = e.strings.get(self) {
            return Ok(*r);
        }
        let r = e.bytes(self.as_bytes())?;
        e.strings.insert(self.clone(), r);
        Ok(r)
    }
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self> {
        let s = d.string(r)?;
        d.charge(s.len())?;
        Ok(s.into())
    }
}
impl<T: Wire> Wire for Option<T> {
    fn encode(&self, e: &mut Encoder) -> Result<Ref> {
        match self {
            Some(v) => {
                let r = v.encode(e)?;
                e.records(&[r])
            }
            None => Ok(Ref::EMPTY),
        }
    }
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self> {
        match d.records(r)? {
            [] => Ok(None),
            [r] => Ok(Some(d.read(*r)?)),
            _ => Err(RecordError::Invalid),
        }
    }
}
impl<T: Wire> Wire for Vec<T> {
    fn encode(&self, e: &mut Encoder) -> Result<Ref> {
        let fields = self
            .iter()
            .map(|v| v.encode(e))
            .collect::<Result<Vec<_>>>()?;
        e.records(&fields)
    }
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self> {
        let fields = d.records(r)?;
        d.charge(
            fields
                .len()
                .checked_mul(size_of::<T>())
                .ok_or(RecordError::Size)?,
        )?;
        fields.iter().map(|r| d.read(*r)).collect()
    }
}
impl<A: Wire, B: Wire> Wire for (A, B) {
    fn encode(&self, e: &mut Encoder) -> Result<Ref> {
        let a = self.0.encode(e)?;
        let b = self.1.encode(e)?;
        e.records(&[a, b])
    }
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self> {
        let f = d.fields(r, 2)?;
        Ok((d.read(f[0])?, d.read(f[1])?))
    }
}
impl<K: Wire + Ord, V: Wire> Wire for BTreeMap<K, V> {
    fn encode(&self, e: &mut Encoder) -> Result<Ref> {
        let fields = self
            .iter()
            .map(|(k, v)| {
                let k = k.encode(e)?;
                let v = v.encode(e)?;
                e.records(&[k, v])
            })
            .collect::<Result<Vec<_>>>()?;
        e.records(&fields)
    }
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self> {
        let entries: Vec<(K, V)> = d.read(r)?;
        let mut map = Self::new();
        for (k, v) in entries {
            if map.insert(k, v).is_some() {
                return Err(RecordError::Invalid);
            }
        }
        Ok(map)
    }
}
impl Wire for OpaqueData {
    fn encode(&self, e: &mut Encoder) -> Result<Ref> {
        let media = self.media_type.encode(e)?;
        let bytes = e.bytes(&self.bytes)?;
        e.records(&[media, bytes])
    }
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self> {
        let f = d.fields(r, 2)?;
        let bytes = d.range(f[1], 1)?;
        d.charge(bytes.len())?;
        Ok(Self {
            media_type: d.read(f[0])?,
            bytes: bytes.into(),
        })
    }
}
impl Wire for OwnedNode {
    fn encode(&self, e: &mut Encoder) -> Result<Ref> {
        let id = self.id.0.get();
        if !e.ids.insert(id) {
            return Err(RecordError::Invalid);
        }
        let index = e.nodes.len();
        let parent = e.parent;
        e.nodes.push(Node {
            id: U32::new(id),
            parent: U32::new(parent),
            kind: U32::ZERO,
            text_offset: U32::ZERO,
            text_length: U32::ZERO,
            data_offset: U32::ZERO,
            data_count: U32::ZERO,
        });
        e.parent = u32::try_from(index + 1).map_err(|_| RecordError::Size)?;
        let data = self.content.encode(e)?;
        e.parent = parent;
        let (kind, text) = match &self.content {
            Part::Section { heading, .. } => (0, heading.as_ref()),
            Part::Paragraph(text) => (1, Some(text)),
            Part::List { .. } => (2, None),
            Part::Infobox { heading, .. } => (3, heading.as_ref()),
            Part::Field { label, .. } => (4, Some(label)),
            Part::Table(_) => (5, None),
            Part::Image(_) => (6, None),
            Part::Gallery(_) => (7, None),
            Part::Unknown { .. } => (8, None),
        };
        let text = match text {
            Some(t) => *e.strings.get(&t.value).ok_or(RecordError::Invalid)?,
            None => Ref::EMPTY,
        };
        e.nodes[index] = Node {
            id: U32::new(id),
            parent: U32::new(parent),
            kind: U32::new(kind),
            text_offset: text.a,
            text_length: text.b,
            data_offset: data.a,
            data_count: data.b,
        };
        id.encode(e)
    }
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self> {
        let id: NodeId = d.read(r)?;
        let node = d.node(id.0.get())?;
        Ok(Self {
            id,
            content: d.read(node.data())?,
        })
    }
}

/// Store one self-contained article. All node text points into the one string
/// arena; identifiers in construction-only arrays refer to the flat node array.
pub fn encode(article: &Article) -> Result<Vec<u8>> {
    let mut e = Encoder {
        bytes: vec![0; HEADER_SIZE],
        strings: BTreeMap::new(),
        nodes: Vec::new(),
        ids: BTreeSet::new(),
        parent: 0,
    };
    if article.abstract_text.is_some() {
        e.nodes.push(Node {
            id: U32::ZERO,
            parent: U32::ZERO,
            kind: U32::new(9),
            text_offset: U32::ZERO,
            text_length: U32::ZERO,
            data_offset: U32::ZERO,
            data_count: U32::ZERO,
        });
    }
    let root = article.encode(&mut e)?;
    if let Some(text) = &article.abstract_text {
        let r = *e.strings.get(&text.value).ok_or(RecordError::Invalid)?;
        e.nodes[0].text_offset = r.a;
        e.nodes[0].text_length = r.b;
    }
    let nodes = Ref::new(e.bytes.len(), e.nodes.len())?;
    let stored = core::mem::take(&mut e.nodes);
    for node in stored {
        e.bytes(node.as_bytes())?;
    }
    let header = Header {
        magic: MAGIC,
        version: U32::new(VERSION),
        length: U32::new(e.bytes.len().try_into().map_err(|_| RecordError::Size)?),
        root,
        nodes,
    };
    e.bytes[..HEADER_SIZE].copy_from_slice(header.as_bytes());
    Ok(e.bytes)
}

/// A borrowed stored article. Parsing borrows the header and node slice; it does
/// not allocate, decompress, or reconstruct an owned Article/Vec<Node>.
#[derive(Clone, Copy)]
pub struct Record<'a> {
    bytes: &'a [u8],
    root: Ref,
    nodes: &'a [Node],
}
impl<'a> Record<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > package::MAX_ARTICLE_BYTES {
            return Err(RecordError::Size);
        }
        let h = Header::ref_from_bytes(bytes.get(..HEADER_SIZE).ok_or(RecordError::Truncated)?)
            .map_err(|_| RecordError::Invalid)?;
        if h.magic != MAGIC {
            return Err(RecordError::Invalid);
        }
        if h.version.get() != VERSION {
            return Err(RecordError::Version);
        }
        if h.length.get() as usize != bytes.len() {
            return Err(RecordError::Size);
        }
        let d = Decoder {
            bytes,
            nodes: &[],
            depth: 0,
            budget: None,
        };
        d.fields(h.root, 11)?;
        let nodes = <[Node]>::ref_from_bytes(d.range(h.nodes, size_of::<Node>())?)
            .map_err(|_| RecordError::Invalid)?;
        Ok(Self {
            bytes,
            root: h.root,
            nodes,
        })
    }
    pub fn nodes(&self) -> &'a [Node] {
        self.nodes
    }
    fn decoder(&self) -> Decoder<'a> {
        Decoder {
            bytes: self.bytes,
            nodes: self.nodes,
            depth: 0,
            budget: None,
        }
    }
    fn fields(&self) -> Result<&'a [Ref]> {
        self.decoder().fields(self.root, 11)
    }
    pub fn title(&self) -> Result<&'a str> {
        self.decoder().string(self.fields()?[1])
    }
    pub fn resource_id(&self) -> Result<(&'a str, &'a str)> {
        let d = self.decoder();
        let f = d.fields(self.fields()?[0], 2)?;
        Ok((d.string(f[0])?, d.string(f[1])?))
    }
    pub fn node_text(&self, node: Option<NodeId>) -> Result<&'a str> {
        let id = node.map_or(0, |n| n.0.get());
        let n = self.decoder().node(id)?;
        if n.kind.get() != if node.is_some() { 1 } else { 9 } {
            return Err(RecordError::Invalid);
        }
        n.text(self)
    }
    pub fn excerpt(&self, node: Option<NodeId>, start: u32, end: u32) -> Result<&'a str> {
        if start >= end {
            return Err(RecordError::Invalid);
        }
        self.node_text(node)?
            .get(start as usize..end as usize)
            .ok_or(RecordError::Invalid)
    }
    pub fn heading(&self, node: Option<NodeId>) -> Result<String> {
        let Some(node) = node else {
            return Ok(String::new());
        };
        self.heading_in(self.fields()?[5], node, "", 0)?
            .ok_or(RecordError::Invalid)
    }
    fn heading_in(
        &self,
        nodes: Ref,
        wanted: NodeId,
        heading: &str,
        depth: usize,
    ) -> Result<Option<String>> {
        if depth >= 128 {
            return Err(RecordError::Limit);
        }
        let d = self.decoder();
        let join = |name: &str| {
            if heading.is_empty() {
                String::from(name)
            } else if name.is_empty() {
                String::from(heading)
            } else {
                alloc::format!("{heading} > {name}")
            }
        };
        let text = |r: Ref| -> Result<&str> { d.string(d.fields(r, 3)?[0]) };
        for reference in d.records(nodes)? {
            let id = d.read::<NodeId>(*reference)?;
            if id == wanted {
                return Ok(Some(heading.into()));
            }
            let node = d.node(id.0.get())?;
            let children = match node.kind.get() {
                0 | 3 => {
                    let f = d.fields(node.data(), 3)?;
                    let name = match d.records(f[1])? {
                        [] => "",
                        [r] => text(*r)?,
                        _ => return Err(RecordError::Invalid),
                    };
                    Some((f[2], join(name)))
                }
                4 => {
                    let f = d.fields(node.data(), 3)?;
                    Some((f[2], join(text(f[1])?)))
                }
                2 => {
                    let f = d.fields(node.data(), 3)?;
                    for item in d.records(f[2])? {
                        let f = d.fields(*item, 2)?;
                        let label = match d.records(f[0])? {
                            [] => "",
                            [r] => text(*r)?,
                            _ => return Err(RecordError::Invalid),
                        };
                        if let Some(found) =
                            self.heading_in(f[1], wanted, &join(label), depth + 1)?
                        {
                            return Ok(Some(found));
                        }
                    }
                    None
                }
                _ => None,
            };
            if let Some((children, path)) = children {
                if let Some(found) = self.heading_in(children, wanted, &path, depth + 1)? {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }
    /// Explicit convenience conversion for host-side editing/tests; never needed
    /// to iterate the stored nodes or borrow their text.
    pub fn to_owned(&self) -> Result<Article> {
        let budget = BudgetCell::new(package::MAX_ARTICLE_BYTES * 4);
        Decoder {
            bytes: self.bytes,
            nodes: self.nodes,
            depth: 0,
            budget: Some(&budget),
        }
        .read(self.root)
    }
}
