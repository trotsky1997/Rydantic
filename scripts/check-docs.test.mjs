import { test } from "node:test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  readFile,
  mkdir,
  writeFile,
  rm,
  symlink,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { checkDocs, headingId } from "./check-docs.mjs";
const header = '---\ntitle: "Test"\ndescription: "Test guide"\n---\n\n';
async function fixture(t, body = "# Heading\n", files = {}, edit = () => {}) {
  const root = await mkdtemp(path.join(tmpdir(), "rydantic-docs-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const config = JSON.parse(
    await readFile(new URL("../docs/docs.json", import.meta.url), "utf8"),
  );
  config.navigation = {
    tabs: [
      { tab: "Test", groups: [{ group: "Test", pages: ["introduction"] }] },
    ],
  };
  edit(config);
  await writeFile(path.join(root, "docs.json"), JSON.stringify(config));
  for (const [name, source] of Object.entries({
    "introduction.mdx": header + body,
    "assets/favicon.svg": "<svg/>",
    "assets/logo-light.svg": "<svg/>",
    "assets/logo-dark.svg": "<svg/>",
    ...files,
  })) {
    await mkdir(path.dirname(path.join(root, name)), { recursive: true });
    await writeFile(path.join(root, name), source);
  }
  return root;
}
test("compiles GFM, static snippet imports, JSX links, heading and explicit anchors", async (t) => {
  const root = await fixture(
    t,
    'import Snippet from "/snippets/test.mdx";\n\n# Heading\n\n[local](#heading)\n\n# Heading\n\n[duplicate](#heading-2)\n\n<Snippet />\n\n<Card href="/introduction#custom">Go</Card>\n\n<h2 id="custom">Custom</h2>\n\n| A | B |\n| - | - |\n| 1 | 2 |\n',
    { "snippets/test.mdx": "Sample **text**" },
  );
  assert.deepEqual(await checkDocs(root), { pages: 1, documents: 2 });
});
for (const [name, body, pattern] of [
  ["malformed MDX", "<Card>\n", /./],
  ["missing page", "[bad](/missing)", /ENOENT/],
  ["missing anchor", "[bad](#missing)", /missing anchor/],
  ["missing image", "![bad](/assets/missing.svg)", /ENOENT/],
  ["missing JSX asset", '<img src="/assets/missing.svg" />', /ENOENT/],
  [
    "missing import",
    'import S from "/snippets/missing.mdx";\n\n<S />',
    /ENOENT/,
  ],
  [
    "remote import",
    'import S from "https://example.com/file.mdx";\n\n<S />',
    /local/,
  ],
  ["unknown component", "<NotAComponent />", /unknown component/],
  ["dynamic link", '<Card href={"/introduction"}>Go</Card>', /dynamic href/],
  ["spread attributes", "<Card {...props} />", /spread attributes/],
  ["path traversal", "[bad](../outside.mdx)", /escapes/],
  ["encoded traversal", "[bad](%2e%2e/outside.mdx)", /escapes/],
  ["unsupported scheme", "[bad](javascript:alert)", /unsupported URL/],
  ["duplicate explicit ID", '<div id="x" />\n\n<div id="x" />', /duplicate id/],
  ["unsupported export", "export const x = 1;\n", /static local/],
])
  test(`rejects ${name}`, async (t) => {
    await assert.rejects(checkDocs(await fixture(t, body)), pattern);
  });
test("validates referenced Markdown definitions and relative links", async (t) => {
  const root = await fixture(
    t,
    "[same][ref]\n\n[ref]: ./introduction#heading\n\n# Heading",
  );
  await checkDocs(root);
});
test("rejects YAML duplicate keys", async (t) => {
  const root = await fixture(t);
  await writeFile(
    path.join(root, "introduction.mdx"),
    "---\ntitle: A\ntitle: B\ndescription: C\n---\n",
  );
  await assert.rejects(checkDocs(root), /invalid YAML/);
});
test("rejects missing navigation pages", async (t) => {
  await assert.rejects(
    checkDocs(
      await fixture(t, "", {}, (c) =>
        c.navigation.tabs[0].groups[0].pages.push("missing"),
      ),
    ),
    /missing navigation/,
  );
});
test("rejects unsupported configuration instead of silently ignoring features", async (t) => {
  await assert.rejects(
    checkDocs(await fixture(t, "", {}, (c) => (c.openapi = "schema.json"))),
    /unsupported key/,
  );
});
test("rejects cyclic imports", async (t) => {
  const root = await fixture(t, 'import S from "/snippets/a.mdx";\n\n<S />', {
    "snippets/a.mdx": 'import S from "./b.mdx";\n\n<S />',
    "snippets/b.mdx": 'import S from "./a.mdx";\n\n<S />',
  });
  await assert.rejects(checkDocs(root), /cyclic|imported snippet/);
});
test("rejects assets symlinked outside docs", async (t) => {
  const root = await fixture(t, "![bad](/assets/outside.txt)");
  await symlink("/etc/hostname", path.join(root, "assets/outside.txt"));
  await assert.rejects(checkDocs(root), /escapes/);
});
test("compilation never executes MDX expressions", async (t) => {
  const root = await fixture(
    t,
    '{(() => { throw new Error("must not execute"); })()}',
  );
  await assert.rejects(checkDocs(root), /dynamic MDX expressions/);
});

test("Mintlify-compatible heading conventions retain punctuation and duplicate suffixes", () => {
  assert.equal(
    headingId("0.6.0 · Model-driven database migrations"),
    "0-6-0-·-model-driven-database-migrations",
  );
  assert.equal(headingId("Field_name CamelCase"), "field_name-camelcase");
});
test("does not accept component IDs the hosted renderer drops", async (t) => {
  await assert.rejects(
    checkDocs(await fixture(t, '<Card id="lost">Card</Card>\n\n[bad](#lost)')),
    /rendered anchor/,
  );
});

for (const expression of [
  '{import("/snippets/missing.mdx")}',
  '{true && <a href="/missing">Broken</a>}',
])
  test(`rejects dynamic expression ${expression}`, async (t) => {
    await assert.rejects(
      checkDocs(await fixture(t, expression)),
      /dynamic MDX expressions/,
    );
  });
for (const url of ["#heading#missing", "/introduction%23heading"])
  test(`does not confuse URL delimiters ${url}`, async (t) => {
    await assert.rejects(
      checkDocs(await fixture(t, `# Heading\n\n[broken](${url})`)),
    );
  });
test("external URL availability is explicitly outside offline checks", async (t) => {
  await checkDocs(
    await fixture(
      t,
      "[external](https://does-not-exist.invalid/missing#anchor)",
    ),
  );
});

test("rejects component member expressions rather than treating them as HTML", async (t) => {
  await assert.rejects(
    checkDocs(await fixture(t, "<foo.Bar />")),
    /namespaced/,
  );
});
for (const content of [
  "# Imported heading",
  "[relative](../introduction)",
  '<div id="imported" />',
])
  test(`rejects renderer-dependent imported snippet content ${content}`, async (t) => {
    await assert.rejects(
      checkDocs(
        await fixture(t, 'import S from "/snippets/test.mdx";\n\n<S />', {
          "snippets/test.mdx": content,
        }),
      ),
      /imported snippet/,
    );
  });

test("rejects missing required config members", async (t) => {
  await assert.rejects(
    checkDocs(await fixture(t, "", {}, (c) => (c.colors = {}))),
    /colors.primary/,
  );
});
test("rejects explicit and generated anchor collisions", async (t) => {
  await assert.rejects(
    checkDocs(await fixture(t, '<div id="heading" />\n\n# Heading')),
    /collision/,
  );
});
test("does not accept snippet paths as standalone rendered pages", async (t) => {
  await assert.rejects(
    checkDocs(
      await fixture(t, "[bad](/snippets/hidden)", {
        "snippets/hidden.mdx": "text",
      }),
    ),
    /standalone/,
  );
});

test("does not accept a directory as an image asset", async (t) => {
  await assert.rejects(
    checkDocs(
      await fixture(t, "![bad](/assets/fake.svg)", {
        "assets/fake.svg/file.txt": "text",
      }),
    ),
    /must be a file/,
  );
});

for (const markup of [
  '<img srcSet="/assets/missing.svg 1x" />',
  '<video poster="/assets/missing.svg" />',
  '<form action="/missing" />',
  '<use xlinkHref="/assets/missing.svg#x" />',
  '<div style={{backgroundImage: "url(/missing.svg)"}} />',
]) {
  test(`rejects unsupported URL-bearing attributes ${markup}`, async (t) => {
    await assert.rejects(
      checkDocs(await fixture(t, markup)),
      /unsupported JSX attribute|dynamic JSX/,
    );
  });
}

test("rejects path-based JSX icons rather than skipping asset checks", async (t) => {
  await assert.rejects(
    checkDocs(
      await fixture(t, '<Card icon="/assets/missing.svg">Broken</Card>'),
    ),
    /library icon/,
  );
});
test("rejects URL-shaped navigation icons", async (t) => {
  await assert.rejects(
    checkDocs(
      await fixture(
        t,
        "",
        {},
        (c) => (c.navigation.tabs[0].icon = "https://example.com/icon.svg"),
      ),
    ),
    /library icon/,
  );
});
test("rejects path-based frontmatter icons", async (t) => {
  const root = await fixture(t);
  await writeFile(
    path.join(root, "introduction.mdx"),
    header.replace(
      'title: "Test"',
      'icon: "/assets/missing.svg"\ntitle: "Test"',
    ),
  );
  await assert.rejects(checkDocs(root), /library icon/);
});
