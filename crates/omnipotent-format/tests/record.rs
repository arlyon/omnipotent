use omnipotent_format::{DocumentId, NodeId, RecordError, package::PassageRef, record::Node};
use std::num::NonZeroU32;

#[test]
fn stored_nodes_and_passages_have_fixed_unaligned_layouts() {
    assert_eq!(std::mem::size_of::<Node>(), 28);
    assert_eq!(std::mem::align_of::<Node>(), 1);
    assert_eq!(std::mem::size_of::<PassageRef>(), 16);
    assert_eq!(std::mem::align_of::<PassageRef>(), 1);
}

#[test]
fn passage_is_only_little_endian_identity_and_range_and_borrows_its_bytes() {
    let p = PassageRef::new(
        DocumentId(0x11223344),
        Some(NodeId(NonZeroU32::new(7).unwrap())),
        1,
        9,
    )
    .unwrap();
    assert_eq!(
        p.bytes(),
        &[0x44, 0x33, 0x22, 0x11, 7, 0, 0, 0, 1, 0, 0, 0, 9, 0, 0, 0]
    );
    let mut bytes = vec![255];
    bytes.extend_from_slice(p.bytes());
    let p = PassageRef::parse(&bytes[1..]).unwrap();
    assert_eq!(p.bytes().as_ptr(), bytes[1..].as_ptr());
    assert_eq!(p.resource(), DocumentId(0x11223344));
    assert_eq!(p.node().unwrap().0.get(), 7);
    assert_eq!((p.start(), p.end()), (1, 9));
    assert!(PassageRef::parse(&bytes[2..]).is_err());
    assert_eq!(
        PassageRef::new(DocumentId(0), None, 1, 1).unwrap_err(),
        RecordError::Invalid
    );
}
