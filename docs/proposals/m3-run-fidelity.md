<!-- SPDX-License-Identifier: Apache-2.0 -->

# PROPOSAL: realized late-run fidelity

Status: **PROPOSAL, awaiting research approval. No acceptance gate is enabled.**

Compare measured source and actual HngBuffer output on the same received packet
identities, ordered by original send/ingress time. Output OWD is modeled release
minus arrival and includes serialization and clamp; subtract the declared
propagation offset as an integer. Use the full-source, first-send-anchored 15 s
median baseline and strict excess >20 ms. Sequence gaps and send gaps >3 times
the original median cadence split runs. For fixed two-point mode, use its known
base t and independently generated source late flags; preserve source order even
when permitted late/ordinary overtaking changes output order.

Proposed joint equivalence tolerances, per source window/direction:

- Absolute late-packet share difference <=0.1 percentage point.
- Maximum distance between packet-run-length empirical CDFs <=0.05.
- Absolute P(next late | late) difference <=1 percentage point.

Proposed minimum evidence: at least 100 late packets, 20 late runs and 100
eligible late-to-next pairs in both source and output; otherwise INCONCLUSIVE.
Report histograms, spans, numerators/denominators and clamp count/max as well as
the comparison metrics. The research owner must approve/adjust thresholds and
any repetition/uncertainty protocol before an equivalence claim.

The tool always labels fidelity `PROPOSAL_PENDING`. Its independent per-packet
oracle checks are exact software correctness checks, separate from these
unapproved source-fidelity criteria. Passing the oracle cannot approve fidelity.
The synthetic downward-step test deliberately demonstrates that global clamp
can lengthen/merge late runs despite configured-row equality. Fixed p=0.01 and
p=0.10, delta=30 ms cases check actual output against their known two-point
sources. None of these results establishes wire precision or G2.

## Observed software replay (not an acceptance decision)

The external figure-1 DL source, 38-49 s, has ZIP SHA-256
`07647bd5b8d64525f5eaca40dcc995581611b593b70e78c0b0776984fe60c236`.
The exact oracle matched all 41,159 packets; 19,207 packets used clamp.
Source/output late counts were 271/272 and both had 31 runs. Run-length CDF
distance was 0.0322581, late-share difference 0.00242960 percentage point, and
P(next late | late) was 0.916031/0.916350 (difference 0.0319276 percentage point).
These observations cover one window, not the canonical pool.

Fixed delta=30 ms, seed=1, 10,000 packets each: p=0.01 produced 95 late
packets/95 runs; p=0.10 produced 1,073 late packets/952 runs. Source and actual
output histograms and conditional probabilities matched exactly in ingress
order. The p=0.01 case falls below the proposed minimum evidence.
