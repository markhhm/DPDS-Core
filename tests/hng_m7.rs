// SPDX-License-Identifier: Apache-2.0
use dpds_core::hng_engine::Engine;

#[test]
fn explicit_m7_registration_loads_with_bounded_storage() {
    // The old loader rejects m7_storage as an unknown field. 1Mbit/s and
    // zero propagation keep this software fixture small; H remains 10ms.
    let config = include_str!("fixtures/m7-tiny.json");
    let engine = Engine::from_json(config);
    assert!(engine.is_ok(), "M7 opt-in should load: {:?}", engine.err());
}

#[test]
fn registered_90g_x_quota_matches_the_independent_2c_accounting() {
    let registration: dpds_core::hng_storage::M7StorageRegistration = serde_json::from_str(
        r#"{"mode":"m7-hybrid-v1","composition":[{"l2_bytes":64,"weight":1},
        {"l2_bytes":512,"weight":1},{"l2_bytes":1522,"weight":1}]}"#,
    )
    .unwrap();
    let quota = registration
        .plan(90_000_000_000, 83_886_086, 462_000_000, 16)
        .unwrap();
    // Literals independently verified in 2c; no giant allocation here.
    assert_eq!(quota.slots, 9_377_250);
    assert_eq!(quota.records, 6_251_505);
    assert_eq!(quota.cells, 28_131_828);
    assert_eq!(quota.normal_mbufs, 3_125_761);
}

#[test]
fn only_registered_composition_can_use_the_smaller_core_reservation() {
    let config = r#"{"schema":"hng-v1","lines":[{"name":"test","ingress_port":0,
      "egress_port":1,"vlan_id":100,"model":{"c_bps":1000000,"q_bytes":64,
      "t_ns":100000000,"delta_ns":0,"p":"1","seed":1,"storage_capacity_packets":41,
      "storage_capacity_bytes":62238},"envelope":{"min_q_bytes":64,
      "max_q_bytes":1522,"burst_bound_packets":16},"m7_storage":{
      "mode":"m7-hybrid-v1","composition":[{"l2_bytes":64,"weight":1},
      {"l2_bytes":512,"weight":1},{"l2_bytes":1522,"weight":1}]}}]}"#;
    assert!(
        Engine::from_json(config).is_ok(),
        "registered 41-slot plan should load"
    );
    let mut legacy: serde_json::Value = serde_json::from_str(config).unwrap();
    legacy["lines"][0]
        .as_object_mut()
        .unwrap()
        .remove("m7_storage");
    assert!(
        Engine::from_json(&legacy.to_string()).is_err(),
        "legacy all-small gate must remain"
    );
}

#[test]
fn m7_rejects_unregistered_lengths_before_selecting_a_packet() {
    use dpds_core::hng::PushResult;
    use dpds_core::hng_engine::hng_engine_offer;
    let config = include_str!("fixtures/m7-tiny.json");
    let mut engine = Engine::from_json(config).unwrap();
    let mut token = 0u8;
    let result =
        unsafe { hng_engine_offer(&mut engine, 0, (&mut token as *mut u8).cast(), 80, 0, 0) };
    assert_eq!(result, PushResult::InvalidInput);
    assert_eq!(engine.lines[0].next_selection_seq(), 0);
}

#[test]
fn window_occupancy_limit_fails_honestly_before_the_physical_slot_reserve() {
    use dpds_core::hng::PushResult;
    use dpds_core::hng_engine::hng_engine_offer;
    // At U: 1.25x requires four physical slots, W allows three live slots.
    let config = r#"{"schema":"hng-v1","lines":[{"name":"test","ingress_port":0,
      "egress_port":1,"vlan_id":100,"model":{"c_bps":1000000,"q_bytes":1522,
      "t_ns":0,"delta_ns":0,"p":"1","seed":1,"storage_capacity_packets":4,
      "storage_capacity_bytes":6072},"envelope":{"min_q_bytes":1522,
      "max_q_bytes":1522,"burst_bound_packets":1},"m7_storage":{
      "mode":"m7-hybrid-v1","composition":[{"l2_bytes":1522,"weight":1}]}}]}"#;
    let mut engine = Engine::from_json(config).unwrap();
    let mut tokens = [0u8; 4];
    for (i, token) in tokens.iter_mut().enumerate() {
        let result = unsafe {
            hng_engine_offer(
                &mut engine,
                0,
                (token as *mut u8).cast(),
                1518,
                i as u64 * 20_000_000,
                i as u64,
            )
        };
        assert_eq!(
            result,
            if i < 3 {
                PushResult::Accepted
            } else {
                PushResult::StorageDrop
            }
        );
    }
}

#[test]
fn c_plan_interface_reports_quotas_without_allocating_an_engine() {
    use dpds_core::hng_storage::{hng_m7_storage_plan, M7StorageInfo};
    use std::ffi::CString;
    let input=CString::new(r#"{"mode":"m7-hybrid-v1","composition":[{"l2_bytes":64,"weight":1},{"l2_bytes":512,"weight":1},{"l2_bytes":1522,"weight":1}]}"#).unwrap();
    let mut out = M7StorageInfo::default();
    assert!(unsafe {
        hng_m7_storage_plan(
            input.as_ptr(),
            90_000_000_000,
            83_886_086,
            462_000_000,
            16,
            &mut out,
        )
    });
    assert_eq!(
        (
            out.slots,
            out.record_limit,
            out.cell_limit,
            out.normal_limit
        ),
        (9_377_250, 6_251_505, 24_814_737, 2_538_883)
    );
    assert_eq!(&out.lengths[..3], &[64, 512, 1522]);
    out.slots = 12345;
    assert!(!unsafe {
        hng_m7_storage_plan(
            input.as_ptr(),
            90_000_000_000,
            83_886_086,
            u64::MAX,
            16,
            &mut out,
        )
    });
    assert_eq!(out.slots, 12345, "failed plan must leave output unchanged");
}

#[test]
fn engine_getter_keeps_m7_resources_absent_on_legacy_configs() {
    use dpds_core::hng_engine::hng_engine_m7_storage_info;
    use dpds_core::hng_storage::M7StorageInfo;
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/m7-tiny.json")).unwrap();
    let m7 = Engine::from_json(&config.to_string()).unwrap();
    let mut out = M7StorageInfo::default();
    assert!(unsafe { hng_engine_m7_storage_info(&m7, 0, &mut out) });
    assert_eq!(
        (out.threshold_l2, out.hold_ns, out.slots),
        (1024, 10_000_000, 19)
    );
    config["lines"][0]
        .as_object_mut()
        .unwrap()
        .remove("m7_storage");
    let legacy = Engine::from_json(&config.to_string()).unwrap();
    out.slots = 12345;
    assert!(!unsafe { hng_engine_m7_storage_info(&legacy, 0, &mut out) });
    assert_eq!(out.slots, 12345);
}

#[test]
fn accepted_storage_commit_is_transactional_and_never_runs_for_q_drop() {
    use dpds_core::hng::{HngBuffer, HngConfig, HngPacket, PushResult};
    use std::ffi::c_void;
    struct Store {
        calls: u64,
        fail: bool,
        replacement: *mut c_void,
        service_ns: u64,
    }
    unsafe extern "C" fn commit(record: *const HngPacket, context: *mut c_void) -> *mut c_void {
        let state = &mut *context.cast::<Store>();
        state.calls += 1;
        state.service_ns = (*record).service_ns;
        if state.fail {
            std::ptr::null_mut()
        } else {
            state.replacement
        }
    }
    let file: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/m7-tiny.json")).unwrap();
    let config: HngConfig = serde_json::from_value(file["lines"][0]["model"].clone()).unwrap();
    let mut buffer = HngBuffer::new(config).unwrap();
    let mut source = 0u8;
    let mut stored = 0u8;
    let pointer = (&mut source as *mut u8).cast();
    let replacement = (&mut stored as *mut u8).cast();
    let mut state = Store {
        calls: 0,
        fail: true,
        replacement,
        service_ns: 0,
    };
    let context = (&mut state as *mut Store).cast();
    let first = unsafe { buffer.push_with_storage(pointer, 60, 0, 0, None, Some(commit), context) };
    assert_eq!(first, PushResult::StorageDrop);
    assert_eq!(
        buffer.stats().q_bytes,
        0,
        "failed storage cannot commit service/Q"
    );
    state.fail = false;
    let second =
        unsafe { buffer.push_with_storage(pointer, 60, 0, 1, None, Some(commit), context) };
    assert_eq!(second, PushResult::Accepted);
    assert_eq!(
        state.service_ns, 672000,
        "failed commit cannot add serialization time"
    );
    let third = unsafe { buffer.push_with_storage(pointer, 60, 0, 2, None, Some(commit), context) };
    assert_eq!(third, PushResult::QDrop);
    assert_eq!(state.calls, 2, "model drops do not consume storage");
    assert_eq!(buffer.stats().storage_drops, 1);
    assert_eq!(buffer.stats().q_drops, 1);
    assert_eq!(buffer.pop().unwrap().packet_ptr, replacement);
}

#[test]
fn burst_storage_callback_applies_only_to_explicit_m7_lines() {
    use dpds_core::hng::{HngPacket, PushResult};
    use dpds_core::hng_engine::{hng_engine_line, hng_lines_offer_burst_storage};
    use std::ffi::c_void;
    unsafe extern "C" fn commit(record: *const HngPacket, ctx: *mut c_void) -> *mut c_void {
        *ctx.cast::<u64>() += 1;
        (*record).packet_ptr
    }
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/m7-tiny.json")).unwrap();
    let mut classic = config["lines"][0].clone();
    classic["name"] = "classic".into();
    classic["vlan_id"] = 101.into();
    classic.as_object_mut().unwrap().remove("m7_storage");
    config["lines"].as_array_mut().unwrap().push(classic);
    let mut engine = Engine::from_json(&config.to_string()).unwrap();
    let handles = unsafe {
        [
            hng_engine_line(&mut engine, 0),
            hng_engine_line(&mut engine, 1),
        ]
    };
    let mut token = [0u8; 2];
    let packets = [
        (&mut token[0] as *mut u8).cast(),
        (&mut token[1] as *mut u8).cast(),
    ];
    let mut calls = 0u64;
    let mut results = [PushResult::InvalidInput; 2];
    let mut records: [HngPacket; 2] = unsafe { std::mem::zeroed() };
    unsafe {
        hng_lines_offer_burst_storage(
            handles.as_ptr(),
            [0u16, 1].as_ptr(),
            packets.as_ptr(),
            [60u32; 2].as_ptr(),
            [0u64; 2].as_ptr(),
            [0u64; 2].as_ptr(),
            2,
            results.as_mut_ptr(),
            records.as_mut_ptr(),
            Some(commit),
            (&mut calls as *mut u64).cast(),
        )
    };
    assert_eq!(results, [PushResult::Accepted; 2]);
    assert_eq!(
        calls, 1,
        "classic line must never enter the storage callback"
    );
}

#[test]
fn merged_peek_preserves_the_head_until_materialization_can_commit() {
    use dpds_core::hng::{HngPacket, PushResult};
    use dpds_core::hng_engine::{
        hng_engine_len, hng_engine_line, hng_engine_offer, hng_line_end_input,
        hng_lines_peek_merged, hng_lines_pop_merged,
    };
    let mut engine = Engine::from_json(include_str!("fixtures/m7-tiny.json")).unwrap();
    let mut token = 0u8;
    assert_eq!(
        unsafe { hng_engine_offer(&mut engine, 0, (&mut token as *mut u8).cast(), 60, 0, 0) },
        PushResult::Accepted
    );
    let handles = [unsafe { hng_engine_line(&mut engine, 0) }];
    unsafe {
        hng_line_end_input(handles[0]);
    }
    let watermark = [u64::MAX];
    let mut peek: HngPacket = unsafe { std::mem::zeroed() };
    for _ in 0..2 {
        assert_eq!(
            unsafe {
                hng_lines_peek_merged(
                    handles.as_ptr(),
                    watermark.as_ptr(),
                    1,
                    1_000_000,
                    u64::MAX,
                    &mut peek,
                )
            },
            0
        );
        assert_eq!(
            unsafe { hng_engine_len(&engine, 0) },
            1,
            "peek cannot relinquish model custody"
        );
        assert_eq!(
            (peek.ingress_seq, peek.selection_seq, peek.deadline_ns),
            (0, 0, 672000)
        );
    }
    let mut pop = peek;
    assert_eq!(
        unsafe {
            hng_lines_pop_merged(
                handles.as_ptr(),
                watermark.as_ptr(),
                1,
                1_000_000,
                u64::MAX,
                &mut pop,
            )
        },
        0
    );
    assert_eq!(
        (pop.packet_ptr, pop.release_ns, pop.late),
        (peek.packet_ptr, peek.release_ns, peek.late)
    );
    assert_eq!(unsafe { hng_engine_len(&engine, 0) }, 0);
}

#[test]
fn explicit_null_is_not_a_silent_opt_out_from_new_configuration() {
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/m7-tiny.json")).unwrap();
    config["lines"][0]["m7_storage"] = serde_json::Value::Null;
    assert!(
        Engine::from_json(&config.to_string()).is_err(),
        "absent field is legacy; explicit null is invalid"
    );
}
