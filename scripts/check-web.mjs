import assert from "node:assert/strict";
import { access, readFile, readdir } from "node:fs/promises";

const root = new URL("../", import.meta.url);
const config = JSON.parse(
  await readFile(new URL("docs/docs.json", root), "utf8"),
);
const pages = config.navigation.tabs.flatMap((tab) =>
  tab.groups.flatMap((group) => group.pages),
);
assert.equal(
  new Set(pages).size,
  pages.length,
  "Navigation has duplicate pages",
);
const pageSet = new Set(pages);
for (const page of pages) {
  const source = await readFile(new URL(`docs/${page}.mdx`, root), "utf8");
  assert.match(
    source,
    /^---\ntitle: ".+"\ndescription: ".+"\n/,
    `${page}: frontmatter missing`,
  );
  assert.doesNotMatch(
    source,
    /[\u3400-\u9fff]/u,
    `${page}: documentation must be English`,
  );
  assert.equal(
    [...source.matchAll(/^```/gm)].length % 2,
    0,
    `${page}: unclosed code block`,
  );
  for (const link of source.matchAll(
    /(?:href="|\]\()\/(?!\/)([^"\s)#]+)(?:#[^"\s)]+)?/g,
  )) {
    const path = link[1];
    if (path.startsWith("assets/")) await access(new URL(`docs/${path}`, root));
    else assert(pageSet.has(path), `${page}: unlisted internal link /${path}`);
  }
}
for (const image of [config.favicon, config.logo.light, config.logo.dark]) {
  await access(new URL(`docs${image}`, root));
}
const files = await readdir(new URL(".site/", root));
const html = await readFile(new URL(".site/index.html", root), "utf8");
assert.doesNotMatch(
  html,
  /\{\{\w+\}\}/,
  "Homepage has unresolved template variables",
);
const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
assert.equal(ids.length, new Set(ids).size, "Homepage has duplicate IDs");
for (const link of html.matchAll(/(?:href|src)="([^"#]+|#[^"]+)"/g)) {
  const target = link[1];
  if (target.startsWith("#"))
    assert(ids.includes(target.slice(1)), `Missing anchor ${target}`);
  else if (target.startsWith("./") && target !== "./")
    assert(files.includes(target.slice(2)), `Missing asset ${target}`);
}
const site = JSON.parse(await readFile(new URL("website/site.json", root), "utf8"));
const docs = site.docs.replace(/\/$/, "");
for (const anchor of html.matchAll(/<a\b[^>]*>/g)) {
  const page = anchor[0].match(/\bdata-docs-path="([^"]+)"/)?.[1];
  if (!page) continue;
  assert(pageSet.has(page), `Missing documentation ${page}`);
  const href = anchor[0].match(/\bhref="([^"]+)"/)?.[1];
  assert.equal(href, `${docs}/${page}`, `Incorrect documentation link for ${page}`);
}
console.log(
  `Checked ${pages.length} documentation pages, navigation, English content, assets and homepage links.`,
);
