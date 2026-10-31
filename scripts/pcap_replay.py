#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""JSON stdin/stdout runner using optional DPDK net_pcap and the current core FFI.

Set HNG_PCAP_BINARY to the compiled pcap_trace_replay executable. Only synthetic
76-byte frames are transported; source timing and identity are encoded in payload.
Pcap capture timestamps are never used for model arrivals or wire-time claims.
"""
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

from compare_trace import strict_json


def frame(packet):
    data = bytearray(76)
    data[:6] = b'\xff' * 6
    data[6:12] = b'\x02\x00\x00\x00\x00\x01'
    data[12:18] = struct.pack('!HHH', 0x8100, 100, 0x88b5)
    data[18:34] = struct.pack('<QQ', packet['id'], packet['arrival_ns'])
    return bytes(data)


def pcap_frames(path):
    with path.open('rb') as source:
        header = source.read(24)
        if len(header) != 24 or header[:4] not in (b'\xd4\xc3\xb2\xa1', b'\x4d\x3c\xb2\xa1'):
            raise ValueError('unsupported output pcap header')
        while row := source.read(16):
            if len(row) != 16:
                raise ValueError('truncated pcap record')
            _, _, length, original = struct.unpack('<IIII', row)
            data = source.read(length)
            if length != 76 or original != length or len(data) != length:
                raise ValueError('pcap frame coverage mismatch')
            yield data


def run(request, directory):
    packets = request['packets']
    if not 0 < len(packets) <= 1_000_000 or any(
            p['length'] != 76 or p['id'] != i+1 or
            not 0 <= p['arrival_ns'] < 2**64 or
            (i and p['arrival_ns'] <= packets[i-1]['arrival_ns']) for i, p in enumerate(packets)):
        raise ValueError('pcap replay requires sequential identities, increasing arrivals and 76-byte frames')
    trace = request['trace']
    if trace is None:
        trace_config = None
    else:
        trace_path = directory/'trace.json'
        profile_hash = hashlib.sha256(json.dumps(trace).encode()).hexdigest()
        payload = json.dumps(dict(schema='hng-trace-v1', source_id='caller-profile-json', direction='ul',
            units='ns_owd', conversion='adapter wraps supplied profile; source hash identifies profile JSON', source_sha256=profile_hash,
            splice_policy='natural_hold', trace_epoch_ns=0, time_scale=1, profile=trace)).encode()
        trace_path.write_bytes(payload)
        trace_config = dict(path=str(trace_path), sha256=hashlib.sha256(payload).hexdigest())
    model = dict(request['model'])
    delay = max(row['t_ns'] for row in trace['rows']) if trace else model['t_ns']
    numerator = model['c_bps'] * (delay + model['delta_ns'])
    denominator = 8_000_000_000 * 100
    capacity = max(model['storage_capacity_packets'], model['q_bytes']//80 + 1 +
                   (numerator + denominator - 1)//denominator)
    model['storage_capacity_packets'] = capacity
    model['storage_capacity_bytes'] = max(model['storage_capacity_bytes'], capacity*80)
    config = dict(schema='hng-v1', lines=[dict(name='replay', ingress_port=0, egress_port=0,
        vlan_id=100, model=model, envelope=dict(min_q_bytes=80, max_q_bytes=80,
        burst_bound_packets=1), trace=trace_config)])
    (directory/'config.json').write_text(json.dumps(config))
    with (directory/'input.pcap').open('xb') as output:
        output.write(struct.pack('<IHHIIII', 0xa1b2c3d4, 2, 4, 0, 0, 65535, 1))
        for packet in packets:
            data = frame(packet)
            output.write(struct.pack('<IIII', 123, 0, len(data), len(data)))
            output.write(data)
    binary = Path(os.environ['HNG_PCAP_BINARY']).resolve()
    cpu = min(os.sched_getaffinity(0))
    # Default mbufs occupy separate base pages in no-huge IOVA-contiguous pools.
    memory_mb = (len(packets)+1024)*max(4096, os.sysconf('SC_PAGESIZE'))//(1024**2) + 128
    command = [str(binary), f'--lcores=0@{cpu}', '--no-pci', '--no-huge', '--no-telemetry', '-m', str(memory_mb),
               '--no-shconf', f'--file-prefix=pcap-{os.getpid()}',
               f'--vdev=net_pcap0,rx_pcap={directory}/input.pcap,tx_pcap={directory}/output.pcap']
    env = dict(os.environ, PCAP_CONFIG=str(directory/'config.json'),
               PCAP_COUNT=str(len(packets)), PCAP_RECORDS=str(directory/'records.json'))
    with tempfile.TemporaryDirectory(prefix='hng-eal-') as runtime:
        env['RUNTIME_DIRECTORY'] = runtime
        result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=120)
    (directory/'pmd.log').write_text(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError(f'pcap adapter failed ({result.returncode}): {result.stdout}{result.stderr}')
    records = strict_json((directory/'records.json').read_bytes())
    if len(records) != len(packets):
        raise ValueError('engine record coverage mismatch')
    output = list(pcap_frames(directory/'output.pcap'))
    expected = [frame(packets[event['id']-1]) for event in records]
    if output != expected:
        raise ValueError('PMD egress identity, bytes or order mismatch')
    return records


def main():
    payload = sys.stdin.buffer.read(128*1024**2+1)
    if len(payload) > 128*1024**2:
        raise ValueError('replay input exceeds 128 MiB')
    request = strict_json(payload)
    evidence = os.environ.get('HNG_PCAP_EVIDENCE')
    if evidence:
        directory = Path(evidence).resolve()
        directory.mkdir(parents=True, exist_ok=False)
        records = run(request, directory)
    else:
        with tempfile.TemporaryDirectory(prefix='hng-pcap-') as temporary:
            records = run(request, Path(temporary))
    print(json.dumps(records))


if __name__ == '__main__':
    main()
