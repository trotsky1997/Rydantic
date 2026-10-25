"""Negative controls: a fast but wrong worker must never produce a comparison."""
import copy
import unittest

from run import summarize, verify, verify_engine


class GateTests(unittest.TestCase):
    def setUp(self):
        self.case = {"name": "fixture", "accepted": True, "output": {"id": 1}, "errors": []}
        self.request = {"iterations": 2, "repeats": 3}
        self.result = {"accepted": True, "output": {"id": 1}, "errors": [],
                       "iterations": 2, "checksum": 6, "samples_ns": [100, 200, 300],
                       "baseline_ns": [10, 20, 30]}

    def test_correct_worker(self):
        verify(self.case, self.result, self.request)

    def test_rejects_wrong_output_types(self):
        for wrong in (True, 1.0, "1", None):
            with self.subTest(wrong=wrong), self.assertRaises(ValueError):
                result = copy.deepcopy(self.result)
                result["output"]["id"] = wrong
                verify(self.case, result, self.request)

    def test_rejects_wrong_acceptance(self):
        result = dict(self.result, accepted=False)
        with self.assertRaises(ValueError):
            verify(self.case, result, self.request)

    def test_rejects_wrong_errors(self):
        case = dict(self.case, accepted=False, output=None, errors=[{"path": "/id", "category": "type"}])
        for errors in ([], [{"path": "/wrong", "category": "type"}], [{"path": "/id", "category": "range"}]):
            with self.subTest(errors=errors), self.assertRaises(ValueError):
                verify(case, dict(self.result, accepted=False, output=None, errors=errors, checksum=0), self.request)

    def test_rejects_skipped_work_and_invalid_samples(self):
        for field, value in (("checksum", 0), ("iterations", 1), ("samples_ns", [1]),
                             ("samples_ns", [0, 1, 2]), ("baseline_ns", [-1, 2, 3])):
            with self.subTest(field=field), self.assertRaises(ValueError):
                verify(self.case, dict(self.result, **{field: value}), self.request)

    def test_rejects_debug_and_unpinned_versions(self):
        with self.assertRaises(ValueError):
            verify_engine("rydantic", {"engine": {"name": "rydantic", "profile": "debug"}})
        with self.assertRaises(ValueError):
            verify_engine("pydantic", {"engine": {"pydantic": "2.0.0", "pydantic_core": "2.50.1"}})
        verify_engine("rydantic", {"engine": {"name": "rydantic", "profile": "release"}})
        verify_engine("pydantic", {"engine": {"pydantic": "2.14.1", "pydantic_core": "2.50.1"}})

    def test_summary_is_batch_average_not_request_percentile(self):
        summary = summarize(self.result)
        self.assertEqual(summary["median_ns_per_operation"], 100)
        self.assertEqual(summary["iqr_ns_per_operation"], 50)
        self.assertEqual(summary["operations_per_second_from_median"], 10_000_000)
        self.assertEqual(summary["baseline_median_ns_per_iteration"], 10)


if __name__ == "__main__":
    unittest.main()
