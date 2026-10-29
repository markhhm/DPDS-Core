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
            q_bytes: 1_000_000,
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
    assert_eq!(Probability::ZERO.sample(0, 0), false);
    assert_eq!(Probability::ONE.sample(0, 0), true);
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
