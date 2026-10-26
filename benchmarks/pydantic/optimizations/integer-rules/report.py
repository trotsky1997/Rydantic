#!/usr/bin/env python3
"""Recompute complete tables directly from private or public raw evidence."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import statistics
from common import ENGINES, load_json


def stats(values):
    q1, _, q3 = statistics.quantiles(values, n=4, method="inclusive")
    return statistics.median(values), q3 - q1


def render(data):
    samples = defaultdict(list)
    rounds = defaultdict(list)
    for row in data["measurements"]:
        key = row["case"], row["mode"], row["engine"]
        values = [value / row["result"]["iterations"] for value in row["result"]["samples_ns"]]
        samples[key].extend(values)
        rounds[row["round"], *key].extend(values)
    fixtures = json.loads(data["fixtures_text"])
    jobs = [(c["name"], m) for c in fixtures["cases"] for m in c["modes"]]
    p = data["protocol"]
    out = ["# Integer rule validation: paired measurements", "", "Refs #125.", "",
           "## Protocol and provenance", "",
           f"UTC: {data['started']} to {data['finished']}. {p['iterations']:,} operations/batch, {p['warmup']:,} warmups, {p['repeats_per_position']} batches/position, {p['rounds']} rounds, {p['samples_per_engine_case']} samples/engine/case. All 49 case/mode pairs are retained.", "",
           f"Orders by round: `{p['engine_orders']}`. Case order is independently shuffled with seeds `{p['seed']} + round`. Processes run sequentially; warmup, validation gates and process startup are outside reported timings. Loop controls are retained, never subtracted.", "",
           "Baseline is current main before this fix. Control is a fresh build of exactly the same source, not another optimization. Candidate is this integer-rule change. Identical baseline/control executable hashes are permitted and explicitly reported. Their observed differences indicate run-to-run noise, not a code difference.", "",
           "Fresh build receipts verify opt-level 3, no debug info/assertions or overflow checks. Source snapshots and executable hashes are checked before and after measurement. Full source-file hashes and receipt hashes are in the raw evidence.", "",
           "| Engine | Measured local source commit | Binary SHA256 |", "|---|---|---|"]
    for engine in ENGINES[:-1]:
        b = data["builds"][engine]
        out.append(f"| {engine} | `{b['revision']}` | `{b['executable_sha256']}` |")
    out += ["", f"Baseline/control binary equality: **{data['builds']['baseline']['executable_sha256'] == data['builds']['control']['executable_sha256']}**.", "",
            f"Rust compiler: `{data['builds']['candidate']['rustc'].splitlines()[0]}`. Python: `{data['python_version']}`. Pydantic/core pins: 2.14.1/2.50.1. Python executable SHA256: `{data['python_executable_sha256']}`.", "",
            f"CPU: {data['cpu_model']}; {data['cpu_count']} logical CPUs. Affinity: `{data['affinity']}`. Shared-host load start/end: `{data['load_start']}` / `{data['load_end']}`. These metadata do not establish an exclusive host or fixed CPU frequency.", "",
            "## Complete results", "",
            "All timings are median (inclusive IQR) **batch-average ns/op**, not individual-request latency percentiles. Reduction = 100 × (1 − candidate/baseline); positive is faster. Baseline/control compares identical source; candidate/Pydantic > 1 means candidate is slower. Ratios of medians are descriptive, not statistical significance tests.", "",
            "| Case | Mode | Baseline | Rebuilt control | Candidate | Pydantic | Candidate reduction | Baseline/control | Candidate/Pydantic |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|"]
    slower = []
    for case, mode in jobs:
        values = [stats(samples[case, mode, engine]) for engine in ENGINES]
        medians = [v[0] for v in values]
        reduction = 100 * (1 - medians[2] / medians[0])
        cells = " | ".join(f"{median:.1f} ({iqr:.1f})" for median, iqr in values)
        out.append(f"| {case} | {mode} | {cells} | {reduction:+.2f}% | {medians[0] / medians[1]:.3f} | {medians[2] / medians[3]:.3f} |")
        if reduction < 0:
            slower.append(f"`{case}/{mode}` ({reduction:+.2f}%)")
    out += ["", "Rows with a slower candidate pooled median (including small/noisy differences): " + (", ".join(slower) if slower else "none in this run") + ".", "",
            "## Selected per-round results", "",
            "Per-round medians retain disagreement rather than replacing it with only a pooled result.", "",
            "| Round | Case (JSON) | Baseline | Rebuilt control | Candidate | Pydantic | Candidate reduction |", "|---|---|---:|---:|---:|---:|---:|"]
    for round_index in range(p["rounds"]):
        for case in ("flat_valid", "nested_valid", "collection_100", "flat_negative", "flat_upper_bound"):
            values = [stats(rounds[round_index, case, "json", engine])[0] for engine in ENGINES]
            out.append(f"| {round_index + 1} | {case} | " + " | ".join(f"{v:.1f}" for v in values) + f" | {100 * (1 - values[2] / values[0]):+.2f}% |")
    out += ["", "## Interpretation limits", "",
            "- Primary JSON includes parsing, validation, model/error construction and disposal. Secondary `value_copy` includes Rust Value clone versus Python deepcopy and is not pure validation or parser cost.",
            "- Invalid-input diagnostic work differs between libraries. Native formatting and full diagnostic equivalence are established by regression tests, not this normalized category/path timing oracle.",
            "- Workers repeatedly process fixed warmed payloads; Pydantic's default string cache remains enabled. This does not establish cold or high-cardinality performance.",
            "- Pydantic core is also Rust. No language-wide ranking, universal speedup, CPU-profile attribution, production claim or CI performance threshold follows.",
            "- The same-source control detects some timing/build confounds but cannot remove host scheduling, frequency, allocator placement or compiler-layout sensitivity between different programs.",
            "- Allocation instrumentation, if reported separately, is not used in these timed binaries and is not interchangeable with wall-clock evidence.", ""]
    return "\n".join(out)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(render(load_json(args.results)))


if __name__ == "__main__":
    main()
