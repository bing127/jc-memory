import {
  createCipheriv,
  createDecipheriv,
  randomBytes,
} from "node:crypto";

export interface EncryptedCredential {
  nonce: string;
  ciphertext: string;
  tag: string;
}

export interface CredentialCipher {
  encrypt(value: string): EncryptedCredential;
  decrypt(value: EncryptedCredential): string;
}

export function createCredentialCipher(key: Buffer): CredentialCipher {
  if (key.byteLength !== 32) {
    throw new Error("credential cipher requires a 32-byte encryption key");
  }

  return {
    encrypt(value) {
      const nonce = randomBytes(12);
      const cipher = createCipheriv("aes-256-gcm", key, nonce);
      const ciphertext = Buffer.concat([
        cipher.update(value, "utf8"),
        cipher.final(),
      ]);

      return {
        nonce: nonce.toString("base64"),
        ciphertext: ciphertext.toString("base64"),
        tag: cipher.getAuthTag().toString("base64"),
      };
    },
    decrypt(value) {
      const decipher = createDecipheriv(
        "aes-256-gcm",
        key,
        Buffer.from(value.nonce, "base64"),
      );
      decipher.setAuthTag(Buffer.from(value.tag, "base64"));
      return Buffer.concat([
        decipher.update(Buffer.from(value.ciphertext, "base64")),
        decipher.final(),
      ]).toString("utf8");
    },
  };
}
