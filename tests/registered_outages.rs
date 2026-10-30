// SPDX-License-Identifier: Apache-2.0
use dpds_core::hng::{HngBuffer, HngConfig, Probability, PushResult, TraceProfile};
use dpds_core::hng_engine::Engine;
use std::{ffi::c_void, path::Path, process::Command};

#[test]
fn registered_synthetic_outages_match_deadline_and_gate_oracles() {
    for action in ["drop", "hold"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/outage-{action}.json"));
        let text =
            std::fs::read_to_string(&path).expect("registered synthetic outage profile missing");
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let source_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/outage-source.csv");
        let source_digest = Command::new("sha256sum").arg(source_path).output().unwrap();
        assert!(source_digest.status.success());
        let source_digest = String::from_utf8(source_digest.stdout).unwrap();
        assert_eq!(
            doc["source_sha256"].as_str(),
            source_digest.split_whitespace().next()
        );
        let trace: TraceProfile = serde_json::from_value(doc["profile"].clone()).unwrap();
        let model = HngConfig {
            c_bps: 800_000,
            q_bytes: 80,
            t_ns: 0,
            delta_ns: 0,
            p: Probability::ZERO,
            seed: 1,
            storage_capacity_packets: 512,
            storage_capacity_bytes: 1_000_000,
        };
        let digest = Command::new("sha256sum").arg(&path).output().unwrap();
        assert!(digest.status.success());
        let digest = String::from_utf8(digest.stdout).unwrap();
        let configuration = serde_json::json!({"schema":"hng-v1","lines":[{
            "name":"synthetic-outage", "ingress_port":0, "egress_port":1, "vlan_id":200,
            "model": {"c_bps":800000,"q_bytes":80,"t_ns":0,"delta_ns":0,"p":"0","seed":1,
                      "storage_capacity_packets":512,"storage_capacity_bytes":1000000},
            "envelope":{"min_q_bytes":80,"max_q_bytes":80,"burst_bound_packets":1},
            "trace":{"path":path,"sha256":digest.split_whitespace().next().unwrap()}
        }]});
        assert!(Engine::from_json(&configuration.to_string()).is_ok());
        let mut buffer = HngBuffer::with_trace(model, trace).unwrap();
        let policy = buffer.tx_policy();
        let down_gate = if action == "drop" { 2 } else { 1 };
        assert_eq!(policy.gate(199_999_999), 0);
        assert_eq!(policy.gate(200_000_000), down_gate);
        assert_eq!(policy.gate(299_999_999), down_gate);
        assert_eq!(policy.gate(300_000_000), 0);
        assert_eq!(policy.gate(1_059_999_999), 0);
        assert_eq!(policy.gate(1_060_000_000), 3);
        for (index, arrival) in [189_000_000, 199_000_000, 299_000_000]
            .into_iter()
            .enumerate()
        {
            let result = buffer.push((index + 1) as *mut c_void, 76, arrival);
            assert_eq!(
                result,
                if action == "drop" && index < 2 {
                    PushResult::TraceOutage
                } else {
                    PushResult::Accepted
                }
            );
        }
        assert!(buffer.try_pop(299_999_999, u64::MAX).is_none());
        let ids: &[usize] = if action == "hold" { &[1, 2, 3] } else { &[3] };
        for &id in ids {
            let packet = buffer.try_pop(310_000_000, u64::MAX).unwrap();
            let deadline = [200_000_000, 210_000_000, 310_000_000][id - 1];
            let release = if id < 3 { 300_000_000 } else { deadline };
            assert_eq!(packet.packet_ptr as usize, id);
            assert_eq!(packet.deadline_ns, deadline);
            assert_eq!(packet.release_ns, release);
        }
        assert!(buffer.try_pop(310_000_000, u64::MAX).is_none());
        assert_eq!(
            buffer.stats().trace_outage_drops,
            if action == "drop" { 2 } else { 0 }
        );
        assert_eq!(buffer.stats().q_drops, 0);
        assert_eq!(buffer.stats().storage_drops, 0);
    }
}

#[test]
fn dropped_packet_does_not_create_phantom_deadline_plateau() {
    let trace: TraceProfile = serde_json::from_value(serde_json::json!({
        "end_ns":10000, "terminal":"error", "terminal_horizon_ns":null,
        "loss_seed":0, "outage_action":"drop", "rows":[
            {"at_ns":0, "t_ns":200, "loss":"0", "outage":false},
            {"at_ns":200, "t_ns":1000, "loss":"0", "outage":false},
            {"at_ns":300, "t_ns":10, "loss":"0", "outage":false},
            {"at_ns":1000, "t_ns":10, "loss":"0", "outage":true},
            {"at_ns":2000, "t_ns":10, "loss":"0", "outage":false}
        ]
    }))
    .unwrap();
    let model = HngConfig {
        c_bps: 8_000_000_000,
        q_bytes: 80,
        t_ns: 0,
        delta_ns: 0,
        p: Probability::ZERO,
        seed: 1,
        storage_capacity_packets: 32,
        storage_capacity_bytes: 10000,
    };
    let mut buffer = HngBuffer::with_trace(model, trace).unwrap();
    // Services 100/200/300 ns give raw deadlines 300/1200/310 ns.
    for (id, arrival, expected) in [
        (1, 0, PushResult::Accepted),
        (2, 100, PushResult::TraceOutage),
        (3, 200, PushResult::Accepted),
    ] {
        assert_eq!(buffer.push(id as *mut c_void, 76, arrival), expected);
    }
    assert!(buffer.try_pop(299, u64::MAX).is_none());
    for (id, deadline) in [(1, 300), (3, 310)] {
        let packet = buffer.try_pop(deadline, u64::MAX).unwrap();
        assert_eq!(packet.packet_ptr as usize, id);
        assert_eq!(packet.deadline_ns, deadline);
        assert_eq!(packet.clamp_extra_ns, 0);
    }
    assert_eq!(buffer.stats().trace_outage_drops, 1);
    assert_eq!(buffer.stats().clamp_packets, 0);
    assert!(buffer.try_pop(1200, u64::MAX).is_none());
}
