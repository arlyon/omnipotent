use omnipotent_format::{
    DocumentId,
    bundle::{Bundle, DIMENSIONS, Header, SUBVECTORS},
    package::PassageRef,
};
use zerocopy::{
    IntoBytes,
    little_endian::{F32, U32},
};

#[test]
fn packed_arrays_borrow_unaligned_bytes_and_pq_only_selects_candidates() {
    let mut q = [0.; DIMENSIONS];
    q[0] = 1.;
    let mut negative = q;
    negative[0] = -1.;
    // One cluster plus one empty cluster. PQ prefers row 0, while the exact
    // vectors prefer row 1 when both survive candidate selection.
    let mut bytes = vec![0xff];
    bytes.extend(Header::new(2, 0).as_bytes());
    bytes.extend([0, 2, 2].map(U32::new).as_bytes());
    bytes.extend([q.map(F32::new), negative.map(F32::new)].as_bytes());
    let codebook_start = bytes.len();
    bytes.resize(codebook_start + 262_144, 0);
    bytes[codebook_start..codebook_start + 4].copy_from_slice(F32::new(1.).as_bytes());
    bytes.extend([[0u8; SUBVECTORS], [1u8; SUBVECTORS]].as_bytes());
    let vector_start = bytes.len();
    bytes.extend([negative.map(F32::new), q.map(F32::new)].as_bytes());
    let p = PassageRef::new(DocumentId(0), None, 0, 1).unwrap();
    bytes.extend([p; 2].as_bytes());
    bytes.extend(U32::new(0).as_bytes());
    let bundle = Bundle::parse(&bytes[1..]).unwrap();
    assert_eq!(bundle.codes().len(), 2);
    assert_eq!(
        bundle.vectors().as_bytes().as_ptr(),
        bytes[vector_start..].as_ptr()
    );
    assert_eq!(bundle.search(&q, 1, 1, 1).unwrap()[0].row, 0);
    assert_eq!(bundle.search(&q, 1, 2, 1).unwrap()[0].row, 1);
    assert!(bundle.article(DocumentId(0)).is_err());
    assert!(Bundle::parse(&bytes[1..bytes.len() - 1]).is_err());
}
