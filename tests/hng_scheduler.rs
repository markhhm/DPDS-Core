// SPDX-License-Identifier: Apache-2.0
use dpds_core::hng::{HngBuffer, HngConfig, Probability, PushResult};
use std::ffi::c_void;

fn config() -> HngConfig {
    HngConfig {
        c_bps: 8_000_000_000,
        q_bytes: 80,
        t_ns: 1000,
        delta_ns: 0,
        p: Probability::ZERO,
        seed: 0,
        storage_capacity_packets: 32,
        storage_capacity_bytes: 100_000,
    }
}

fn scheduler() -> HngBuffer {
    HngBuffer::new(config()).unwrap()
}

fn packet(id: usize) -> *mut c_void {
    id as *mut c_void
}

#[test]
fn deadline_includes_first_packets_service_before_propagation() {
    let mut b = scheduler();
    // 76 mbuf bytes + 4 FCS + 20 preamble/IFG = 100 wire bytes;
    // at 8 Gbit/s service takes 100 ns, then propagation takes 1000 ns.
    assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
    assert_eq!(b.front().unwrap().deadline_ns, 1100);
}

#[test]
fn q_releases_at_service_completion_while_packet_remains_in_delay() {
    let mut b = scheduler();
    assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
    assert_eq!(b.push(packet(2), 76, 99), PushResult::QDrop);
    assert_eq!(b.push(packet(3), 76, 100), PushResult::Accepted);
    assert_eq!(b.front().unwrap().deadline_ns, 1100);
}

#[test]
fn capacity_counts_the_in_service_packet_and_whole_wire_bytes() {
    for (q, expected) in [
        (0, PushResult::QDrop),
        (79, PushResult::QDrop),
        (80, PushResult::Accepted),
    ] {
        let mut b = HngBuffer::new(HngConfig {
            q_bytes: q,
            ..config()
        })
        .unwrap();
        assert_eq!(b.push(packet(1), 76, 0), expected);
    }
    let mut b = scheduler();
    assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
    assert_eq!(b.push(packet(2), 76, 99), PushResult::QDrop);
    assert_eq!(b.push(packet(3), 76, 100), PushResult::Accepted);
    assert_eq!(b.push(packet(4), 76, 100), PushResult::QDrop);
    assert_eq!(b.stats().q_bytes, 80);
}

#[test]
fn propagation_does_not_change_q_admission() {
    let mut a = scheduler();
    let mut b = HngBuffer::new(HngConfig {
        t_ns: 500_000_000,
        delta_ns: 320_000_000,
        p: Probability::ONE,
        ..config()
    })
    .unwrap();
    for (i, now) in [0, 0, 99, 100, 199, 200].into_iter().enumerate() {
        assert_eq!(
            a.push(packet(i + 1), 76, now),
            b.push(packet(i + 1), 76, now)
        );
    }
    assert_eq!(a.stats().q_drops, 3);
    assert_eq!(b.stats().q_drops, 3);
}

#[test]
fn opposite_directions_do_not_share_admission_budget() {
    let mut forward = scheduler();
    let mut reverse = scheduler();
    assert_eq!(forward.push(packet(1), 76, 0), PushResult::Accepted);
    assert_eq!(reverse.push(packet(2), 76, 0), PushResult::Accepted);
    assert_eq!(forward.push(packet(3), 76, 0), PushResult::QDrop);
    assert_eq!(reverse.front().unwrap().deadline_ns, 1100);
}

#[test]
fn serialization_preserves_fractional_ns_without_per_packet_rounding_drift() {
    let mut b = HngBuffer::new(HngConfig {
        c_bps: 100_000_000_000,
        q_bytes: 100_000,
        t_ns: 0,
        ..config()
    })
    .unwrap();
    for i in 1..=25 {
        assert_eq!(b.push(packet(i), 1514, 0), PushResult::Accepted);
    }
    for i in 1..=25 {
        let p = b.pop().unwrap();
        // 1538 wire bytes take exactly 123.04 ns at 100G.
        assert_eq!(p.deadline_ns, (i as u64 * 3076).div_ceil(25));
    }
}

#[test]
fn q_drain_is_exact_at_the_explicit_ledger_watermark_and_read_only() {
    let mut b = HngBuffer::new(HngConfig {
        c_bps: 3_000_000_000,
        q_bytes: 160,
        ..config()
    })
    .unwrap();
    assert_eq!(b.q_drain_ns(), 0);
    assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
    assert_eq!(b.push(packet(2), 76, 0), PushResult::Accepted);
    // Two 100-byte wire frames finish at exactly 1600/3 ns.
    assert_eq!(b.q_drain_ns(), 534);
    assert_eq!(b.push(packet(3), 1518, 100), PushResult::QDrop);
    assert_eq!(b.q_drain_ns(), 434);
    let before = b.stats();
    assert!(b.try_pop(10_000, 0).is_none());
    assert_eq!(b.q_drain_ns(), 434);
    assert_eq!(b.stats().ledger_watermark_ns, before.ledger_watermark_ns);
    assert_eq!(b.stats().q_bytes, before.q_bytes);
    assert_eq!(b.push(packet(4), 1518, 267), PushResult::QDrop);
    assert_eq!((b.stats().q_bytes, b.q_drain_ns()), (80, 267));
    b.end_input();
    assert!(b.drain_advance(533));
    assert_eq!(b.q_drain_ns(), 1);
    assert!(b.drain_advance(534));
    assert_eq!((b.stats().q_bytes, b.q_drain_ns()), (0, 0));
    assert!(!b.is_empty());
}

#[test]
fn selector_counter_follows_actual_validation_q_and_storage_outcomes() {
    let mut b = HngBuffer::new(HngConfig {
        storage_capacity_packets: 1,
        ..config()
    })
    .unwrap();
    assert_eq!(b.next_selection_seq(), 0);
    assert_eq!(
        b.push_with_seq(packet(1), 0, 0, 0),
        PushResult::InvalidInput
    );
    assert_eq!(b.next_selection_seq(), 0);
    assert_eq!(b.push_with_seq(packet(2), 76, 0, 1), PushResult::Accepted);
    assert_eq!(b.next_selection_seq(), 1);
    assert_eq!(b.push_with_seq(packet(3), 76, 0, 2), PushResult::QDrop);
    assert_eq!(b.next_selection_seq(), 2);
    assert_eq!(
        b.push_with_seq(packet(4), 76, 100, 3),
        PushResult::StorageDrop
    );
    assert_eq!(b.next_selection_seq(), 3);
    b.pop().unwrap();
    assert_eq!(b.push_with_seq(packet(5), 76, 100, 4), PushResult::Accepted);
    assert_eq!(b.front().unwrap().selection_seq, 3);
    assert_eq!(b.next_selection_seq(), 4);
}

#[test]
fn late_deadline_has_exact_delta_and_storage_never_drops_below_capacity() {
    for p in [Probability::ZERO, Probability::ONE] {
        let mut b = HngBuffer::new(HngConfig {
            q_bytes: 100_000,
            delta_ns: 320_000_000,
            p,
            ..config()
        })
        .unwrap();
        for i in 1..=32 {
            assert_eq!(b.push(packet(i), 76, i as u64 * 100), PushResult::Accepted);
        }
        for i in 1..=32 {
            let item = b.pop().unwrap();
            assert_eq!(item.packet_ptr, packet(i));
            assert_eq!(item.late, p == Probability::ONE);
            assert_eq!(
                item.deadline_ns,
                i as u64 * 100
                    + 100
                    + 1000
                    + if p == Probability::ONE {
                        320_000_000
                    } else {
                        0
                    }
            );
        }
        assert_eq!(b.stats().storage_drops, 0);
        assert_eq!(b.stats().storage_bytes, 0);
    }
}

#[test]
fn storage_drop_does_not_consume_q_or_extend_service_timeline() {
    let mut b = HngBuffer::new(HngConfig {
        storage_capacity_packets: 1,
        ..config()
    })
    .unwrap();
    assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
    assert_eq!(b.push(packet(2), 76, 100), PushResult::StorageDrop);
    assert_eq!(b.stats().q_bytes, 0);
    assert_eq!(b.pop().unwrap().packet_ptr, packet(1));
    assert_eq!(b.push(packet(3), 76, 100), PushResult::Accepted);
    assert_eq!(b.front().unwrap().deadline_ns, 1200);
    let s = b.stats();
    assert_eq!((s.accepted, s.storage_drops, s.q_drops), (2, 1, 0));
    assert_eq!((s.accepted_bytes, s.storage_drop_bytes), (152, 76));
}

#[test]
fn storage_byte_capacity_is_independent_of_q_and_packet_slots() {
    let mut b = HngBuffer::new(HngConfig {
        storage_capacity_bytes: 75,
        ..config()
    })
    .unwrap();
    assert_eq!(b.push(packet(1), 76, 0), PushResult::StorageDrop);
    assert_eq!((b.stats().q_bytes, b.stats().storage_bytes), (0, 0));
}

#[test]
fn malformed_config_and_clock_or_deadline_overflow_are_rejected() {
    for c in [
        HngConfig {
            c_bps: 0,
            ..config()
        },
        HngConfig {
            storage_capacity_packets: 0,
            ..config()
        },
        HngConfig {
            storage_capacity_bytes: 0,
            ..config()
        },
        HngConfig {
            t_ns: u64::MAX,
            delta_ns: 1,
            ..config()
        },
    ] {
        assert!(HngBuffer::new(c).is_err(), "accepted invalid config: {c:?}");
    }
    let mut b = scheduler();
    assert_eq!(b.push(packet(1), 76, 100), PushResult::Accepted);
    assert_eq!(b.push(packet(2), 76, 99), PushResult::InvalidInput);
    assert_eq!(
        b.push(std::ptr::null_mut(), 76, 100),
        PushResult::InvalidInput
    );
    assert_eq!(b.push(packet(3), 0, 100), PushResult::InvalidInput);
    assert_eq!(b.push(packet(4), 76, u64::MAX), PushResult::InvalidInput);
    assert_eq!(b.stats().invalid_drops, 4);
    assert_eq!(b.stats().accepted, 1);
}

#[test]
fn mixed_seeded_stream_matches_sorted_deadline_oracle_and_replays() {
    for seed in 0u64..32 {
        let c = HngConfig {
            q_bytes: 100_000_000,
            p: "0.5".parse().unwrap(),
            delta_ns: 100,
            storage_capacity_packets: 4096,
            storage_capacity_bytes: 10_000_000,
            seed,
            ..config()
        };
        let mut first = HngBuffer::new(c).unwrap();
        let mut replay = HngBuffer::new(c).unwrap();
        // Independent oracle sorts the observed per-packet selection, not the
        // scheduler queues. Arrival/size/flow vary; the service clock is exact.
        let mut rng = seed + 1;
        let mut now = 0;
        let mut finish = 0;
        let mut expected = Vec::new();
        for i in 1..=1024usize {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            now += (rng >> 32) % 200;
            let len = 60 + ((rng >> 16) % 1455) as u32;
            finish = now.max(finish) + len as u64 + 24;
            assert_eq!(first.push(packet(i), len, now), PushResult::Accepted);
            assert_eq!(replay.push(packet(i), len, now), PushResult::Accepted);
            expected.push((i, finish + c.t_ns));
        }
        let mut actual = Vec::new();
        while let Some(item) = first.pop() {
            let repeat = replay.pop().unwrap();
            assert_eq!(
                (item.packet_ptr, item.deadline_ns, item.late),
                (repeat.packet_ptr, repeat.deadline_ns, repeat.late)
            );
            let id = item.packet_ptr as usize;
            assert_eq!(item.ingress_seq, id as u64 - 1);
            let base = expected[id - 1].1;
            assert_eq!(
                item.deadline_ns,
                base + if item.late { c.delta_ns } else { 0 }
            );
            actual.push((item.deadline_ns, item.ingress_seq, id, item.late));
        }
        assert!(replay.pop().is_none());
        let mut oracle = actual.clone();
        oracle.sort_by_key(|p| (p.0, p.1));
        assert_eq!(actual, oracle);
        for late in [false, true] {
            let mut last_by_flow = [0; 7];
            for (_, _, id, selected) in &actual {
                if *selected == late {
                    assert!(*id > last_by_flow[id % 7]);
                    last_by_flow[id % 7] = *id;
                }
            }
        }
        let selected = actual.iter().filter(|p| p.3).count();
        assert!(
            (400..625).contains(&selected),
            "seed={seed}, selected={selected}"
        );
        let s = first.stats();
        assert_eq!(
            (s.accepted, s.dequeued, s.storage_drops, s.q_drops),
            (1024, 1024, 0, 0)
        );
        assert_eq!(s.accepted_bytes, s.dequeued_bytes);
        assert_eq!(s.storage_bytes, 0);
    }
}

#[test]
fn exact_decimal_probability_and_selector_have_published_vectors() {
    let p: Probability = "0.10".parse().unwrap();
    assert_eq!(p.threshold, 1_844_674_407_370_955_161);
    for bad in [
        "-0.1",
        "1.1",
        "NaN",
        "inf",
        "1e-2",
        "0.12345678901234567890",
    ] {
        assert!(bad.parse::<Probability>().is_err());
    }
    assert!(!Probability::ZERO.sample(0, 0));
    assert!(Probability::ONE.sample(0, 0));
    assert_eq!(dpds_core::hng::selection_word(0, 0), 0xe220a8397b1dcdaf);
    assert_eq!(dpds_core::hng::selection_word(0, 1), 0x6e789e6aa1b965f4);
    assert_eq!(dpds_core::hng::selection_word(0, 2), 0x06c45d188009454f);
}

#[test]
fn cross_queue_equal_deadline_tie_and_permitted_late_inversion() {
    // Seed 3 gives late then ordinary for p=0.5 (fixed counter vectors).
    for (delta, order) in [(100, vec![1, 2]), (101, vec![2, 1])] {
        let mut b = HngBuffer::new(HngConfig {
            seed: 3,
            p: "0.5".parse().unwrap(),
            q_bytes: 160,
            delta_ns: delta,
            t_ns: 10,
            ..config()
        })
        .unwrap();
        assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
        assert_eq!(b.push(packet(2), 76, 0), PushResult::Accepted);
        assert_eq!(b.front().unwrap().deadline_ns, 210);
        assert!(b.try_pop(209, u64::MAX).is_none());
        assert_eq!(
            b.try_pop(210, u64::MAX).unwrap().packet_ptr,
            packet(order[0])
        );
        assert_eq!(
            b.try_pop(211, u64::MAX).unwrap().packet_ptr,
            packet(order[1])
        );
        assert!(b.try_pop(u64::MAX, u64::MAX).is_none());
    }
}

#[test]
fn dequeue_wall_time_does_not_age_admission_ledger() {
    let mut b = HngBuffer::new(HngConfig {
        q_bytes: 160,
        ..config()
    })
    .unwrap();
    assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
    assert_eq!(b.push(packet(2), 76, 0), PushResult::Accepted);
    assert!(b.try_pop(50_000, u64::MAX).is_some());
    // This packet was already timestamped by RX but delayed in its handoff.
    assert_eq!(b.push(packet(3), 76, 50), PushResult::QDrop);
    assert_eq!(b.stats().admission_watermark_ns, 50);
}

#[test]
fn dropped_packets_still_consume_sequence_and_selector_draw() {
    let mut a = HngBuffer::new(HngConfig {
        q_bytes: 160,
        p: "0.5".parse().unwrap(),
        ..config()
    })
    .unwrap();
    let mut b = HngBuffer::new(HngConfig {
        q_bytes: 80,
        p: "0.5".parse().unwrap(),
        ..config()
    })
    .unwrap();
    for i in 0..3 {
        let now = if i < 2 { 0 } else { 200 };
        a.push(packet(i + 1), 76, now);
        b.push(packet(i + 1), 76, now);
    }
    let x = (0..3)
        .map(|_| a.pop().unwrap())
        .find(|p| p.packet_ptr == packet(3))
        .unwrap();
    let y = (0..2)
        .map(|_| b.pop().unwrap())
        .find(|p| p.packet_ptr == packet(3))
        .unwrap();
    assert_eq!((x.ingress_seq, x.late), (y.ingress_seq, y.late));
    assert_eq!(x.ingress_seq, 2);
}

#[test]
fn queued_old_ordinary_packet_prevents_premature_late_output() {
    let mut b = HngBuffer::new(HngConfig {
        seed: 3,
        q_bytes: 160,
        p: "0.5".parse().unwrap(),
        t_ns: 10,
        delta_ns: 1000,
        ..config()
    })
    .unwrap();
    assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
    assert!(b.try_pop(5000, 0).is_none());
    assert_eq!(b.push(packet(2), 76, 1), PushResult::Accepted);
    assert!(b.try_pop(5000, 1).is_none());
    assert_eq!(b.try_pop(5000, 5000).unwrap().packet_ptr, packet(2));
    assert_eq!(b.try_pop(5000, 5000).unwrap().packet_ptr, packet(1));
}

#[test]
fn trace_delay_steps_up_and_down_remain_fifo() {
    use dpds_core::hng::{TraceProfile, TraceRow};
    for (delays, deadlines) in [([10, 1000], [110, 1200]), ([1000, 10], [1100, 1100])] {
        let trace = TraceProfile {
            terminal_horizon_ns: None,
            outage_action: dpds_core::hng::OutageAction::Drop,
            terminal: dpds_core::hng::TerminalPolicy::Error,
            loss_seed: 7,
            end_ns: 10_000,
            rows: vec![
                TraceRow {
                    at_ns: 0,
                    t_ns: delays[0],
                    loss: Probability::ZERO,
                    outage: false,
                },
                TraceRow {
                    at_ns: 200,
                    t_ns: delays[1],
                    loss: Probability::ZERO,
                    outage: false,
                },
            ],
        };
        let mut b = HngBuffer::with_trace(
            HngConfig {
                q_bytes: 160,
                ..config()
            },
            trace,
        )
        .unwrap();
        assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
        assert_eq!(b.push(packet(2), 76, 0), PushResult::Accepted);
        for (id, deadline) in [1, 2].into_iter().zip(deadlines) {
            let p = b.try_pop(10_000, u64::MAX).unwrap();
            assert_eq!((p.packet_ptr, p.deadline_ns), (packet(id), deadline));
        }
    }
}

#[test]
fn trace_loss_outage_and_end_are_distinct_from_q_or_storage_loss() {
    use dpds_core::hng::{TraceProfile, TraceRow};
    let trace = TraceProfile {
        terminal_horizon_ns: None,
        outage_action: dpds_core::hng::OutageAction::Drop,
        terminal: dpds_core::hng::TerminalPolicy::Error,
        loss_seed: 7,
        end_ns: 400,
        rows: vec![
            TraceRow {
                at_ns: 0,
                t_ns: 10,
                loss: Probability::ZERO,
                outage: false,
            },
            TraceRow {
                at_ns: 200,
                t_ns: 10,
                loss: Probability::ONE,
                outage: false,
            },
            TraceRow {
                at_ns: 300,
                t_ns: 10,
                loss: Probability::ZERO,
                outage: true,
            },
        ],
    };
    let mut b = HngBuffer::with_trace(
        HngConfig {
            q_bytes: 800,
            ..config()
        },
        trace,
    )
    .unwrap();
    for (id, expected) in [
        (1, PushResult::Accepted),
        (2, PushResult::TraceLoss),
        (3, PushResult::TraceOutage),
        (4, PushResult::InvalidInput),
    ] {
        assert_eq!(b.push(packet(id), 76, 0), expected);
    }
    let s = b.stats();
    assert_eq!(
        (s.q_bytes, s.trace_loss_drops, s.trace_outage_drops),
        (240, 1, 1)
    );
    assert_eq!((s.q_drops, s.storage_drops), (0, 0));
    assert_eq!(b.pop().unwrap().packet_ptr, packet(1));
    assert!(b.pop().is_none());
}

#[test]
fn malformed_trace_and_unimplemented_packet_late_mode_are_rejected() {
    use dpds_core::hng::{TraceProfile, TraceRow};
    let row = TraceRow {
        at_ns: 0,
        t_ns: 10,
        loss: Probability::ZERO,
        outage: false,
    };
    for trace in [
        TraceProfile {
            terminal_horizon_ns: None,
            outage_action: dpds_core::hng::OutageAction::Drop,
            terminal: dpds_core::hng::TerminalPolicy::Error,
            loss_seed: 7,
            end_ns: 100,
            rows: vec![],
        },
        TraceProfile {
            terminal_horizon_ns: None,
            outage_action: dpds_core::hng::OutageAction::Drop,
            terminal: dpds_core::hng::TerminalPolicy::Error,
            loss_seed: 7,
            end_ns: 100,
            rows: vec![TraceRow { at_ns: 1, ..row }],
        },
        TraceProfile {
            terminal_horizon_ns: None,
            outage_action: dpds_core::hng::OutageAction::Drop,
            terminal: dpds_core::hng::TerminalPolicy::Error,
            loss_seed: 7,
            end_ns: 100,
            rows: vec![row, row],
        },
        TraceProfile {
            terminal_horizon_ns: None,
            outage_action: dpds_core::hng::OutageAction::Drop,
            terminal: dpds_core::hng::TerminalPolicy::Error,
            loss_seed: 7,
            end_ns: 0,
            rows: vec![row],
        },
    ] {
        assert!(HngBuffer::with_trace(config(), trace).is_err());
    }
    assert!(HngBuffer::with_trace(
        HngConfig {
            p: Probability::ONE,
            delta_ns: 1,
            ..config()
        },
        TraceProfile {
            terminal_horizon_ns: None,
            outage_action: dpds_core::hng::OutageAction::Drop,
            terminal: dpds_core::hng::TerminalPolicy::Error,
            loss_seed: 7,
            end_ns: 100,
            rows: vec![row]
        }
    )
    .is_err());
}

#[test]
fn trace_outage_hold_and_drop_apply_at_candidate_deadline() {
    use dpds_core::hng::{OutageAction, TerminalPolicy, TraceProfile, TraceRow};
    for action in [OutageAction::Hold, OutageAction::Drop] {
        let trace = TraceProfile {
            terminal_horizon_ns: Some(1_000_000),
            outage_action: action,
            terminal: TerminalPolicy::HoldLastSample,
            loss_seed: 42,
            end_ns: 1000,
            rows: vec![
                TraceRow {
                    at_ns: 0,
                    t_ns: 110,
                    loss: Probability::ZERO,
                    outage: false,
                },
                TraceRow {
                    at_ns: 200,
                    t_ns: 20,
                    loss: Probability::ZERO,
                    outage: true,
                },
                TraceRow {
                    at_ns: 300,
                    t_ns: 10,
                    loss: Probability::ZERO,
                    outage: false,
                },
            ],
        };
        let mut b = HngBuffer::with_trace(
            HngConfig {
                q_bytes: 800,
                ..config()
            },
            trace,
        )
        .unwrap();
        for i in 1..=3 {
            assert_eq!(
                b.push(packet(i), 76, 0),
                if action == OutageAction::Drop && i < 3 {
                    PushResult::TraceOutage
                } else {
                    PushResult::Accepted
                }
            );
        }
        if action == OutageAction::Hold {
            assert!(b.try_pop(250, u64::MAX).is_none());
            for id in [1, 2] {
                let p = b.try_pop(300, u64::MAX).unwrap();
                assert_eq!((p.packet_ptr, p.release_ns), (packet(id), 300));
            }
        }
        assert_eq!(b.try_pop(310, u64::MAX).unwrap().packet_ptr, packet(3));
    }
}

#[test]
fn failed_storage_reservation_does_not_advance_trace_cursor() {
    use dpds_core::hng::{OutageAction, TerminalPolicy, TraceProfile, TraceRow};
    let trace = TraceProfile {
        terminal_horizon_ns: Some(1_000_000),
        outage_action: OutageAction::Drop,
        terminal: TerminalPolicy::HoldLastSample,
        loss_seed: 0,
        end_ns: 100_000,
        rows: vec![
            TraceRow {
                at_ns: 0,
                t_ns: 1000,
                loss: Probability::ZERO,
                outage: false,
            },
            TraceRow {
                at_ns: 500,
                t_ns: 10_000,
                loss: Probability::ZERO,
                outage: false,
            },
        ],
    };
    let mut b = HngBuffer::with_trace(
        HngConfig {
            q_bytes: 10_000,
            storage_capacity_packets: 1,
            ..config()
        },
        trace,
    )
    .unwrap();
    assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
    assert_eq!(b.push(packet(2), 1514, 0), PushResult::StorageDrop);
    b.pop().unwrap();
    assert_eq!(b.push(packet(3), 76, 100), PushResult::Accepted);
    assert_eq!(b.front().unwrap().deadline_ns, 1200);
}

#[test]
fn line_selection_ordinal_is_independent_of_other_vlan_traffic() {
    let c = HngConfig {
        q_bytes: 100_000,
        p: "0.5".parse().unwrap(),
        ..config()
    };
    let mut a = HngBuffer::new(c).unwrap();
    let mut b = HngBuffer::new(c).unwrap();
    for i in 0..16u64 {
        assert_eq!(
            a.push_with_seq(packet(i as usize + 1), 76, 0, i),
            PushResult::Accepted
        );
        assert_eq!(
            b.push_with_seq(packet(i as usize + 1), 76, 0, i * 10),
            PushResult::Accepted
        );
    }
    for i in 0..16 {
        let x = a.pop().unwrap();
        let y = b.pop().unwrap();
        assert_eq!((x.late, x.selection_seq), (y.late, y.selection_seq));
        assert_eq!(x.selection_seq, i);
    }
}

#[test]
fn q_zero_rejects_before_unused_deadline_arithmetic() {
    let mut b = HngBuffer::new(HngConfig {
        q_bytes: 0,
        ..config()
    })
    .unwrap();
    assert_eq!(b.push(packet(1), 76, u64::MAX), PushResult::QDrop);
}

#[test]
fn cumulative_trace_clamp_overflow_is_rejected_before_custody_commit() {
    use dpds_core::hng::{OutageAction, TerminalPolicy, TraceProfile, TraceRow};
    let trace = TraceProfile {
        terminal_horizon_ns: None,
        end_ns: 10_000_000_000_000_000_000,
        terminal: TerminalPolicy::Error,
        outage_action: OutageAction::Drop,
        loss_seed: 0,
        rows: vec![
            TraceRow {
                at_ns: 0,
                t_ns: 8_000_000_000_000_000_000,
                loss: Probability::ZERO,
                outage: false,
            },
            TraceRow {
                at_ns: 1_000_000_000_000,
                t_ns: 0,
                loss: Probability::ZERO,
                outage: false,
            },
        ],
    };
    let mut b = HngBuffer::with_trace(
        HngConfig {
            c_bps: 1,
            q_bytes: 800,
            ..config()
        },
        trace,
    )
    .unwrap();
    for i in 1..=3 {
        assert_eq!(b.push(packet(i), 76, 0), PushResult::Accepted);
    }
    assert_eq!(b.push(packet(4), 76, 0), PushResult::InvalidInput);
    assert_eq!(
        (b.len(), b.stats().accepted, b.stats().q_bytes),
        (3, 3, 240)
    );
}

#[test]
fn mixed_frame_stream_matches_independent_finite_q_reference() {
    for seed in 1..=64u64 {
        let mut random = seed;
        let q_cap = 64 + seed * 47;
        let c = HngConfig {
            q_bytes: q_cap,
            storage_capacity_packets: 512,
            storage_capacity_bytes: 1_000_000,
            ..config()
        };
        let mut b = HngBuffer::new(c).unwrap();
        let mut events: Vec<(u64, u64)> = Vec::new();
        let mut survivors = Vec::new();
        let mut now = 0;
        let mut tail = 0;
        let mut q_drops = 0;
        for id in 1..=512usize {
            random = random
                .wrapping_mul(2862933555777941757)
                .wrapping_add(3037000493);
            now += (random >> 20) % 700;
            let bytes = 1 + ((random >> 40) % 1518) as u32;
            let l2 = (u64::from(bytes) + 4).max(64);
            events.retain(|(finish, _)| *finish > now);
            let used: u64 = events.iter().map(|(_, q)| q).sum();
            let expected = if used + l2 > q_cap {
                q_drops += 1;
                PushResult::QDrop
            } else {
                tail = tail.max(now) + l2 + 20;
                events.push((tail, l2));
                survivors.push((id, tail, bytes));
                PushResult::Accepted
            };
            assert_eq!(
                b.push(packet(id), bytes, now),
                expected,
                "seed={seed}, id={id}"
            );
            assert_eq!(
                b.stats().q_bytes,
                events.iter().map(|(_, q)| q).sum::<u64>()
            );
        }
        for (id, service, bytes) in survivors {
            let p = b.pop().unwrap();
            assert_eq!(
                (p.packet_ptr, p.service_ns, p.deadline_ns, p.packet_bytes),
                (packet(id), service, service + 1000, bytes)
            );
        }
        assert!(b.pop().is_none());
        assert_eq!((b.stats().q_drops, b.stats().storage_drops), (q_drops, 0));
        assert_eq!(b.stats().accepted_bytes, b.stats().dequeued_bytes);
    }
}

#[test]
fn minimum_and_vlan_frame_accounting_use_one_fcs_and_wire_overhead() {
    for (payload, q, wire) in [
        (40, 64, 84),
        (60, 64, 84),
        (1514, 1518, 1538),
        (1518, 1522, 1542),
    ] {
        let mut b = HngBuffer::new(HngConfig {
            q_bytes: q,
            ..config()
        })
        .unwrap();
        assert_eq!(b.push(packet(1), payload, 0), PushResult::Accepted);
        let p = b.front().unwrap();
        assert_eq!((p.l2_bytes, p.wire_bytes, p.service_ns), (q, wire, wire));
        assert_eq!(
            (
                b.stats().accepted_bytes,
                b.stats().accepted_l2_bytes,
                b.stats().accepted_wire_bytes
            ),
            (u64::from(payload), q, wire)
        );
    }
}

#[test]
fn terminal_horizon_is_enforced_even_when_every_packet_is_trace_lost() {
    use dpds_core::hng::{OutageAction, TerminalPolicy, TraceProfile, TraceRow};
    for (arrival, expected) in [
        (1899, PushResult::TraceLoss),
        (1900, PushResult::InvalidInput),
        (2000, PushResult::InvalidInput),
    ] {
        let trace = TraceProfile {
            end_ns: 1000,
            terminal_horizon_ns: Some(2000),
            terminal: TerminalPolicy::HoldLastSample,
            outage_action: OutageAction::Drop,
            loss_seed: 0,
            rows: vec![TraceRow {
                at_ns: 0,
                t_ns: 0,
                loss: Probability::ONE,
                outage: false,
            }],
        };
        let mut b = HngBuffer::with_trace(
            HngConfig {
                q_bytes: 800,
                ..config()
            },
            trace,
        )
        .unwrap();
        assert_eq!(b.push(packet(1), 76, arrival), expected);
        assert_eq!(
            b.stats().q_bytes,
            if expected == PushResult::TraceLoss {
                80
            } else {
                0
            }
        );
    }
}

#[test]
fn core_handoff_keeps_pre_outage_head_visible_for_later_tx_arbitration() {
    use dpds_core::hng::{OutageAction, TerminalPolicy, TraceProfile, TraceRow};
    let trace = TraceProfile {
        end_ns: 1000,
        terminal_horizon_ns: Some(10_000),
        terminal: TerminalPolicy::HoldLastSample,
        outage_action: OutageAction::Hold,
        loss_seed: 0,
        rows: vec![
            TraceRow {
                at_ns: 0,
                t_ns: 90,
                loss: Probability::ZERO,
                outage: false,
            },
            TraceRow {
                at_ns: 200,
                t_ns: 90,
                loss: Probability::ZERO,
                outage: true,
            },
            TraceRow {
                at_ns: 300,
                t_ns: 90,
                loss: Probability::ZERO,
                outage: false,
            },
        ],
    };
    let mut b = HngBuffer::with_trace(config(), trace).unwrap();
    assert_eq!(b.push(packet(1), 76, 0), PushResult::Accepted);
    assert_eq!(b.tx_policy().gate(250), 1);
    assert_eq!(b.tx_policy().gate(300), 0);
    // TX may execute after recovery; it must receive the older head too.
    assert_eq!(b.try_pop(250, 1000).unwrap().release_ns, 190);
}

/// service_ns is the exact rational finish time rounded up, also when the rate does not divide it
/// and the numerator needs more than 64 bits.
#[test]
fn service_time_is_the_exact_ceiling_for_rates_that_do_not_divide() {
    for c_bps in [3_000_000_000u64, 7_000_000_001, 100_000_000_000] {
        let mut b = HngBuffer::new(HngConfig {
            c_bps,
            q_bytes: 100_000_000,
            storage_capacity_packets: 4096,
            storage_capacity_bytes: 10_000_000,
            ..config()
        })
        .unwrap();
        let mut finish = 0u128;
        for i in 0..2000u64 {
            let now = 50_000_000_000 + i * 37;
            let bytes = 40 + (i * 13 % 1400) as u32;
            assert_eq!(
                b.push_with_seq(packet(1 + i as usize), bytes, now, i),
                PushResult::Accepted
            );
            let wire = u128::from((u64::from(bytes) + 4).max(64) + 20);
            finish = finish.max(u128::from(now) * u128::from(c_bps)) + wire * 8_000_000_000;
            let want = finish.div_ceil(u128::from(c_bps));
            let p = b.pop().unwrap();
            assert_eq!(u128::from(p.service_ns), want, "c_bps {c_bps} packet {i}");
        }
    }
}
