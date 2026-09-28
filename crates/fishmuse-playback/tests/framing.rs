use fishmuse_playback::foobar::framing::{FrameDecoder, MAX_FRAME_SIZE, encode_frame};
use fishmuse_playback::foobar::protocol::ProtocolErrorCode;

#[test]
fn rejects_zero_length_and_oversized_frames_from_the_prefix() {
    let mut decoder = FrameDecoder::default();
    let zero = decoder
        .push(&0_u32.to_le_bytes())
        .expect_err("zero length must be rejected");
    assert_eq!(zero.code(), ProtocolErrorCode::ProtocolInvalid);

    let mut decoder = FrameDecoder::default();
    let oversized = u32::try_from(MAX_FRAME_SIZE + 1).unwrap().to_le_bytes();
    let error = decoder
        .push(&oversized)
        .expect_err("oversized prefix must be rejected before a body is supplied");
    assert_eq!(error.code(), ProtocolErrorCode::FrameTooLarge);
    assert_eq!(error.code().as_str(), "frame_too_large");
}

#[test]
fn buffers_truncated_frames_and_reports_them_when_the_stream_finishes() {
    let mut decoder = FrameDecoder::default();
    let frame = encode_frame(br#"{"kind":"ping"}"#).unwrap();
    assert!(decoder.push(&frame[..6]).unwrap().is_empty());

    let error = decoder
        .finish()
        .expect_err("a partial frame at EOF must be rejected");
    assert_eq!(error.code(), ProtocolErrorCode::ProtocolInvalid);

    let decoded = decoder.push(&frame[6..]).unwrap();
    assert_eq!(decoded, vec![br#"{"kind":"ping"}"#.to_vec()]);
    decoder.finish().unwrap();
}

#[test]
fn decodes_multiple_coalesced_frames_without_losing_boundaries() {
    let first = encode_frame(b"first").unwrap();
    let second = encode_frame(b"second").unwrap();
    let joined = [first, second].concat();

    let mut decoder = FrameDecoder::default();
    let frames = decoder.push(&joined).unwrap();

    assert_eq!(frames, vec![b"first".to_vec(), b"second".to_vec()]);
    decoder.finish().unwrap();
}

#[test]
fn accepts_the_exact_limit_and_rejects_larger_outbound_frames() {
    let maximum = vec![b'x'; MAX_FRAME_SIZE];
    let frame = encode_frame(&maximum).expect("the exact limit is valid");
    assert_eq!(frame.len(), MAX_FRAME_SIZE + 4);

    let error = encode_frame(&vec![b'x'; MAX_FRAME_SIZE + 1])
        .expect_err("an oversized outbound frame must be rejected");
    assert_eq!(error.code(), ProtocolErrorCode::FrameTooLarge);
}
