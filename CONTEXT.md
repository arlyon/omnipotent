# Omnipotent Reference Library

A library of locally available reference publications, retaining their structure, provenance, and relationships independently of their presentation.

## Language

**Collection**:
A selected group of reference material prepared for offline use.

**Generation**:
An immutable installed release of a collection. Availability and collection-local identities belong to a particular generation.

**Source**:
A logical origin namespace for reference material, independent of download mirrors, transports, and collection membership.

**Resource identity**:
A source-qualified, stable identity for a reference item. Its source-assigned key survives title, location, and revision changes; the same key in different sources identifies different resources.

**Article**:
A structured reference document carrying a resource identity, independently of its title, presentation, or local storage location.

**Revision**:
A particular upstream version of a resource, distinct from its stable identity and the collection generation containing it. Article-local anchors and content positions describe that revision, not all future revisions.

**Document ID**:
A collection-generation-local identity used for retrieval and search membership. It is not a durable cross-article link target.

**Part**:
A structural unit of article content, such as a section, paragraph, list, infobox field, or table reference.

**Associated link**:
A link known to belong to a text block without a trustworthy position within that text. It must not be presented as a confirmed inline link.

**Inline span**:
A confirmed range within a text block carrying a link, citation, or semantic text mark.

**Installed target**:
A link destination confirmed to be available in the selected offline library.

**Missing target**:
A destination known not to be installed. It remains distinguishable from a destination whose identity or availability has not been resolved.

**Source fragment**:
An original destination anchor retained from a source link. It is not automatically a confirmed local anchor.

**Opaque part**:
Source content retained without interpretation when its structure is unsupported or unknown.
