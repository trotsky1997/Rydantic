#!/usr/bin/env python3
"""One-process worker. Imports/model compilation/input setup are outside timing."""
import copy
import gc
import json
import platform
import sys
from time import perf_counter_ns
from typing import Annotated

import pydantic
import pydantic_core
from pydantic import BaseModel, ConfigDict, Field, ValidationError


class StrictModel(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")


class Flat(StrictModel):
    id: Annotated[int, Field(ge=0, le=1_000_000_000)]
    name: Annotated[str, Field(min_length=1, max_length=64)]
    active: bool


class Nested(StrictModel):
    user: Flat
    tags: list[str]


class Collection(StrictModel):
    items: list[Flat]


MODELS = {"flat": Flat, "nested": Nested, "collection": Collection}


def category(code):
    if code == "missing":
        return "missing"
    if code == "extra_forbidden":
        return "extra"
    if code == "json_invalid":
        return "json_syntax"
    if code in {"greater_than_equal", "less_than_equal"}:
        return "range"
    if code in {"string_too_short", "string_too_long"}:
        return "length"
    if code.endswith("_type"):
        return "type"
    raise ValueError(f"unmapped native error code: {code}")


def pointer(loc):
    return "".join("/" + str(part).replace("~", "~0").replace("/", "~1") for part in loc)


def bounded_int(request, key, low, high):
    value = request[key]
    if type(value) is not int or not low <= value <= high:
        raise ValueError(f"{key} must be integer in [{low}, {high}]")
    return value


def run(request):
    model = MODELS[request["schema"]]
    mode = request["mode"]
    if mode not in {"json", "value_copy"}:
        raise ValueError("unknown mode")
    raw = request["input"]
    if not isinstance(raw, str):
        raise ValueError("input must be a JSON string")
    iterations = bounded_int(request, "iterations", 1, 10_000_000)
    warmup = bounded_int(request, "warmup", 0, 1_000_000)
    repeats = bounded_int(request, "repeats", 1, 100)
    decoded = json.loads(raw) if mode == "value_copy" else None

    def operation():
        if mode == "json":
            return model.model_validate_json(raw)
        return model.model_validate(copy.deepcopy(decoded))

    try:
        result = operation()
        correctness = {"accepted": True, "output": result.model_dump(mode="json"), "errors": []}
    except ValidationError as error:
        errors = [{"path": pointer(e["loc"]), "category": category(e["type"])}
                  for e in error.errors(include_url=False, include_context=False, include_input=False)]
        correctness = {"accepted": False, "output": None,
                       "errors": sorted(errors, key=lambda e: (e["path"], e["category"]))}

    # Consume success/failure; CPython does not eliminate calls or returned objects.
    # Error materialization/rendering and model_dump are deliberately outside timing.
    def batch(count):
        accepted = 0
        for _ in range(count):
            try:
                result = operation()
                accepted += result is not None
                del result
            except ValidationError:
                pass
        return accepted

    batch(warmup)
    samples, baselines = [], []
    checksum = 0
    for _ in range(repeats):
        start = perf_counter_ns()
        checksum += batch(iterations)
        samples.append(perf_counter_ns() - start)
        start = perf_counter_ns()
        baseline_count = 0
        for _ in range(iterations):
            baseline_count += 1
        baselines.append(perf_counter_ns() - start)
        if baseline_count != iterations:
            raise RuntimeError("baseline consumption failed")
    return {**correctness, "samples_ns": samples, "baseline_ns": baselines,
            "iterations": iterations, "checksum": checksum,
            "engine": {"python": platform.python_version(), "implementation": platform.python_implementation(),
                       "pydantic": pydantic.__version__, "pydantic_core": pydantic_core.__version__,
                       "gc_enabled": gc.isenabled(), "python_optimize": sys.flags.optimize}}


if __name__ == "__main__":
    try:
        print(json.dumps(run(json.load(sys.stdin)), ensure_ascii=False, allow_nan=False))
    except Exception as error:
        print(f"worker failed: {type(error).__name__}: {error}", file=sys.stderr)
        sys.exit(1)
