// SPDX-License-Identifier: Apache-2.0
//! Offline scheduler records over JSON stdin/stdout; no packet I/O or wire clock.
use dpds_core::hng::{HngBuffer, HngConfig, PushResult, TraceProfile};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, error::Error, ffi::c_void, io::Read};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    model: HngConfig,
    trace: Option<TraceProfile>,
    packets: Vec<Packet>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    id: usize,
    arrival_ns: u64,
    length: u32,
}

#[derive(Serialize)]
struct Record {
    id: usize,
    ingress_seq: u64,
    service_ns: u64,
    deadline_ns: u64,
    release_ns: u64,
    clamp_extra_ns: u64,
    late: bool,
}

fn drain(buffer: &mut HngBuffer, now: u64, records: &mut Vec<Record>) {
    while let Some(packet) = buffer.try_pop(now, now) {
        records.push(Record {
            id: packet.packet_ptr as usize,
            ingress_seq: packet.ingress_seq,
            service_ns: packet.service_ns,
            deadline_ns: packet.deadline_ns,
            release_ns: packet.release_ns,
            clamp_extra_ns: packet.clamp_extra_ns,
            late: packet.late,
        });
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    std::io::stdin()
        .take(128 * 1024 * 1024 + 1)
        .read_to_string(&mut text)?;
    if text.len() > 128 * 1024 * 1024 {
        return Err("replay input exceeds 128 MiB".into());
    }
    let input: Input = serde_json::from_str(&text)?;
    let mut buffer = match input.trace {
        Some(trace) => HngBuffer::with_trace(input.model, trace)?,
        None => HngBuffer::new(input.model)?,
    };
    let mut identities = HashSet::new();
    let mut records = Vec::new();
    for packet in input.packets {
        if packet.id == 0 || !identities.insert(packet.id) {
            return Err("packet identities must be unique and nonzero".into());
        }
        let result = buffer.push(packet.id as *mut c_void, packet.length, packet.arrival_ns);
        if result != PushResult::Accepted {
            return Err(format!("replay packet {} was not accepted: {result:?}", packet.id).into());
        }
        drain(&mut buffer, packet.arrival_ns, &mut records);
    }
    buffer.end_input();
    while let Some(packet) = buffer.front() {
        let due = packet.release_ns;
        drain(&mut buffer, due, &mut records);
    }
    println!("{}", serde_json::to_string(&records)?);
    Ok(())
}
