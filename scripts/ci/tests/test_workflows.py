"""跨平台 CI、供应链和工作流固定版本的仓库契约测试。"""

import json
import re
import subprocess
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
WORKFLOW_DIR = ROOT / ".github" / "workflows"
WORKFLOWS = {
    "rust.yml": [
        "ubuntu-latest",
        "windows-latest",
        "macos-latest",
        "cargo",
        "fmt",
        "clippy",
        "test --workspace --all-features --exclude jc-memory-python",
        "doc",
    ],
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
PNPM_VERSION = "9.15.9"
BUF_VERSION = "1.64.0"
BUF_SETUP_SHA = "a47c93e0b1648d5651a065437926377d060baa99"
SETUP_GO_SHA = "924ae3a1cded613372ab5595356fb5720e22ba16"
PUSH_WORKFLOWS = (
    "benchmarks.yml",
    "mobile.yml",
    "proto.yml",
    "rust.yml",
    "sdks.yml",
    "security.yml",
)


class WorkflowContractTests(unittest.TestCase):
    def test_all_required_workflows_exist_and_cover_their_gate(self) -> None:
        for filename, required_fragments in WORKFLOWS.items():
            content = (WORKFLOW_DIR / filename).read_text(encoding="utf-8")
            for fragment in required_fragments:
                self.assertIn(fragment, content, f"{filename} 缺少 {fragment}")

    def test_regular_workflows_run_on_master_push(self) -> None:
        expected_branch_filter = 'branches: [main, master, "feat/**"]'
        for filename in PUSH_WORKFLOWS:
            workflow = (WORKFLOW_DIR / filename).read_text(encoding="utf-8")
            self.assertIn(expected_branch_filter, workflow, f"{filename} 未监听 master push")

    def test_non_unix_directory_sync_helpers_use_ready_futures(self) -> None:
        for relative_path in (
            "crates/memory-storage/src/blob/store.rs",
            "crates/memory-storage/src/snapshot.rs",
        ):
            source = (ROOT / relative_path).read_text(encoding="utf-8")
            self.assertNotIn("#[cfg(not(unix))]\nasync fn sync_directory", source, relative_path)
            self.assertIn("std::future::ready(Ok(()))", source, relative_path)

    def test_rust_quality_installs_fixed_buf_before_workspace_tests(self) -> None:
        workflow = (WORKFLOW_DIR / "rust.yml").read_text(encoding="utf-8")
        install = f"uses: bufbuild/buf-setup-action@{BUF_SETUP_SHA}"
        workspace_test = 'cargo +"$RUST_TOOLCHAIN" test --workspace'
        self.assertIn(install, workflow)
        self.assertIn(f'version: "{BUF_VERSION}"', workflow)
        self.assertLess(workflow.index(install), workflow.index(workspace_test))

    def test_rust_quality_bounds_cargo_disk_usage(self) -> None:
        workflow = (WORKFLOW_DIR / "rust.yml").read_text(encoding="utf-8")
        for setting in (
            'CARGO_INCREMENTAL: "0"',
            'CARGO_PROFILE_DEV_DEBUG: "0"',
            'CARGO_PROFILE_TEST_DEBUG: "0"',
        ):
            self.assertIn(setting, workflow)

    def test_security_uses_module_go_version_for_govulncheck(self) -> None:
        workflow = (WORKFLOW_DIR / "security.yml").read_text(encoding="utf-8")
        setup = f"uses: actions/setup-go@{SETUP_GO_SHA}"
        govulncheck = "go install golang.org/x/vuln/cmd/govulncheck@v1.1.4"
        self.assertIn(setup, workflow)
        self.assertIn("go-version-file: sdk/go/go.mod", workflow)
        self.assertLess(workflow.index(setup), workflow.index(govulncheck))

    def test_windows_security_noops_expect_fallible_contract_lint(self) -> None:
        source = (ROOT / "crates" / "memory-security" / "src" / "key.rs").read_text(
            encoding="utf-8"
        )
        helpers = (
            "sync_directory",
            "ensure_restricted_directory",
            "set_restricted_directory_permissions",
            "ensure_restricted_file",
            "set_restricted_file_permissions",
        )
        for helper in helpers:
            pattern = (
                r"#\[cfg_attr\(\s*not\(unix\),\s*expect\(\s*"
                r"clippy::unnecessary_wraps,\s*reason = \"[^\"]+\"\s*\)\s*\)\]\s*"
                rf"fn {helper}\b"
            )
            self.assertRegex(source, pattern, helper)
        self.assertIn("let _ = (path, file);", source)
        self.assertRegex(
            source,
            r"fn set_restricted_file_permissions[\s\S]{0,500}let _ = file;",
        )

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

    def test_protocol_codegen_uses_vendored_protoc(self) -> None:
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        protocol = tomllib.loads(
            (ROOT / "crates" / "memory-protocol" / "Cargo.toml").read_text(encoding="utf-8")
        )
        build_script = (ROOT / "crates" / "memory-protocol" / "build.rs").read_text(
            encoding="utf-8"
        )

        self.assertIn("protoc-bin-vendored", workspace["workspace"]["dependencies"])
        self.assertTrue(protocol["build-dependencies"]["protoc-bin-vendored"]["workspace"])
        self.assertIn("protoc_bin_vendored::protoc_bin_path()", build_script)
        self.assertIn("protoc_executable", build_script)

    def test_node_workflows_pin_pnpm(self) -> None:
        package = json.loads((ROOT / "sdk" / "node" / "package.json").read_text(encoding="utf-8"))
        self.assertEqual(package.get("packageManager"), f"pnpm@{PNPM_VERSION}")
        for filename in ("sdks.yml", "security.yml"):
            workflow = (WORKFLOW_DIR / filename).read_text(encoding="utf-8")
            self.assertIn(f"corepack pnpm@{PNPM_VERSION}", workflow)

    def test_workspace_crates_are_non_publishable(self) -> None:
        result = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        packages = json.loads(result.stdout)["packages"]
        publishable = [package["name"] for package in packages if package["publish"] != []]
        self.assertEqual(publishable, [], f"以下私有 crate 仍可发布：{publishable}")

    def test_deny_allows_licenses_required_by_locked_dependencies(self) -> None:
        deny = tomllib.loads((ROOT / "deny.toml").read_text(encoding="utf-8"))
        allowed = set(deny["licenses"]["allow"])
        required = {
            "Apache-2.0",
            "Apache-2.0 WITH LLVM-exception",
            "CDLA-Permissive-2.0",
            "MPL-2.0",
        }
        self.assertEqual(required - allowed, set())

    def test_deny_allows_warnings_but_keeps_denied_findings_blocking(self) -> None:
        deny = tomllib.loads((ROOT / "deny.toml").read_text(encoding="utf-8"))
        self.assertTrue(deny["bans"].get("allow-wildcard-paths"))
        security = (WORKFLOW_DIR / "security.yml").read_text(encoding="utf-8")
        self.assertIn("cargo deny check -A warnings", security)

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
