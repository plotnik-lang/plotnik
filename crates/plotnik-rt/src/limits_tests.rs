use crate::{Limit, LimitExceeded, REPLAY_DEPTH_AUTO, RuntimeLimitSpec, replay_depth_auto};

#[test]
fn fuel_limit_resolves_and_reports_canonically() {
    let limits = RuntimeLimitSpec {
        fuel_limit: Limit::Of(42),
        memory: Limit::Unbounded,
    }
    .resolve(1);

    assert_eq!(limits.fuel_limit, Some(42));
    assert_eq!(
        LimitExceeded::OutOfFuel(42).to_string(),
        "exhausted the fuel limit of 42"
    );
}

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
