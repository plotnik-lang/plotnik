use super::{Frame, FrameArena, FrameReturns, ReturnOutcome};

#[test]
fn ordinary_frame_returns_to_its_only_continuation() {
    let mut frames = FrameArena::new();
    frames.push(FrameReturns::single(7));

    assert_eq!(frames.pop(ReturnOutcome::Matched), 7);
    assert!(frames.is_empty());
}

#[test]
fn split_frame_selects_the_reported_outcome_after_restore() {
    let mut frames = FrameArena::new();
    let frame = frames.push(FrameReturns::split(11, 13));

    assert_eq!(frames.pop(ReturnOutcome::Matched), 11);

    frames.restore(Some(frame));
    assert_eq!(frames.pop(ReturnOutcome::Empty), 13);
}

#[test]
fn split_routing_keeps_call_frames_compact() {
    assert_eq!(std::mem::size_of::<Frame>(), 12);
}
