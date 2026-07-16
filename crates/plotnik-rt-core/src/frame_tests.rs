use super::{Frame, FrameArena, PortId};

#[test]
fn frame_returns_its_call_site_token() {
    let mut frames = FrameArena::new();
    frames.push(7);

    assert_eq!(frames.pop(), 7);
    assert!(frames.is_empty());
}

#[test]
fn restored_frame_returns_the_same_call_site_token() {
    let mut frames = FrameArena::new();
    let frame = frames.push(11);

    assert_eq!(frames.pop(), 11);

    frames.restore(Some(frame));
    assert_eq!(frames.pop(), 11);
}

#[test]
fn checkpoint_high_water_retains_a_detached_cactus_branch() {
    let mut frames = FrameArena::new();
    frames.push(3);
    let checkpointed = frames.push(5);

    assert_eq!(frames.pop(), 5);
    frames.prune(Some(checkpointed));
    frames.restore(Some(checkpointed));

    assert_eq!(frames.pop(), 5);
    assert_eq!(frames.pop(), 3);
}

#[test]
fn pruning_drops_frames_above_the_active_parent() {
    let mut frames = FrameArena::new();
    frames.push(3);
    frames.push(5);

    assert_eq!(frames.pop(), 5);
    frames.prune(None);

    assert_eq!(frames.byte_footprint(), std::mem::size_of::<Frame>() as u64);
    assert_eq!(frames.pop(), 3);
}

#[test]
fn port_ids_cover_exactly_the_eight_port_universe() {
    for index in 0..PortId::COUNT {
        let port = PortId::from_byte(index).expect("port is in range");
        assert_eq!(port.to_byte(), index);
        assert_eq!(port.index(), usize::from(index));
    }

    assert_eq!(PortId::from_byte(PortId::COUNT), None);
    assert_eq!(PortId::from_byte(u8::MAX), None);
}

#[test]
fn call_site_routing_keeps_frames_compact() {
    assert_eq!(std::mem::size_of::<Frame>(), 12);
}
