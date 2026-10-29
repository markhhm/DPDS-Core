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
