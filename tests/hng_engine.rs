// SPDX-License-Identifier: Apache-2.0
use dpds_core::hng_engine::Engine;

fn json() -> String {
    r#"{"schema":"hng-v1","lines":[
      {"name":"cable","ingress_port":0,"egress_port":1,"vlan_id":100,
       "model":{"c_bps":8000000000,"q_bytes":80,"t_ns":0,"delta_ns":0,"p":"0","seed":1,
         "storage_capacity_packets":32,"storage_capacity_bytes":100000},
       "envelope":{"min_q_bytes":80,"max_q_bytes":80,"burst_bound_packets":1},"trace":null},
      {"name":"leo","ingress_port":0,"egress_port":1,"vlan_id":200,
       "model":{"c_bps":8000000000,"q_bytes":80,"t_ns":0,"delta_ns":0,"p":"0","seed":2,
         "storage_capacity_packets":32,"storage_capacity_bytes":100000},
       "envelope":{"min_q_bytes":80,"max_q_bytes":80,"burst_bound_packets":1},"trace":null}
    ]}"#
    .into()
}

#[test]
fn vlan_lines_have_independent_shared_bottlenecks() {
    let e = Engine::from_json(&json()).unwrap();
    assert_eq!(e.lines.len(), 2);
    assert_eq!(e.lines[0].info.vlan_id, 100);
    assert_eq!(e.lines[1].info.vlan_id, 200);
}

#[test]
fn unknown_mixed_duplicate_and_undersized_configs_fail_before_rx() {
    let j = json();
    for bad in [
        j.replace("hng-v1", "legacy"),
        j.replace("\"q_bytes\":80", "\"q_bytes\":80,\"limit_bit\":800"),
        j.replace("\"vlan_id\":200", "\"vlan_id\":100"),
        j.replace(
            "\"storage_capacity_packets\":32",
            "\"storage_capacity_packets\":1",
        ),
        j.replace(
            "\"storage_capacity_bytes\":100000",
            "\"storage_capacity_bytes\":10",
        ),
        j.replace("\"p\":\"0\"", "\"p\":0.1"),
        j.replace("\"vlan_id\":200", "\"vlan_id\":4095"),
        j.replace("\"vlan_id\":200", "\"vlan_id\":0"),
        j.replace(
            "\"schema\":\"hng-v1\"",
            "\"schema\":\"hng-v1\",\"reordering\":true",
        ),
    ] {
        assert!(
            Engine::from_json(&bad).is_err(),
            "accepted invalid config: {bad}"
        );
    }
}

#[test]
fn ffi_rejects_outside_frame_envelope_and_preserves_pointer_custody() {
    use dpds_core::hng::{HngPacket, PushResult};
    use dpds_core::hng_engine::*;
    use std::mem::MaybeUninit;
    unsafe {
        let engine = Box::into_raw(Box::new(Engine::from_json(&json()).unwrap()));
        let mut payload = 0u8;
        let ptr = std::ptr::from_mut(&mut payload).cast::<std::ffi::c_void>();
        assert_eq!(
            hng_engine_offer(engine, 0, ptr, 77, 0, 0),
            PushResult::InvalidInput
        );
        assert_eq!(
            hng_engine_offer(engine, 0, ptr, 76, 0, 1),
            PushResult::Accepted
        );
        assert!(!hng_engine_destroy(engine));
        let mut out = MaybeUninit::<HngPacket>::uninit();
        assert!(!hng_engine_pop(engine, 0, 99, u64::MAX, out.as_mut_ptr()));
        assert!(!hng_engine_pop(
            engine,
            0,
            100,
            u64::MAX,
            std::ptr::null_mut()
        ));
        assert!(hng_engine_pop(engine, 0, 100, u64::MAX, out.as_mut_ptr()));
        assert_eq!(out.assume_init().packet_ptr, ptr);
        assert!(!hng_engine_pop(engine, 0, 100, u64::MAX, out.as_mut_ptr()));
        assert!(!hng_engine_destroy(engine));
        assert!(hng_engine_has_backlog(engine));
        assert!(!hng_engine_drain_advance(engine, 100));
        hng_engine_end_input(engine);
        assert!(hng_engine_drain_advance(engine, 99));
        assert!(hng_engine_has_backlog(engine));
        assert!(hng_engine_drain_advance(engine, 100));
        assert!(!hng_engine_has_backlog(engine));
        assert!(hng_engine_destroy(engine));
        assert!(hng_engine_destroy(std::ptr::null_mut()));
    }
}

#[test]
fn trace_file_is_hashed_validated_and_loaded_before_rx() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/trace.json");
    let descriptor = format!(
        r#"{{"path":"{path}","sha256":"a1adf11d728051e51639b1f5dfee53aee3642ee8056beecbeeca687c00aace49"}}"#
    );
    let good = json().replace("\"trace\":null", &format!("\"trace\":{descriptor}"));
    assert!(Engine::from_json(&good).is_ok());
    assert!(Engine::from_json(&good.replace(
        "a1adf11d728051e51639b1f5dfee53aee3642ee8056beecbeeca687c00aace49",
        &"0".repeat(64)
    ))
    .is_err());
}

#[test]
fn documented_hardware_free_four_state_example_validates() {
    assert_eq!(
        Engine::from_json(include_str!("../hng.config.example.json"))
            .unwrap()
            .lines
            .len(),
        4
    );
}

#[test]
fn metadata_reservation_is_exact_per_storage_slot() {
    use dpds_core::hng_engine::{hng_engine_metadata_bytes, hng_slot_metadata_bytes};
    let e = Box::into_raw(Box::new(Engine::from_json(&json()).unwrap()));
    let slot = hng_slot_metadata_bytes();
    // Two packet FIFOs and the service ledger, each reserved at the slot capacity.
    assert!(slot >= 2 * std::mem::size_of::<dpds_core::hng::HngPacket>());
    assert_eq!(unsafe { hng_engine_metadata_bytes(e) }, 2 * 32 * slot);
    assert_eq!(unsafe { hng_engine_metadata_bytes(std::ptr::null()) }, 0);
    drop(unsafe { Box::from_raw(e) });
}

fn four_lines() -> String {
    json()
        .replace("\"vlan_id\":200", "\"vlan_id\":101")
        .replace("]}", ",")
        + json()
            .replace("\"name\":\"cable\"", "\"name\":\"cable-r\"")
            .replace("\"name\":\"leo\"", "\"name\":\"leo-r\"")
            .replace("\"vlan_id\":200", "\"vlan_id\":101")
            .replace(
                "\"ingress_port\":0,\"egress_port\":1",
                "\"ingress_port\":1,\"egress_port\":0",
            )
            .split_once("\"lines\":[")
            .unwrap()
            .1
}

/// Line handles give each owner thread exclusive access to its own lines.
#[test]
fn line_handles_match_engine_calls_and_run_concurrently() {
    use dpds_core::hng::{HngPacket, PushResult};
    use dpds_core::hng_engine::*;
    let run = |split: bool| -> Vec<Vec<(u64, u64)>> {
        let e = Box::into_raw(Box::new(Engine::from_json(&four_lines()).unwrap()));
        assert_eq!(unsafe { hng_engine_line_count(e) }, 4);
        assert!(unsafe { hng_engine_line(e, 4) }.is_null());
        let lines: Vec<usize> = (0..4)
            .map(|i| unsafe { hng_engine_line(e, i) } as usize)
            .collect();
        let eu = e as usize;
        let work = |owned: Vec<usize>| -> Vec<Vec<(u64, u64)>> {
            let mut out = vec![Vec::new(); owned.len()];
            for seq in 0..200u64 {
                for (k, &i) in owned.iter().enumerate() {
                    let now = seq * 10;
                    let ptr = (seq + 1) as usize as *mut std::ffi::c_void;
                    let r = if split {
                        unsafe { hng_line_offer(lines[i] as *mut Line, ptr, 76, now, seq) }
                    } else {
                        unsafe { hng_engine_offer(eu as *mut Engine, i, ptr, 76, now, seq) }
                    };
                    assert!(matches!(r, PushResult::Accepted | PushResult::QDrop));
                    let mut p = std::mem::MaybeUninit::<HngPacket>::uninit();
                    while unsafe {
                        if split {
                            hng_line_pop(lines[i] as *mut Line, now, now, p.as_mut_ptr())
                        } else {
                            hng_engine_pop(eu as *mut Engine, i, now, now, p.as_mut_ptr())
                        }
                    } {
                        let p = unsafe { p.assume_init() };
                        out[k].push((p.ingress_seq, p.release_ns));
                    }
                }
            }
            out
        };
        let result = if split {
            std::thread::scope(|scope| {
                let a = scope.spawn(|| work(vec![0, 1]));
                let b = scope.spawn(|| work(vec![2, 3]));
                let mut r = a.join().unwrap();
                r.extend(b.join().unwrap());
                r
            })
        } else {
            work(vec![0, 1, 2, 3])
        };
        for &handle in &lines {
            let l = handle as *mut Line;
            unsafe { hng_line_end_input(l) };
            assert!(unsafe { hng_line_drain_advance(l, u64::MAX / 2) });
            let mut p = std::mem::MaybeUninit::<HngPacket>::uninit();
            while unsafe { hng_line_abort_pop(l, p.as_mut_ptr()) } {}
            assert!(!unsafe { hng_line_has_backlog(l) });
            assert_eq!(unsafe { hng_line_min_delay(l) }, 0);
            let mut s = std::mem::MaybeUninit::uninit();
            assert!(unsafe { hng_line_stats(l, s.as_mut_ptr()) });
            assert!(!unsafe { hng_line_peek(l, 0, 0, std::ptr::null_mut()) });
        }
        assert!(unsafe { hng_engine_destroy(e) });
        result
    };
    let single = run(false);
    assert!(single.iter().all(|v| !v.is_empty()));
    assert_eq!(single, run(true));
}
