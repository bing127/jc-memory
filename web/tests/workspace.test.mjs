import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const webRoot = new URL("../", import.meta.url);

async function readJson(path) {
  return JSON.parse(await readFile(new URL(path, webRoot), "utf8"));
}

test("web workspace pins the toolchain and keeps frontend/backend boundaries", async () => {
  const [root, backend, frontend] = await Promise.all([
    readJson("package.json"),
    readJson("backend/package.json"),
    readJson("frontend/package.json"),
  ]);

  assert.equal(root.private, true);
  assert.equal(root.packageManager, "pnpm@9.15.9");
  assert.equal(root.scripts.test, "pnpm -r test");
  assert.equal(root.scripts.typecheck, "pnpm -r typecheck");
  assert.equal(root.scripts.build, "pnpm -r build");

  assert.equal(backend.name, "@jiangcheng-memory/control-plane-backend");
  assert.equal(backend.engines.node, ">=22.22");
  assert.equal(backend.scripts.test, "vitest run");
  assert.equal(backend.scripts.typecheck, "tsc -p tsconfig.json --noEmit");

  assert.equal(frontend.name, "@jiangcheng-memory/control-plane-frontend");
  assert.equal(frontend.engines.node, ">=22.22");
  assert.equal(frontend.scripts.test, "vitest run");
  assert.equal(frontend.scripts.typecheck, "tsc -p tsconfig.json --noEmit");
});
