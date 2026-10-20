import { readFile, mkdir, rm, cp, writeFile } from "node:fs/promises";

const root = new URL("../", import.meta.url);
const site = JSON.parse(
  await readFile(new URL("website/site.json", root), "utf8"),
);
const manifest = await readFile(new URL("Cargo.toml", root), "utf8");
const version = manifest.match(
  /\[workspace\.package\]\s*\nversion\s*=\s*"([^"]+)"/,
)[1];
const output = new URL(".site/", root);
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
for (const file of ["styles.css", "app.js", "favicon.svg"]) {
  await cp(new URL(`website/${file}`, root), new URL(file, output));
}
let html = await readFile(new URL("website/index.html", root), "utf8");
const values = {
  SITE_URL: site.url,
  DOCS_URL: site.docs.replace(/\/$/, ""),
  REPO_URL: site.repository,
  VERSION: version,
};
for (const [name, value] of Object.entries(values))
  html = html.replaceAll(`{{${name}}}`, value);
await writeFile(new URL("index.html", output), html);
await writeFile(new URL(".nojekyll", output), "");
await writeFile(
  new URL("robots.txt", output),
  `User-agent: *\nAllow: /\nSitemap: ${site.url}sitemap.xml\n`,
);
await writeFile(
  new URL("sitemap.xml", output),
  `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>${site.url}</loc></url></urlset>\n`,
);
console.log(`Built Rydantic ${version} homepage in .site/`);
