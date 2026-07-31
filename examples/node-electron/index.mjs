import { readFile } from "node:fs/promises";
import { RemoteMemoryClient } from "../../sdk/node/dist/index.js";

const fixture = JSON.parse(
  await readFile(new URL("../../sdk/contracts/fixtures/recall-exact.json", import.meta.url), "utf8"),
);
const client = new RemoteMemoryClient({
  baseUrl: process.env.JC_MEMORY_URL ?? "http://127.0.0.1:8080",
  apiKey: process.env.JC_MEMORY_API_KEY,
});

try {
  const response = await client.recall(fixture.request);
  console.log(JSON.stringify(response));
} finally {
  client.close();
}
