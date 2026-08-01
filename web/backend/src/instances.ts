import { randomUUID } from "node:crypto";
import type { DatabaseSync } from "node:sqlite";

import type { CredentialCipher } from "./crypto.js";

export interface CreateInstanceInput {
  name: string;
  baseUrl: string;
  namespaceId: string;
  apiKey: string;
}

export interface InstanceSummary {
  id: string;
  name: string;
  baseUrl: string;
  namespaceId: string;
  protocolVersion: "v1";
  createdAt: string;
  updatedAt: string;
}

export interface InstanceConnection extends InstanceSummary {
  apiKey: string;
}

interface InstanceRow {
  id: string;
  name: string;
  base_url: string;
  namespace_id: string;
  protocol_version: "v1";
  api_key_nonce: string;
  api_key_ciphertext: string;
  api_key_tag: string;
  created_at: string;
  updated_at: string;
}

interface StoreOptions {
  id?: () => string;
  now?: () => string;
}

export interface InstanceStore {
  create(input: CreateInstanceInput): InstanceSummary;
  list(): InstanceSummary[];
  getConnection(id: string): InstanceConnection | null;
  remove(id: string): boolean;
}

export function createInstanceStore(
  database: DatabaseSync,
  cipher: CredentialCipher,
  options: StoreOptions = {},
): InstanceStore {
  const createId = options.id ?? randomUUID;
  const now = options.now ?? (() => new Date().toISOString());
  const insert = database.prepare(`
    INSERT INTO instances (
      id, name, base_url, namespace_id, protocol_version,
      api_key_nonce, api_key_ciphertext, api_key_tag, created_at, updated_at
    ) VALUES (?, ?, ?, ?, 'v1', ?, ?, ?, ?, ?)
  `);
  const selectAll = database.prepare(
    "SELECT * FROM instances ORDER BY created_at ASC, id ASC",
  );
  const selectById = database.prepare("SELECT * FROM instances WHERE id = ?");
  const deleteById = database.prepare("DELETE FROM instances WHERE id = ?");

  return {
    create(input) {
      const id = createId();
      const timestamp = now();
      const encrypted = cipher.encrypt(input.apiKey);
      insert.run(
        id,
        input.name,
        input.baseUrl,
        input.namespaceId,
        encrypted.nonce,
        encrypted.ciphertext,
        encrypted.tag,
        timestamp,
        timestamp,
      );

      return {
        id,
        name: input.name,
        baseUrl: input.baseUrl,
        namespaceId: input.namespaceId,
        protocolVersion: "v1",
        createdAt: timestamp,
        updatedAt: timestamp,
      };
    },
    list() {
      return (selectAll.all() as unknown as InstanceRow[]).map(toSummary);
    },
    getConnection(id) {
      const row = selectById.get(id) as unknown as InstanceRow | undefined;
      if (!row) {
        return null;
      }
      return {
        ...toSummary(row),
        apiKey: cipher.decrypt({
          nonce: row.api_key_nonce,
          ciphertext: row.api_key_ciphertext,
          tag: row.api_key_tag,
        }),
      };
    },
    remove(id) {
      return Number(deleteById.run(id).changes) > 0;
    },
  };
}

function toSummary(row: InstanceRow): InstanceSummary {
  return {
    id: row.id,
    name: row.name,
    baseUrl: row.base_url,
    namespaceId: row.namespace_id,
    protocolVersion: row.protocol_version,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}
