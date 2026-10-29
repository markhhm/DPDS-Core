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
fn terminal_hold_covers_actual_line_delay_and_queue_wire_drain() {
    let path = std::env::temp_dir().join(format!("hng-hold-drain-{}.json", std::process::id()));
    let mut trace: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/trace.json")).unwrap();
    let mut config: serde_json::Value = serde_json::from_str(&json()).unwrap();
    config["lines"].as_array_mut().unwrap().truncate(1);
    // 80/160 L2 bytes plus 20 wire bytes per frame: 100/200 ns at 8 Gbit/s.
    for (q, rate, required_drain) in [
        (80, 8_000_000_000u64, 100),
        (160, 8_000_000_000, 200),
        (80, 3_000_000_000, 267),
    ] {
        config["lines"][0]["model"]["q_bytes"] = q.into();
        config["lines"][0]["model"]["c_bps"] = rate.into();
        for (allowance, accepted) in [
            (0, false),
            (required_drain - 1, false),
            (required_drain, true),
        ] {
            trace["profile"]["terminal_horizon_ns"] = (1000 + 110 + allowance).into();
            std::fs::write(&path, trace.to_string()).unwrap();
            let digest = std::process::Command::new("sha256sum")
                .arg(&path)
                .output()
                .unwrap();
            assert!(digest.status.success());
            let digest = String::from_utf8(digest.stdout).unwrap();
            config["lines"][0]["trace"] =
                serde_json::json!({"path":path,"sha256":digest.split_whitespace().next().unwrap()});
            let loaded = Engine::from_json(&config.to_string());
            assert_eq!(
                loaded.is_ok(),
                accepted,
                "q={q}, C={rate}, queue allowance={allowance}"
            );
            if !accepted {
                assert!(loaded.err().unwrap().contains("HOLD"));
            }
        }
    }
    std::fs::remove_file(path).unwrap();
}
