//! Field order and numeric variant tags below are the version-1 disk schema.
//! Changes to either require a new binary version, not Rust layout assumptions.
use super::*;

macro_rules! record {
    ($name:ident { $($field:ident),+ $(,)? }) => {
        impl Wire for $name {
            fn encode(&self,e:&mut Encoder)->Result<Ref> {
                let fields=[$(self.$field.encode(e)?),+];e.records(&fields)
            }
            fn decode(d:Decoder<'_>,r:Ref)->Result<Self> {
                let count=[$(stringify!($field)),+].len();
                let mut fields=d.fields(r,count)?.iter();
                Ok(Self {$($field:d.read(*fields.next().ok_or(RecordError::Invalid)?)?),+})
            }
        }
    };
}
macro_rules! identifier {
    ($($name:ident),+) => {$(
        impl Wire for $name {
            fn encode(&self,e:&mut Encoder)->Result<Ref> {String::from(self.as_str()).encode(e)}
            fn decode(d:Decoder<'_>,r:Ref)->Result<Self> {Self::new(d.read::<String>(r)?).map_err(|_|RecordError::Invalid)}
        }
    )+};
}
macro_rules! enumeration {
    ($name:ident { $($tag:literal => $variant:ident $(($($value:ident),+))? $({$($field:ident),+})?),+ $(,)? }) => {
        impl Wire for $name {
            fn encode(&self,e:&mut Encoder)->Result<Ref> {
                match self {$(Self::$variant $(($($value),+))? $({$($field),+})? => {
                    let fields=[($tag as u32).encode(e)?, $($($value.encode(e)?,)+)? $($($field.encode(e)?,)+)?];
                    e.records(&fields)
                }),+}
            }
            fn decode(d:Decoder<'_>,r:Ref)->Result<Self> {
                let mut fields=d.records(r)?.iter();
                let tag:u32=d.read(*fields.next().ok_or(RecordError::Invalid)?)?;
                let value=match tag {$($tag => Self::$variant
                    $(($({let _=stringify!($value); d.read(*fields.next().ok_or(RecordError::Invalid)?)?}),+))?
                    $({$($field:d.read(*fields.next().ok_or(RecordError::Invalid)?)?),+})?,)+
                    _=>return Err(RecordError::Invalid),
                };
                if fields.next().is_some() {return Err(RecordError::Invalid);}
                Ok(value)
            }
        }
    };
}
identifier!(SourceId, LocalId, ResourceKey, RevisionId);
impl Wire for NodeId {
    fn encode(&self, e: &mut Encoder) -> Result<Ref> {
        self.0.encode(e)
    }
    fn decode(d: Decoder<'_>, r: Ref) -> Result<Self> {
        Ok(Self(d.read(r)?))
    }
}
record!(ResourceId { source, key });
record!(Revision { id, modified });
record!(License {
    identifier,
    name,
    url,
    attribution
});
record!(Provenance {
    origin_uri,
    language,
    revision,
    licenses,
    extra
});
record!(Article {
    id,
    title,
    description,
    abstract_text,
    provenance,
    parts,
    tables,
    references,
    assets,
    links,
    extra
});
record!(Text {
    value,
    spans,
    associated
});
record!(InlineSpan { start, end, target });
record!(ListItem { label, children });
record!(Table {
    id,
    caption,
    headers,
    rows
});
record!(Cell {
    text,
    row_span,
    column_span
});
record!(Reference { id, text, original });
record!(Asset {
    id,
    origin_uri,
    media_type,
    caption,
    width,
    height,
    availability
});
record!(Link {
    id,
    original_target,
    label,
    target
});

enumeration!(InlineTarget {
    0=>Link(id),1=>Citation(id),2=>Emphasis,3=>Strong,4=>Code,5=>Subscript,6=>Superscript,
});
enumeration!(Association {
    0=>Link(id),1=>Citation {reference,label},
});
enumeration!(Part {
    0=>Section {heading,children},1=>Paragraph(text),2=>List {style,items},
    3=>Infobox {heading,children},4=>Field {label,children},5=>Table(id),6=>Image(id),
    7=>Gallery(ids),8=>Unknown {source_type,original},
});
enumeration!(ListStyle {0=>Unordered,1=>Ordered {start},2=>Definition});
enumeration!(AssetAvailability {0=>NotInstalled,1=>Installed {object,length}});
enumeration!(LinkTarget {
    0=>Article(target),1=>Fragment(target),2=>Asset(id),3=>External,
    4=>Unresolved {reason},5=>Blocked {reason},
});
enumeration!(ArticleLocator {0=>Id(id),1=>Title {source,title}});
enumeration!(ArticleMatch {0=>Canonical,1=>Alias {title}});
enumeration!(ArticleTarget {
    0=>Installed {id,matched_by,fragment},
    1=>NotInstalled {locator,source_fragment},
    2=>Unresolved {locator,source_fragment,reason},
});
enumeration!(FragmentTarget {0=>Absent,1=>Resolved {source,node},2=>Unresolved {source}});
