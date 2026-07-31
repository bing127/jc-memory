"""私有 Release 脚本的失败边界和可审计产物契约测试。"""

import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
VERSION = ROOT / "scripts" / "release" / "version.sh"
CHECKSUMS = ROOT / "scripts" / "release" / "checksums.sh"
VERIFY = ROOT / "scripts" / "release" / "verify.sh"
MOBILE = ROOT / "scripts" / "release" / "mobile.sh"


class ReleaseScriptTests(unittest.TestCase):
    def run_script(self, script: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(script), *arguments],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_version_mismatch_is_rejected(self) -> None:
        result = self.run_script(VERSION, "--version", "9.9.9", "--allow-dirty")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("版本不一致", result.stderr)

    def test_dirty_tree_is_rejected_without_explicit_override(self) -> None:
        result = self.run_script(VERSION, "--version", "0.1.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("dirty", result.stderr)

    def test_missing_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "artifacts").mkdir()
            manifest = {
                "schema_version": "jcmem.release.v1",
                "version": "0.1.0",
                "target": "fixture",
                "license": "PRIVATE",
                "public_publish": False,
                "artifacts": ["artifacts/missing.bin"],
                "sbom": "sbom.cdx.json",
                "metadata": "build-metadata.json",
            }
            (root / "release-manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            (root / "sbom.cdx.json").write_text("{}", encoding="utf-8")
            (root / "build-metadata.json").write_text("{}", encoding="utf-8")
            result = self.run_script(VERIFY, "--input", str(root), "--version", "0.1.0")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Artifact", result.stderr)

    def test_checksum_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "artifacts").mkdir()
            artifact = root / "artifacts" / "fixture.bin"
            artifact.write_bytes(b"original")
            manifest = {
                "schema_version": "jcmem.release.v1",
                "version": "0.1.0",
                "target": "fixture",
                "license": "PRIVATE",
                "public_publish": False,
                "artifacts": ["artifacts/fixture.bin"],
                "sbom": "sbom.cdx.json",
                "metadata": "build-metadata.json",
            }
            (root / "release-manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            (root / "sbom.cdx.json").write_text("{}", encoding="utf-8")
            (root / "build-metadata.json").write_text("{}", encoding="utf-8")
            checksums = self.run_script(CHECKSUMS, "--input", str(root))
            self.assertEqual(checksums.returncode, 0, checksums.stderr)
            artifact.write_bytes(b"tampered")
            result = self.run_script(VERIFY, "--input", str(root), "--version", "0.1.0")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Checksum", result.stderr)

    def test_manifest_artifact_must_have_checksum_entry(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "artifacts").mkdir()
            artifact = root / "artifacts" / "fixture.bin"
            artifact.write_bytes(b"original")
            manifest = {
                "schema_version": "jcmem.release.v1",
                "version": "0.1.0",
                "target": "fixture",
                "license": "PRIVATE",
                "public_publish": False,
                "artifacts": ["artifacts/fixture.bin"],
                "sbom": "sbom.cdx.json",
                "metadata": "build-metadata.json",
            }
            (root / "release-manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            (root / "sbom.cdx.json").write_text("{}", encoding="utf-8")
            (root / "build-metadata.json").write_text("{}", encoding="utf-8")
            checksums = self.run_script(CHECKSUMS, "--input", str(root))
            self.assertEqual(checksums.returncode, 0, checksums.stderr)
            lines = (root / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
            (root / "SHA256SUMS").write_text(
                "\n".join(line for line in lines if "artifacts/fixture.bin" not in line) + "\n",
                encoding="utf-8",
            )
            result = self.run_script(VERIFY, "--input", str(root), "--version", "0.1.0")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Checksum 清单缺少", result.stderr)

    def test_mobile_help_describes_target_selection(self) -> None:
        result = self.run_script(MOBILE, "--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--target", result.stdout)


if __name__ == "__main__":
    unittest.main()
