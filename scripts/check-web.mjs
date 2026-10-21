import assert from "node:assert/strict";
import {
  access,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

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
function checkStableReferences(html, version) {
  assert.match(version, /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/);
  assert(
    html.includes(`v${version}`),
    "Homepage must display the stable version",
  );
  assert.equal(
    html.match(/<code>(cargo add rydantic@=[^<]+)<\/code\s*>/)?.[1],
    `cargo add rydantic@=${version}`,
    "Homepage install must use the stable version",
  );
  assert(
    html.includes(`data-copy="cargo add rydantic@=${version}"`),
    "Homepage copy command must use the stable version",
  );
  assert.equal(
    html.match(/<a\s+href="([^"]+)"\s*>\s*Stable API reference<\/a\s*>/)?.[1],
    `https://docs.rs/rydantic/${version}/rydantic/`,
    "Homepage stable API reference must use the stable version",
  );
  assert.equal(
    html.match(/Stable ([^\s]+) includes breaking changes from 0\.6\.0\./)?.[1],
    version,
    "Homepage cumulative upgrade warning must use the stable version",
  );
}
checkStableReferences(html, site.stableVersion);
const docs = site.docs.replace(/\/$/, "");
for (const anchor of html.matchAll(/<a\b[^>]*>/g)) {
  const page = anchor[0].match(/\bdata-docs-path="([^"]+)"/)?.[1];
  if (!page) continue;
  assert(pageSet.has(page), `Missing documentation ${page}`);
  const href = anchor[0].match(/\bhref="([^"]+)"/)?.[1];
  assert.equal(href, `${docs}/${page}`, `Incorrect documentation link for ${page}`);
}
// Exercise the actual builder with a future stable version without changing the
// checkout's verified stable configuration or its generated homepage.
const temporary = await mkdtemp(join(tmpdir(), "rydantic-site-version-"));
try {
  const fixture = pathToFileURL(`${temporary}/`);
  await mkdir(new URL("scripts/", fixture));
  await cp(
    new URL("scripts/build-site.mjs", root),
    new URL("scripts/build-site.mjs", fixture),
  );
  await cp(new URL("website/", root), new URL("website/", fixture), {
    recursive: true,
  });
  await writeFile(
    new URL("website/site.json", fixture),
    JSON.stringify({ ...site, stableVersion: "0.9.0" }),
  );
  await import(new URL("scripts/build-site.mjs", fixture).href);
  checkStableReferences(
    await readFile(new URL(".site/index.html", fixture), "utf8"),
    "0.9.0",
  );
} finally {
  await rm(temporary, { recursive: true, force: true });
}
console.log(
  `Checked ${pages.length} documentation pages, navigation, English content, assets, homepage links and stable-version regression.`,
);
