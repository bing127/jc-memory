"""跨平台 CI、供应链和工作流固定版本的仓库契约测试。"""

import re
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
WORKFLOW_DIR = ROOT / ".github" / "workflows"
WORKFLOWS = {
    "rust.yml": ["ubuntu-latest", "windows-latest", "macos-latest", "cargo", "fmt", "clippy", "doc"],
    "sdks.yml": ["node", "python", "go", "gradle", "swift"],
    "proto.yml": ["buf lint", "buf breaking", "buf generate"],
    "security.yml": ["cargo audit", "cargo deny", "pnpm", "pip-audit", "govulncheck"],
    "benchmarks.yml": ["scripts/benchmark/smoke.sh", "million-fragments"],
    "mobile.yml": ["Android", "assemble", "Swift", "swift test"],
    "release.yml": [
        "workflow_dispatch",
        "version",
        "private",
        "RUSTUP_TOOLCHAIN: \"1.97.1\"",
        "actions/upload-artifact",
        "jcmem",
    ],
}
CHECKOUT_SHA = "11bd71901bbe5b1630ceea73d27597364c9af683"


class WorkflowContractTests(unittest.TestCase):
    def test_all_required_workflows_exist_and_cover_their_gate(self) -> None:
        for filename, required_fragments in WORKFLOWS.items():
            content = (WORKFLOW_DIR / filename).read_text(encoding="utf-8")
            for fragment in required_fragments:
                self.assertIn(fragment, content, f"{filename} 缺少 {fragment}")

    def test_every_action_reference_uses_a_full_commit_sha(self) -> None:
        references = []
        for path in WORKFLOW_DIR.glob("*.yml"):
            references.extend(re.findall(r"uses:\s*([^\s#]+)", path.read_text(encoding="utf-8")))
        self.assertTrue(references, "工作流必须包含至少一个固定版本的 Action")
        for reference in references:
            self.assertRegex(reference, r"^[^@]+@[0-9a-f]{40}$", reference)
        checkout_references = [reference for reference in references if reference.startswith("actions/checkout@")]
        self.assertTrue(checkout_references)
        self.assertTrue(all(reference.endswith(CHECKOUT_SHA) for reference in checkout_references))

    def test_deny_and_dependabot_configuration_are_present(self) -> None:
        deny = (ROOT / "deny.toml").read_text(encoding="utf-8")
        self.assertIn("[advisories]", deny)
        self.assertIn("[bans]", deny)
        self.assertIn("[licenses]", deny)
        self.assertIn("[sources]", deny)
        dependabot = (ROOT / ".github" / "dependabot.yml").read_text(encoding="utf-8")
        self.assertIn("version: 2", dependabot)
        self.assertIn("package-ecosystem: cargo", dependabot)
        self.assertIn("package-ecosystem: github-actions", dependabot)

    def test_workflow_validator_rejects_invalid_fixture(self) -> None:
        validator = ROOT / "scripts" / "ci" / "validate-workflows.sh"
        fixture_dir = ROOT / "scripts" / "ci" / "fixtures" / "invalid"
        result = subprocess.run(
            ["bash", str(validator), str(fixture_dir)],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("YAML", result.stderr)


if __name__ == "__main__":
    unittest.main()
