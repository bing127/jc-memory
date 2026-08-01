import { readFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, test } from "vitest";

import { createCredentialCipher } from "../src/crypto.js";
import { openControlPlaneDatabase } from "../src/database.js";
import { createInstanceStore } from "../src/instances.js";

const namespaceId = "00000000-0000-0000-0000-000000000001";
const secret = "instance-api-key-must-stay-server-side";
const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories.splice(0).map((directory) =>
      rm(directory, { recursive: true, force: true }),
    ),
  );
});

describe("instance registry", () => {
  test("stores credentials encrypted and never returns them from list", async () => {
    const { databasePath, store, close } = await createStore();

    const created = store.create({
      name: "本地开发实例",
      baseUrl: "http://127.0.0.1:8787",
      namespaceId,
      apiKey: secret,
    });

    expect(store.list()).toEqual([created]);
    expect(store.getConnection(created.id)).toMatchObject({
      ...created,
      apiKey: secret,
    });

    close();
    expect(readFileSync(databasePath).includes(Buffer.from(secret))).toBe(false);
  });

  test("deleting a connection removes only control-plane metadata", async () => {
    const { store, close } = await createStore();
    const created = store.create({
      name: "待删除连接",
      baseUrl: "https://memory.example.com",
      namespaceId,
      apiKey: secret,
    });

    expect(store.remove(created.id)).toBe(true);
    expect(store.list()).toEqual([]);
    expect(store.getConnection(created.id)).toBeNull();
    expect(store.remove(created.id)).toBe(false);
    close();
  });

  test("rejects encryption keys that are not 32 bytes", () => {
    expect(() => createCredentialCipher(Buffer.alloc(31))).toThrow(
      "32-byte encryption key",
    );
  });
});

async function createStore() {
  const directory = await mkdtemp(join(tmpdir(), "jc-memory-control-plane-"));
  temporaryDirectories.push(directory);
  const databasePath = join(directory, "control-plane.db");
  const database = openControlPlaneDatabase(databasePath);
  const cipher = createCredentialCipher(Buffer.alloc(32, 7));
  const store = createInstanceStore(database, cipher, {
    id: () => "019b8c36-e7b0-7f41-8b92-cc77e129fc2d",
    now: () => "2026-08-01T00:00:00.000Z",
  });

  return {
    databasePath,
    store,
    close: () => database.close(),
  };
}
