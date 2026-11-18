use omnipotent_format::{
    DocumentId, RECORD_HEADER_LEN, Record, RecordError, RecordHeader, RecordLimits,
};

const PAYLOAD: &[u8] = include_bytes!("fixtures/payload.json.zst");
const DECODED: &[u8] = include_bytes!("fixtures/payload.json");

fn framed(document: u32) -> Vec<u8> {
    let header = RecordHeader::new(
        DocumentId(document),
        PAYLOAD.len() as u64,
        DECODED.len() as u64,
    )
    .unwrap();
    let mut bytes = header.bytes().to_vec();
    bytes.extend_from_slice(PAYLOAD);
    bytes
}

#[test]
fn wire_layout_is_explicit_little_endian_and_padding_free() {
    let header = RecordHeader::new(
        DocumentId(0x11223344),
        0x0102030405060708,
        0x1112131415161718,
    )
    .unwrap();
    assert_eq!(
        header.bytes(),
        &[
            0x4f, 0x4d, 0x4e, 0x49, 0x52, 0x45, 0x46, 0x00, 0x02, 0x00, 0x28, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x44, 0x33, 0x22, 0x11, 0x01, 0x00, 0x01, 0x00, 0x08, 0x07, 0x06, 0x05,
            0x04, 0x03, 0x02, 0x01, 0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11,
        ]
    );
    assert_eq!(std::mem::size_of::<RecordHeader>(), RECORD_HEADER_LEN);
    assert_eq!(std::mem::align_of::<RecordHeader>(), 1);
}

#[test]
fn parsing_borrows_the_original_header_and_compressed_payload() {
    let bytes = framed(42);
    let record = Record::parse(&bytes, RecordLimits::default()).unwrap();
    assert_eq!(record.header().document(), DocumentId(42));
    assert_eq!(record.header().decoded_len(), DECODED.len() as u64);
    assert_eq!(record.payload(), PAYLOAD);
    assert_eq!(record.header().bytes().as_ptr(), bytes.as_ptr());
    assert_eq!(
        record.payload().as_ptr(),
        bytes[RECORD_HEADER_LEN..].as_ptr()
    );
}

#[test]
fn non_native_alignment_is_supported_without_copying() {
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&framed(0));
    let record = Record::parse(&bytes[1..], RecordLimits::default()).unwrap();
    assert_eq!(record.header().document(), DocumentId(0));
    assert_eq!(record.header().bytes().as_ptr(), bytes[1..].as_ptr());
}

#[test]
fn prefix_parser_scans_concatenated_records() {
    let mut bytes = framed(1);
    bytes.extend_from_slice(&framed(2));
    let (first, remaining) = Record::parse_prefix(&bytes, RecordLimits::default()).unwrap();
    let (second, remaining) = Record::parse_prefix(remaining, RecordLimits::default()).unwrap();
    assert_eq!(first.header().document(), DocumentId(1));
    assert_eq!(second.header().document(), DocumentId(2));
    assert!(remaining.is_empty());
    assert_eq!(
        Record::parse(&bytes, RecordLimits::default()).unwrap_err(),
        RecordError::TrailingBytes
    );
}

#[test]
fn every_truncated_prefix_fails_without_a_panic() {
    let bytes = framed(0);
    for end in 0..bytes.len() {
        assert!(
            Record::parse(&bytes[..end], RecordLimits::default()).is_err(),
            "prefix length {end}"
        );
    }
    assert_eq!(
        RecordHeader::parse(&bytes[..39], RecordLimits::default()).unwrap_err(),
        RecordError::HeaderSize
    );
    assert_eq!(
        RecordHeader::parse(&bytes[..41], RecordLimits::default()).unwrap_err(),
        RecordError::HeaderSize
    );
}

#[test]
fn unknown_versions_flags_payload_types_and_codecs_are_rejected() {
    for (offset, replacement, expected) in [
        (0, 0xff, RecordError::Magic),
        (8, 0, RecordError::Version(0)),
        (8, 1, RecordError::Version(1)),
        (8, 3, RecordError::Version(3)),
        (10, 41, RecordError::HeaderLength(41)),
        (12, 1, RecordError::Flags(1)),
        (20, 2, RecordError::PayloadFormat(2)),
        (22, 2, RecordError::Compression(2)),
    ] {
        let mut bytes = framed(0);
        bytes[offset] = replacement;
        assert_eq!(
            Record::parse(&bytes, RecordLimits::default()).unwrap_err(),
            expected
        );
    }
}

#[test]
fn encoded_and_declared_decoded_limits_are_independent() {
    let bytes = framed(0);
    let encoded_limit = RecordLimits {
        max_encoded_len: PAYLOAD.len() as u64 - 1,
        ..RecordLimits::default()
    };
    let decoded_limit = RecordLimits {
        max_decoded_len: DECODED.len() as u64 - 1,
        ..RecordLimits::default()
    };
    assert_eq!(
        Record::parse(&bytes, encoded_limit).unwrap_err(),
        RecordError::EncodedLimit
    );
    assert_eq!(
        Record::parse(&bytes, decoded_limit).unwrap_err(),
        RecordError::DecodedLimit
    );
}

#[test]
fn empty_payloads_and_non_zstd_payloads_are_rejected() {
    assert_eq!(
        RecordHeader::new(DocumentId(0), 0, 1).unwrap_err(),
        RecordError::EmptyPayload
    );
    assert_eq!(
        RecordHeader::new(DocumentId(0), 1, 0).unwrap_err(),
        RecordError::EmptyPayload
    );
    for field in [24..32, 32..40] {
        let mut bytes = framed(0);
        bytes[field].fill(0);
        assert_eq!(
            Record::parse(&bytes, RecordLimits::default()).unwrap_err(),
            RecordError::EmptyPayload
        );
    }
    let mut bytes = framed(0);
    bytes[RECORD_HEADER_LEN] = 0;
    assert_eq!(
        Record::parse(&bytes, RecordLimits::default()).unwrap_err(),
        RecordError::ZstdMagic
    );
}

#[test]
fn byte_ranges_remain_64_bit_and_overflow_is_controlled() {
    let header = RecordHeader::new(DocumentId(0), 123, 456).unwrap();
    let offset = (1_u64 << 32) + 7;
    assert_eq!(
        header.payload_range(offset).unwrap(),
        offset + 40..offset + 163
    );
    assert_eq!(
        header.payload_range(u64::MAX).unwrap_err(),
        RecordError::RangeOverflow
    );
    assert_eq!(
        header.payload_range(u64::MAX - 40).unwrap_err(),
        RecordError::RangeOverflow
    );
    assert_eq!(
        RecordHeader::new(DocumentId(0), u64::MAX, 1).unwrap_err(),
        RecordError::RangeOverflow
    );
}

#[test]
fn hostile_length_does_not_trigger_an_allocation_or_truncating_cast() {
    let mut bytes = framed(0);
    bytes[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
    let limits = RecordLimits {
        max_encoded_len: u64::MAX,
        max_decoded_len: u64::MAX,
    };
    assert_eq!(
        Record::parse(&bytes, limits).unwrap_err(),
        RecordError::RangeOverflow
    );
}
