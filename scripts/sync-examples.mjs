import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";

const root = new URL("../", import.meta.url);
const check = process.argv.includes("--check");
const examples = {
  "data-contract": "crates/rydantic/examples/data_contract.rs",
  "sqlite-api": "crates/rydantic/examples/sqlite_api.rs",
  "automatic-migrations": "crates/rydantic/examples/automatic_migrations.rs",
  "typed-agent": "crates/rydantic-ai/examples/typed_agent.rs",
  "structured-stream": "crates/rydantic-ai/examples/structured.rs",
};

if (!check) await mkdir(new URL("docs/snippets/examples/", root), { recursive: true });
for (const [name, source] of Object.entries(examples)) {
  const code = await readFile(new URL(source, root), "utf8");
  const snippet = `{/* Generated from ${source}. Run npm run examples:sync to refresh. */}\n\n\`\`\`rust ${source}\n${code.trimEnd()}\n\`\`\`\n`;
  const target = new URL(`docs/snippets/examples/${name}.mdx`, root);
  if (check) {
    assert.equal(await readFile(target, "utf8"), snippet, `${name}: run npm run examples:sync`);
  } else {
    await writeFile(target, snippet);
  }
}
console.log(`${check ? "Checked" : "Synced"} ${Object.keys(examples).length} runnable example snippets.`);
