use crate::{Limit, LimitExceeded, RuntimeLimitSpec, decode_depth_auto};

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
fn decode_depth_auto_scales_with_decoder_frame_size() {
    let baseline = decode_depth_auto(1536);
    assert!(decode_depth_auto(512) > baseline);
    assert!(decode_depth_auto(4096) < baseline);
}

#[test]
fn decode_depth_auto_never_resolves_to_zero() {
    assert_eq!(decode_depth_auto(u64::MAX), 1);
}
