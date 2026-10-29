# Initial implementation — retained, superseded

Refs #125. This is the first implementation at local `4640402`, before the
selected fallback extraction. All original canonical and diagnostic rows,
including regressions, remain unchanged. It is not the final PR measurement.

The canonical four-engine run has fresh-build receipts, exact source/binary
snapshots, an identical-source rebuilt control and verified public export.
Independent audit confirmed every table row and observation. Its pooled
collection reduction of 7.95% is descriptive, not an established exact effect:
in round 1 the candidate was only 0.85% faster than identical-source control,
while baseline/control themselves differed substantially. Nested rounds disagreed.

The initial isolated decimal-bound and upper-u128 regressions are genuine
observations, not erased as noise. They motivated the subsequent bounded
experiments. Diagnostic provenance is explicitly **post-hoc fingerprinting**;
no pre/post-run diagnostic source/binary gate was recorded. See
`diagnostic-provenance.json`, which must not be confused with the stronger
canonical provenance or the later gated final diagnostics. Allocation counts
stop at result construction, before disposal; they do not count deallocation.
