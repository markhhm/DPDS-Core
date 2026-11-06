// SPDX-License-Identifier: Apache-2.0
use dpds_core::hng::{HngPacket, HngStats, PushResult};
use dpds_core::hng_engine::*;
use std::ptr;

fn configuration() -> String {
    let mut line = serde_json::json!({"name":"one","ingress_port":0,"egress_port":1,"vlan_id":100,
      "model":{"c_bps":8000000000_u64,"q_bytes":8000,"t_ns":10,"delta_ns":1000,
      "p":"0.5","seed":3,"storage_capacity_packets":8192,"storage_capacity_bytes":12435456},
      "envelope":{"min_q_bytes":64,"max_q_bytes":1522,"burst_bound_packets":16}});
    let first = line.clone();
    line["name"] = "two".into();
    line["vlan_id"] = 101.into();
    serde_json::json!({"schema":"hng-v1","lines":[first,line]}).to_string()
}
unsafe fn handles(engine: &mut Engine) -> [*mut Line; 2] {
    [hng_engine_line(engine, 0), hng_engine_line(engine, 1)]
}
unsafe fn stats(engine: &Engine, index: usize) -> HngStats {
    let mut out = HngStats::default();
    assert!(hng_engine_stats(engine, index, &mut out));
    out
}

#[test]
fn head_ticket_matches_old_merge_order_and_every_model_counter() {
    let cfg = configuration();
    let mut old = Engine::from_json(&cfg).unwrap();
    let mut new = Engine::from_json(&cfg).unwrap();
    let mut ids = [0u8; 32];
    unsafe {
        for (i, id) in ids.iter_mut().enumerate() {
            let pointer = (id as *mut u8).cast();
            assert_eq!(
                hng_engine_offer(&mut old, i % 2, pointer, 60, 0, i as u64),
                PushResult::Accepted
            );
            assert_eq!(
                hng_engine_offer(&mut new, i % 2, pointer, 60, 0, i as u64),
                PushResult::Accepted
            );
        }
        let old_lines = handles(&mut old);
        let new_lines = handles(&mut new);
        let w = [1_000_000; 2];
        for k in 0..2 {
            hng_line_end_input(old_lines[k]);
            hng_line_end_input(new_lines[k]);
        }
        for _ in 0..32 {
            let mut p = std::mem::MaybeUninit::<HngPacket>::uninit();
            let chosen = hng_lines_pop_merged(
                old_lines.as_ptr(),
                w.as_ptr(),
                2,
                1_000_000,
                1_000_000,
                p.as_mut_ptr(),
            );
            assert!(chosen >= 0);
            let p = p.assume_init();
            let mut head = HngHead::default();
            assert_eq!(
                hng_lines_peek_head(
                    new_lines.as_ptr(),
                    w.as_ptr(),
                    2,
                    1_000_000,
                    1_000_000,
                    &mut head
                ),
                chosen
            );
            assert_eq!(
                (
                    head.packet_ptr,
                    head.ingress_seq,
                    head.packet_bytes,
                    head.late
                ),
                (p.packet_ptr, p.ingress_seq, p.packet_bytes, p.late)
            );
            assert!(hng_head_pop(&mut head));
            assert!(head.line.is_null());
            assert!(!hng_head_pop(&mut head));
            for k in 0..2 {
                assert_eq!(
                    format!("{:?}", stats(&old, k)),
                    format!("{:?}", stats(&new, k))
                );
            }
        }
        assert_eq!(
            hng_lines_peek_head(
                new_lines.as_ptr(),
                w.as_ptr(),
                2,
                1_000_000,
                1_000_000,
                &mut HngHead::default()
            ),
            -1
        );
    }
}

#[test]
fn head_obeys_clock_watermark_limit_and_leaves_output_unchanged_when_blocked() {
    let mut engine = Engine::from_json(include_str!("fixtures/m7-tiny.json")).unwrap();
    let mut id = 0u8;
    unsafe {
        assert_eq!(
            hng_engine_offer(&mut engine, 0, (&mut id as *mut u8).cast(), 60, 0, 0),
            PushResult::Accepted
        );
        let line = [hng_engine_line(&mut engine, 0)];
        let mut head = HngHead {
            clock_ns: 77,
            ..Default::default()
        };
        for (now, watermark, limit) in [
            (671999, 1_000_000, u64::MAX),
            (1_000_000, 0, u64::MAX),
            (1_000_000, 1_000_000, 671999),
        ] {
            assert_eq!(
                hng_lines_peek_head(line.as_ptr(), &watermark, 1, now, limit, &mut head),
                -1
            );
            assert_eq!(head.clock_ns, 77);
        }
        assert_eq!(
            hng_lines_peek_head(line.as_ptr(), &1_000_000, 1, 672000, 672000, &mut head),
            0
        );
        assert_eq!(head.clock_ns, 672000);
        assert!(hng_head_pop(&mut head));
    }
}

#[test]
fn stale_or_changed_ticket_never_consumes_another_payload_or_changes_counters() {
    let mut engine = Engine::from_json(include_str!("fixtures/m7-tiny.json")).unwrap();
    let mut ids = [0u8; 2];
    unsafe {
        assert_eq!(
            hng_engine_offer(&mut engine, 0, ids.as_mut_ptr().cast(), 60, 0, 0),
            PushResult::Accepted
        );
        let lines = [hng_engine_line(&mut engine, 0)];
        let mut head = HngHead::default();
        assert_eq!(
            hng_lines_peek_head(
                lines.as_ptr(),
                &1_000_000,
                1,
                1_000_000,
                u64::MAX,
                &mut head
            ),
            0
        );
        let mut stale = head;
        let mut wrong = head;
        wrong.packet_bytes += 1;
        let before = format!("{:?}", stats(&engine, 0));
        assert!(!hng_head_pop(&mut wrong));
        assert_eq!(format!("{:?}", stats(&engine, 0)), before);
        assert!(hng_head_pop(&mut head));
        assert_eq!(
            hng_engine_offer(
                &mut engine,
                0,
                ids.as_mut_ptr().add(1).cast(),
                60,
                1_000_000,
                1
            ),
            PushResult::Accepted
        );
        let before = format!("{:?}", stats(&engine, 0));
        assert!(!hng_head_pop(&mut stale));
        assert_eq!(format!("{:?}", stats(&engine, 0)), before);
        assert_eq!(
            hng_lines_peek_head(
                lines.as_ptr(),
                &2_000_000,
                1,
                2_000_000,
                u64::MAX,
                &mut head
            ),
            0
        );
        assert_eq!(head.ingress_seq, 1);
        assert!(hng_head_pop(&mut head));
        assert!(!hng_head_pop(ptr::null_mut()));
    }
}
