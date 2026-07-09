use crate::{REPLAY_DEPTH_AUTO, replay_depth_auto};

#[test]
fn replay_depth_auto_scales_with_reader_frame_size() {
    assert!(replay_depth_auto(512) > REPLAY_DEPTH_AUTO);
    assert_eq!(replay_depth_auto(1536), REPLAY_DEPTH_AUTO);
    assert!(replay_depth_auto(4096) < REPLAY_DEPTH_AUTO);
}

#[test]
fn replay_depth_auto_never_resolves_to_zero() {
    assert_eq!(replay_depth_auto(u64::MAX), 1);
}
