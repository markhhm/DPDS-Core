<!-- SPDX-License-Identifier: Apache-2.0 -->

# M3 canonical pool replay, 2026-11-01

**Adopted converter tolerance: 0.01 ms (10,000 ns). Software-only canonical fidelity: PASS.**
All 56 cases are present: **24 PASS, 0 FAIL, 32 INCONCLUSIVE**. Every case passes the independent
service/deadline/release/order oracle and complete pcap egress identity/bytes/order checks. All ten
previous 0.05 ms FAIL cases pass with the adopted converter. The fidelity thresholds and engine are unchanged.
Insufficient windows remain listed; this result is not hardware precision, throughput or G2 acceptance.

## Adopted representation and preserved history

Only the converter subdivision tolerance and matching recipe.adaptive_ns changed from 50,000 to 10,000 ns.
Midpoint sampling, boundary/latency quantization at 1,000 ns, source/window/phase selection, seed, corridors,
C/Q, finite drain, FIFO clamp and m3-fidelity-v1 budgets are unchanged. The current converter is byte-identical
to the verified 0.01 ms snapshot used for all selected profiles. No replay rerun was needed after that identity check.

| Setting | PASS | FAIL | INCONCLUSIVE | Role |
| --- | ---: | ---: | ---: | --- |
| 0.05 ms | 14 | 10 | 32 | Preserved historical baseline |
| 0.01 ms | 24 | 0 | 32 | Adopted canonical profiles |

The historical baseline report, result inventory and raw evidence remain in the external evidence archive.
The unfinished 0.005/0.001 ms trials are not adopted and supply no full-pool verdict. All selected .01 ms
profile/manifest/record/pcap/reference hashes were verified before this update.

## Registered pool and model

The [11 s manifest](m3-canonical-pool-11s.json) retains all 12 paired candidates and two unpaired exclusions.
The [40 s manifest](m3-canonical-pool-40s.json) records all four natural candidates; replay uses the approved
2025-04-16-09-26-16 [8,48) and 2025-04-19-01-16-16 [8.95,48.95) pairs. The approved 11 s [38,49)
09-26-16 pair is part of the complete 11 s pool. Other 40 s candidates are outside the adopted extended-window scope.
No deficient 11 s window was deleted, pooled or replaced by a longer window.

11 s pool SHA-256: `5ae04225066a55ffd2564cd52609f1f852dfb3ebf765191b93b99861fae94dfe`; 40 s: `11d5dba0f158137a3708d5359ed264481f27a19c6150e8e3f7faf7bc5221bd7d`.
Selected sweep registration SHA-256: `09192a1bf2f2e0cbed9075caa3223225dce9007462075ec898e8e1fcb5ef57ba`.
Source ZIP/member hashes, phase and windows are in those manifests.
[Garcia dataset](https://doi.org/10.5281/zenodo.16275284), CC BY 4.0: no measured ZIP, converted trace,
pcap or per-packet record is redistributed in this code repository.

Each direction/corridor uses the same received-sample input as the baseline. Offsets are A=45,600,000 ns
and B=106,800,000 ns, seed=1, p=0, delta=0, C=8,000,000,000 bit/s and Q=80 bytes.
Synthetic 76-byte VLAN frames consume 80 Q bytes including FCS and 100 wire bytes, giving 100 ns service.
Original received send times relative to the window are encoded in payload; pcap timestamps do not pace input.
Terminal HOLD extends max(t)+delta+1,000,000 ns beyond the window; actual Q wire drain is 100 ns.
Storage satisfies the existing Engine startup envelope. End-of-input drains every accepted packet.

The deterministic adapter exercises net_pcap RX → current Engine C API/HngBuffer → net_pcap TX.
It does not execute production host RX/worker/TX threads or establish wall-clock/wire precision.
All settings use the same independent rational deadline oracle.

## Per-case fidelity

Policy: [m3-fidelity-v1](../proposals/m3-run-fidelity.md). N is matched received count. Tuples are L/R/E/J.
Late count budget is max(2,ceil(L_source/20)); changed-run budget is max(1,ceil(R_source/20));
conditional-probability difference budget is 1 pp. Both sides need L>=100, R>=20 and E>=100.
These gates, the common full-source median baseline, strict >20 ms and both gap cut rules are unchanged.

| Case: source/direction/corridor/window s | N | Source L/R/E/J | Output L/R/E/J | Delta L/budget | Runs/budget | P diff pp | L / run / P checks | Verdict |
| --- | ---: | --- | --- | --- | --- | ---: | --- | --- |
| 2025-04-11-15-16-16 UL A [8,19) | 41294 | 503/23/502/480 | 503/23/502/480 | 0/26 | 0/2 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-11-15-16-16 UL B [8,19) | 41294 | 503/23/502/480 | 503/23/502/480 | 0/26 | 0/2 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-11-15-16-16 DL A [8,19) | 41315 | 37/1/37/36 | 37/1/37/36 | 0/2 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-11-15-16-16 DL B [8,19) | 41315 | 37/1/37/36 | 37/1/37/36 | 0/2 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-11-15-16-16 UL A [23,34) | 41299 | 441/15/438/426 | 441/15/438/426 | 0/23 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-11-15-16-16 UL B [23,34) | 41299 | 441/15/438/426 | 441/15/438/426 | 0/23 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-11-15-16-16 DL A [23,34) | 41277 | 212/1/211/211 | 212/1/211/211 | 0/11 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-11-15-16-16 DL B [23,34) | 41277 | 212/1/211/211 | 212/1/211/211 | 0/11 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-11-15-16-16 UL A [38,49) | 41226 | 857/40/856/817 | 859/40/858/819 | 2/43 | 2/2 | 0.01062022 | PASS / PASS / PASS | PASS |
| 2025-04-11-15-16-16 UL B [38,49) | 41226 | 857/40/856/817 | 859/40/858/819 | 2/43 | 2/2 | 0.01062022 | PASS / PASS / PASS | PASS |
| 2025-04-11-15-16-16 DL A [38,49) | 41302 | 82/3/82/79 | 82/3/82/79 | 0/5 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-11-15-16-16 DL B [38,49) | 41302 | 82/3/82/79 | 82/3/82/79 | 0/5 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 UL A [8,19) | 41287 | 620/9/615/611 | 620/9/615/611 | 0/31 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 UL B [8,19) | 41287 | 620/9/615/611 | 620/9/615/611 | 0/31 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 DL A [8,19) | 41233 | 110/1/110/109 | 110/1/110/109 | 0/6 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 DL B [8,19) | 41233 | 110/1/110/109 | 110/1/110/109 | 0/6 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 UL A [23,34) | 41297 | 599/6/595/593 | 599/6/595/593 | 0/30 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 UL B [23,34) | 41297 | 599/6/595/593 | 599/6/595/593 | 0/30 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 DL A [23,34) | 41334 | 237/3/235/234 | 237/3/235/234 | 0/12 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 DL B [23,34) | 41334 | 237/3/235/234 | 237/3/235/234 | 0/12 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 UL A [38,49) | 41036 | 6603/178/6546/6425 | 6604/178/6547/6426 | 1/331 | 3/9 | 0.00028234 | PASS / PASS / PASS | PASS |
| 2025-04-16-08-36-16 UL B [38,49) | 41036 | 6603/178/6546/6425 | 6604/178/6547/6426 | 1/331 | 3/9 | 0.00028234 | PASS / PASS / PASS | PASS |
| 2025-04-16-08-36-16 DL A [38,49) | 41270 | 299/4/298/295 | 299/4/298/295 | 0/15 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-08-36-16 DL B [38,49) | 41270 | 299/4/298/295 | 299/4/298/295 | 0/15 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-09-26-16 UL A [8,19) | 41304 | 667/28/664/639 | 668/28/665/640 | 1/34 | 1/2 | 0.00566174 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 UL B [8,19) | 41304 | 667/28/664/639 | 668/28/665/640 | 1/34 | 1/2 | 0.00566174 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 DL A [8,19) | 41222 | 75/7/69/68 | 75/7/69/68 | 0/4 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-09-26-16 DL B [8,19) | 41222 | 75/7/69/68 | 75/7/69/68 | 0/4 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-09-26-16 UL A [23,34) | 41217 | 3121/167/3115/2954 | 3125/166/3119/2959 | 4/157 | 6/9 | 0.03869002 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 UL B [23,34) | 41217 | 3121/167/3115/2954 | 3125/166/3119/2959 | 4/157 | 6/9 | 0.03869002 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 DL A [23,34) | 41168 | 34/1/34/33 | 34/1/34/33 | 0/2 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-09-26-16 DL B [23,34) | 41168 | 34/1/34/33 | 34/1/34/33 | 0/2 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-09-26-16 UL A [38,49) | 41110 | 2130/103/2118/2027 | 2131/103/2119/2028 | 1/107 | 5/6 | 0.00202761 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 UL B [38,49) | 41110 | 2130/103/2118/2027 | 2131/103/2119/2028 | 1/107 | 5/6 | 0.00202761 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 DL A [38,49) | 41087 | 366/47/364/319 | 366/47/364/319 | 0/19 | 0/3 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 DL B [38,49) | 41087 | 366/47/364/319 | 366/47/364/319 | 0/19 | 0/3 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-19-01-16-16 UL A [8.95,19.95) | 41220 | 925/35/921/890 | 925/35/921/890 | 0/47 | 0/2 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-19-01-16-16 UL B [8.95,19.95) | 41220 | 925/35/921/890 | 925/35/921/890 | 0/47 | 0/2 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-19-01-16-16 DL A [8.95,19.95) | 41288 | 233/5/229/228 | 233/5/229/228 | 0/12 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-19-01-16-16 DL B [8.95,19.95) | 41288 | 233/5/229/228 | 233/5/229/228 | 0/12 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-19-01-16-16 UL A [23.95,34.95) | 41268 | 480/12/477/468 | 480/12/477/468 | 0/24 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-19-01-16-16 UL B [23.95,34.95) | 41268 | 480/12/477/468 | 480/12/477/468 | 0/24 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-19-01-16-16 DL A [23.95,34.95) | 41235 | 216/9/209/207 | 216/9/209/207 | 0/11 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-19-01-16-16 DL B [23.95,34.95) | 41235 | 216/9/209/207 | 216/9/209/207 | 0/11 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-19-01-16-16 UL A [38.95,49.95) | 41281 | 560/19/553/541 | 560/19/553/541 | 0/28 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-19-01-16-16 UL B [38.95,49.95) | 41281 | 560/19/553/541 | 560/19/553/541 | 0/28 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-19-01-16-16 DL A [38.95,49.95) | 41217 | 133/14/120/119 | 133/14/120/119 | 0/7 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-19-01-16-16 DL B [38.95,49.95) | 41217 | 133/14/120/119 | 133/14/120/119 | 0/7 | 0/1 | 0.00000000 | PASS / PASS / PASS | INCONCLUSIVE |
| 2025-04-16-09-26-16 UL A [8,48) | 149914 | 5885/295/5859/5590 | 5892/294/5866/5598 | 7/295 | 13/15 | 0.02252618 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 UL B [8,48) | 149914 | 5885/295/5859/5590 | 5892/294/5866/5598 | 7/295 | 13/15 | 0.02252618 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 DL A [8,48) | 149770 | 517/66/498/451 | 517/66/498/451 | 0/26 | 0/4 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-16-09-26-16 DL B [8,48) | 149770 | 517/66/498/451 | 517/66/498/451 | 0/26 | 0/4 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-19-01-16-16 UL A [8.95,48.95) | 150005 | 1961/72/1948/1889 | 1961/72/1948/1889 | 0/99 | 0/4 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-19-01-16-16 UL B [8.95,48.95) | 150005 | 1961/72/1948/1889 | 1961/72/1948/1889 | 0/99 | 0/4 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-19-01-16-16 DL A [8.95,48.95) | 150056 | 582/28/558/554 | 582/28/558/554 | 0/30 | 0/2 | 0.00000000 | PASS / PASS / PASS | PASS |
| 2025-04-19-01-16-16 DL B [8.95,48.95) | 150056 | 582/28/558/554 | 582/28/558/554 | 0/30 | 0/2 | 0.00000000 | PASS / PASS / PASS | PASS |

Full histograms, clamp count/max, minima, implementation/binary hashes and selected evidence hashes are in
[the machine-readable inventory](m3-canonical-results.json). CDF distance remains descriptive, not a gate.

## Profile cost and TheaterQ configured-sequence parity

The 56 selected cases total **85,367,562 intervals** and **10,055,071,807 JSON bytes (9,589.26 MiB)**.
A/B and overlapping windows are included in those totals and are not independent source evidence.
File size is the actual indent=2 normalized JSON including recipe/provenance. Load time measures one
fresh-process hng_engine_load call through target/debug/libdpds_core.so, including SHA-256, JSON parsing,
validation and reservation. Files were recently read/written; caches were not controlled. Some observations
overlap another conversion. These debug-build Linux/WSL wall times are reference costs, not isolated benchmarks.
Raw ru_maxrss is excluded because inherited pre-exec high-water contaminates the memory comparison.

The independent Step 2 module SHA-256 is b1d87d374a6b036b8cdf810bf2025d5eba766333c589b96a4fd4202ccfcba8cd.
With the same source, window, seed and **adaptive_ms=0.01**, every selected profile has **maximum
configured latency difference 0 us**. This comparison checks all union breakpoints. Against the older
fixed adaptive_ms=0.05 reference, the maximum is **30 us**; the old setting is not silently relabeled equal.
These references were regenerated offline, not recovered from a deployed hardware receipt. Release/FIFO
rules differ across engines and are outside this configured-sequence equality check.

| Case | Intervals | JSON bytes | JSON MiB | Engine load ms | Same .01 ms TQ max diff us | Fixed .05 ms TQ max diff us |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 2025-04-11-15-16-16 UL A [8,19) | 1102824 | 129027487 | 123.050 | 3815.89 | 0.000 | 29.000 |
| 2025-04-11-15-16-16 UL B [8,19) | 1102824 | 130123820 | 124.096 | 3000.05 | 0.000 | 29.000 |
| 2025-04-11-15-16-16 DL A [8,19) | 1114821 | 130426702 | 124.385 | 3419.34 | 0.000 | 29.000 |
| 2025-04-11-15-16-16 DL B [8,19) | 1114821 | 131541511 | 125.448 | 5536.90 | 0.000 | 29.000 |
| 2025-04-11-15-16-16 UL A [23,34) | 1099888 | 128687628 | 122.726 | 4781.37 | 0.000 | 29.000 |
| 2025-04-11-15-16-16 UL B [23,34) | 1099888 | 129777827 | 123.766 | 3803.55 | 0.000 | 29.000 |
| 2025-04-11-15-16-16 DL A [23,34) | 1115209 | 130475972 | 124.432 | 4171.61 | 0.000 | 30.000 |
| 2025-04-11-15-16-16 DL B [23,34) | 1115209 | 131585954 | 125.490 | 3648.13 | 0.000 | 30.000 |
| 2025-04-11-15-16-16 UL A [38,49) | 1100693 | 128783765 | 122.818 | 3054.99 | 0.000 | 30.000 |
| 2025-04-11-15-16-16 UL B [38,49) | 1100693 | 129872658 | 123.856 | 3152.15 | 0.000 | 30.000 |
| 2025-04-11-15-16-16 DL A [38,49) | 1115311 | 130482967 | 124.438 | 2945.55 | 0.000 | 30.000 |
| 2025-04-11-15-16-16 DL B [38,49) | 1115311 | 131597853 | 125.501 | 2957.27 | 0.000 | 30.000 |
| 2025-04-16-08-36-16 UL A [8,19) | 1101929 | 128933545 | 122.961 | 2977.42 | 0.000 | 28.000 |
| 2025-04-16-08-36-16 UL B [8,19) | 1101929 | 130019089 | 123.996 | 2939.12 | 0.000 | 28.000 |
| 2025-04-16-08-36-16 DL A [8,19) | 1115175 | 130470354 | 124.426 | 3434.11 | 0.000 | 30.000 |
| 2025-04-16-08-36-16 DL B [8,19) | 1115175 | 131583450 | 125.488 | 3442.56 | 0.000 | 30.000 |
| 2025-04-16-08-36-16 UL A [23,34) | 1096212 | 128262824 | 122.321 | 3309.64 | 0.000 | 29.000 |
| 2025-04-16-08-36-16 UL B [23,34) | 1096212 | 129343381 | 123.351 | 3055.45 | 0.000 | 29.000 |
| 2025-04-16-08-36-16 DL A [23,34) | 1115533 | 130513396 | 124.467 | 3236.03 | 0.000 | 30.000 |
| 2025-04-16-08-36-16 DL B [23,34) | 1115533 | 131623967 | 125.526 | 3945.91 | 0.000 | 30.000 |
| 2025-04-16-08-36-16 UL A [38,49) | 1107476 | 129740919 | 123.731 | 3223.35 | 0.000 | 30.000 |
| 2025-04-16-08-36-16 UL B [38,49) | 1107476 | 130674193 | 124.621 | 3195.25 | 0.000 | 30.000 |
| 2025-04-16-08-36-16 DL A [38,49) | 1115040 | 130457023 | 124.414 | 3981.59 | 0.000 | 30.000 |
| 2025-04-16-08-36-16 DL B [38,49) | 1115040 | 131565978 | 125.471 | 3252.93 | 0.000 | 30.000 |
| 2025-04-16-09-26-16 UL A [8,19) | 1105859 | 129391123 | 123.397 | 3004.66 | 0.000 | 29.000 |
| 2025-04-16-09-26-16 UL B [8,19) | 1105859 | 130482621 | 124.438 | 3236.07 | 0.000 | 29.000 |
| 2025-04-16-09-26-16 DL A [8,19) | 1113438 | 130267618 | 124.233 | 3322.23 | 0.000 | 30.000 |
| 2025-04-16-09-26-16 DL B [8,19) | 1113438 | 131378771 | 125.293 | 3413.45 | 0.000 | 30.000 |
| 2025-04-16-09-26-16 UL A [23,34) | 1098672 | 128760756 | 122.796 | 3165.40 | 0.000 | 29.000 |
| 2025-04-16-09-26-16 UL B [23,34) | 1098672 | 129634135 | 123.629 | 3365.24 | 0.000 | 29.000 |
| 2025-04-16-09-26-16 DL A [23,34) | 1114943 | 130441144 | 124.398 | 3218.33 | 0.000 | 30.000 |
| 2025-04-16-09-26-16 DL B [23,34) | 1114943 | 131555198 | 125.461 | 3532.52 | 0.000 | 30.000 |
| 2025-04-16-09-26-16 UL A [38,49) | 1102208 | 129256704 | 123.269 | 3169.07 | 0.000 | 29.000 |
| 2025-04-16-09-26-16 UL B [38,49) | 1102208 | 130051238 | 124.027 | 3280.11 | 0.000 | 29.000 |
| 2025-04-16-09-26-16 DL A [38,49) | 1113899 | 130325724 | 124.288 | 3715.01 | 0.000 | 30.000 |
| 2025-04-16-09-26-16 DL B [38,49) | 1113899 | 131432223 | 125.344 | 3080.88 | 0.000 | 30.000 |
| 2025-04-19-01-16-16 UL A [8.95,19.95) | 1096962 | 128349576 | 122.404 | 2988.81 | 0.000 | 29.000 |
| 2025-04-19-01-16-16 UL B [8.95,19.95) | 1096962 | 129432622 | 123.437 | 3020.81 | 0.000 | 29.000 |
| 2025-04-19-01-16-16 DL A [8.95,19.95) | 1115564 | 130517717 | 124.471 | 4760.19 | 0.000 | 30.000 |
| 2025-04-19-01-16-16 DL B [8.95,19.95) | 1115564 | 131627933 | 125.530 | 3901.84 | 0.000 | 30.000 |
| 2025-04-19-01-16-16 UL A [23.95,34.95) | 1098067 | 128476538 | 122.525 | 5036.17 | 0.000 | 29.000 |
| 2025-04-19-01-16-16 UL B [23.95,34.95) | 1098067 | 129563385 | 123.561 | 3583.21 | 0.000 | 29.000 |
| 2025-04-19-01-16-16 DL A [23.95,34.95) | 1112738 | 130191273 | 124.160 | 3523.33 | 0.000 | 30.000 |
| 2025-04-19-01-16-16 DL B [23.95,34.95) | 1112738 | 131299157 | 125.217 | 3422.85 | 0.000 | 30.000 |
| 2025-04-19-01-16-16 UL A [38.95,49.95) | 1097048 | 128358083 | 122.412 | 2925.34 | 0.000 | 29.000 |
| 2025-04-19-01-16-16 UL B [38.95,49.95) | 1097048 | 129442970 | 123.446 | 3162.98 | 0.000 | 29.000 |
| 2025-04-19-01-16-16 DL A [38.95,49.95) | 1113563 | 130285126 | 124.250 | 3082.22 | 0.000 | 30.000 |
| 2025-04-19-01-16-16 DL B [38.95,49.95) | 1113563 | 131395816 | 125.309 | 3200.21 | 0.000 | 30.000 |
| 2025-04-16-09-26-16 UL A [8,48) | 4006247 | 472204092 | 450.329 | 12236.59 | 0.000 | 29.000 |
| 2025-04-16-09-26-16 UL B [8,48) | 4006247 | 475628793 | 453.595 | 11382.48 | 0.000 | 29.000 |
| 2025-04-16-09-26-16 DL A [8,48) | 4051670 | 476989387 | 454.893 | 11577.79 | 0.000 | 30.000 |
| 2025-04-16-09-26-16 DL B [8,48) | 4051670 | 481028379 | 458.744 | 15935.76 | 0.000 | 30.000 |
| 2025-04-19-01-16-16 UL A [8.95,48.95) | 3991081 | 469878439 | 448.111 | 11597.70 | 0.000 | 29.000 |
| 2025-04-19-01-16-16 UL B [8.95,48.95) | 3991081 | 473832783 | 451.882 | 11456.30 | 0.000 | 29.000 |
| 2025-04-19-01-16-16 DL A [8.95,48.95) | 4051711 | 476990794 | 454.894 | 12468.30 | 0.000 | 30.000 |
| 2025-04-19-01-16-16 DL B [8.95,48.95) | 4051711 | 481029426 | 458.745 | 12013.88 | 0.000 | 30.000 |

## Reproduction and validation scope

Use the [trace tools](../TRACE-TOOLS.md) and optional pcap adapter with the adopted .01 ms converter.
Resolve each source/pool index by its frozen manifest. Keep all generated artifacts outside the public repo.
Run one heavy process at a time; cap potentially large Python processes with a 4 GiB address-space limit.

```sh
OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 prlimit --as=4294967296 -- python -B scripts/garcia_trace.py \
  "$RAW/2025-04-16-09-26-16__ul.csv.zip" "$OUT/normalized.json" \
  --start-ns 38000000000 --duration-ns 11000000000 --corridor A --seed 1 \
  --queue-drain-ns 1000000 --pool "$OUT/pool-11s.json" --pool-index 8
```

Repeat only the adopted recipe for the registered windows/directions/corridors when new evidence is required.
The selected 56 cases were already complete and verified with streamed hashes and independent integer gate
arithmetic, so this adoption does not rerun them. Fixed delta/p controls remain the previous N=20,000
Stage 1 controls; they are unaffected by converter subdivision and are not a LEO correlation claim.
No physical run, Git operation, pre-push hook, CI or new detector-v3 projection forms part of this adoption.
Physical validation still requires at least five independent repeats per cell (estimate), all passing.
Formal milestone and hardware acceptance remain separate from these software fidelity results.
