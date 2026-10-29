// The initial fixture exercises upstream's Buffer; it will be switched to the
// HNG scheduler once the regressions have failed on the unmodified core.
use dpds_core::{
    buffer::{buffer_new, Buffer},
    delayer::RVDistribution,
    dropper::LossModel,
    limiter::LimiterMode,
    Config, DelayerConfig, LimiterConfig,
};
use std::ffi::c_void;

fn scheduler() -> Box<Buffer> {
    unsafe {
        Box::from_raw(buffer_new(Config {
            buffer_capacity: 32,
            reordering: false,
            delayer: DelayerConfig {
                distribution: RVDistribution::DeterministicDistribution,
                mean_ns: 1000.,
                std_ns: 0.,
                activate_correlation: false,
                half_life_period_ns: 1.,
            },
            limiter: LimiterConfig {
                mode: LimiterMode::Spacer,
                activate: true,
                rate_bps: 8_000_000_000.,
                limit_bit: 800.,
            },
            loss_model: LossModel::Random(0.),
        }))
    }
}

fn packet(id: usize) -> *mut c_void {
    id as *mut c_void
}

#[test]
fn deadline_includes_first_packets_service_before_propagation() {
    let mut b = scheduler();
    // 76 mbuf bytes + 4 FCS + 20 preamble/IFG = 100 wire bytes;
    // at 8 Gbit/s service takes 100 ns, then propagation takes 1000 ns.
    assert!(b.buffer_push(packet(1), 76, 0.));
    assert_eq!(b.buffer_front_time(), 1100.);
}

#[test]
fn q_releases_at_service_completion_while_packet_remains_in_delay() {
    let mut b = scheduler();
    assert!(b.buffer_push(packet(1), 76, 0.));
    assert!(!b.buffer_push(packet(2), 76, 99.));
    assert!(b.buffer_push(packet(3), 76, 100.));
    assert_eq!(b.buffer_front_time(), 1100.);
}
