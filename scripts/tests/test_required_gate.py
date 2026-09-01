import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".forgejo/workflows/deep-verification.yml"
CATALOG = ROOT / "scripts/ci-suites.json"


class RequiredGateTests(unittest.TestCase):
    def test_workflow_emits_the_exact_catalog_and_retains_reports(self):
        workflow = WORKFLOW.read_text(encoding="utf-8")
        value = json.loads(CATALOG.read_text(encoding="utf-8"))
        suites = [suite["id"] for suite in value["suites"]]
        self.assertEqual(len(suites), len(set(suites)))
        for suite in suites:
            self.assertEqual(workflow.count(f"--suite {suite}"), 1, suite)
        self.assertIn("name: Forgejo / Paper required", workflow)
        self.assertIn("pull_request: {}", workflow)
        self.assertIn("SKWD_VERIFY_ROOT: .ci/checkouts/skwd-verify", workflow)
        self.assertIn(
            'if: always()\n        run: python3 "$SKWD_VERIFY_ROOT/scripts/ci-report.py" aggregate',
            workflow,
        )
        self.assertIn("retention-days: 14", workflow)
        self.assertIn(
            "actions/upload-artifact@c6a3b2bd78b3985e4b2f15397fec357f0fd808de",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()
