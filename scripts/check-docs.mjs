import assert from "node:assert/strict";
import { readFile, readdir, realpath, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createProcessor } from "@mdx-js/mdx";
import frontmatter from "remark-frontmatter";
import gfm from "remark-gfm";
import slugify from "@sindresorhus/slugify";
import { parseDocument } from "yaml";

const components = new Set([
  "Accordion",
  "Card",
  "CardGroup",
  "CodeGroup",
  "Info",
  "Note",
  "Tab",
  "Tabs",
  "Warning",
]);
// Match the current Mintlify heading convention for the supported source subset.
// Renderer-specific extensions still require the hosted preview gate.
export function headingId(title) {
  const encoded = encodeURIComponent(
    title.toLowerCase().trim().replace(/\s+/g, "-"),
  );
  const unicode = /%[0-9A-F]{2}/.test(encoded);
  const slug = slugify(encoded, {
    decamelize: false,
    preserveCharacters: unicode ? ["%", "_"] : ["_"],
    lowercase: !unicode,
  });
  return decodeURIComponent(slug.replace(/%(?![0-9A-Fa-f]{2})/g, "%25"))
    .replace(/[?,;:!'"()[\]{}]/g, "")
    .replace(
      /\p{Emoji_Modifier}|\p{Emoji_Modifier_Base}|\p{Emoji_Presentation}|\p{Extended_Pictographic}|[\u200D\uFE0E\uFE0F]/gu,
      "",
    );
}
const fail = (file, message) => {
  throw new Error(`${file}: ${message}`);
};
const text = (node) => node.value ?? (node.children ?? []).map(text).join("");
function walk(node, visit) {
  visit(node);
  for (const child of node.children ?? []) walk(child, visit);
}
function object(value, name) {
  assert(
    value && typeof value === "object" && !Array.isArray(value),
    `${name}: expected object`,
  );
}
function string(value, name) {
  assert(
    typeof value === "string" && value.trim(),
    `${name}: expected nonempty string`,
  );
}
function keys(value, allowed, name) {
  object(value, name);
  for (const key of Object.keys(value))
    assert(
      allowed.includes(key),
      `${name}: unsupported key ${key}; extend validation explicitly`,
    );
}
export function checkConfig(config) {
  keys(
    config,
    [
      "$schema",
      "theme",
      "name",
      "description",
      "colors",
      "favicon",
      "logo",
      "appearance",
      "icons",
      "navbar",
      "navigation",
      "footer",
      "contextual",
      "background",
      "fonts",
      "search",
      "styling",
    ],
    "docs.json",
  );
  assert.equal(config.$schema, "https://mintlify.com/docs.json");
  assert.equal(config.theme, "mint");
  for (const key of ["name", "description", "favicon"])
    string(config[key], key);
  keys(config.colors, ["primary", "light", "dark"], "colors");
  for (const color of Object.values(config.colors))
    assert.match(color, /^#[a-f\d]{6}$/i);
  string(config.colors.primary, "colors.primary");
  assert.equal(config.navbar.primary.type, "button");
  keys(config.logo, ["light", "dark", "href"], "logo");
  for (const key of ["light", "dark", "href"])
    string(config.logo[key], `logo.${key}`);
  keys(config.appearance, ["default"], "appearance");
  assert(["system", "light", "dark"].includes(config.appearance.default));
  keys(config.icons, ["library"], "icons");
  assert.equal(config.icons.library, "lucide");
  keys(config.navigation, ["tabs"], "navigation");
  assert(
    Array.isArray(config.navigation.tabs) && config.navigation.tabs.length,
  );
  const pages = [];
  for (const tab of config.navigation.tabs) {
    keys(tab, ["tab", "icon", "groups"], "tab");
    string(tab.tab, "tab");
    if (tab.icon) string(tab.icon, "tab.icon");
    assert(Array.isArray(tab.groups) && tab.groups.length);
    for (const group of tab.groups) {
      keys(group, ["group", "pages"], "group");
      string(group.group, "group");
      assert(Array.isArray(group.pages) && group.pages.length);
      for (const page of group.pages) {
        string(page, "page");
        assert.match(page, /^[a-z0-9-]+(?:\/[a-z0-9-]+)*$/);
        pages.push(page);
      }
    }
  }
  assert.equal(new Set(pages).size, pages.length, "duplicate navigation page");
  keys(config.navbar, ["links", "primary"], "navbar");
  assert(Array.isArray(config.navbar.links));
  for (const link of [...config.navbar.links, config.navbar.primary]) {
    keys(link, ["label", "href", "type"], "navbar link");
    string(link.label, "label");
    string(link.href, "href");
    if (link.type) assert.equal(link.type, "button");
  }
  keys(config.footer, ["socials"], "footer");
  keys(config.footer.socials, ["github"], "socials");
  string(config.footer.socials.github, "github");
  keys(config.contextual, ["options"], "contextual");
  assert(Array.isArray(config.contextual.options));
  for (const option of config.contextual.options)
    assert(["copy", "view", "chatgpt", "claude"].includes(option));
  keys(config.background, ["color"], "background");
  keys(config.background.color, ["light", "dark"], "background.color");
  for (const color of Object.values(config.background.color))
    assert.match(color, /^#[a-f\d]{6}$/i);
  keys(config.fonts, ["family"], "fonts");
  string(config.fonts.family, "fonts.family");
  keys(config.search, ["prompt"], "search");
  string(config.search.prompt, "search.prompt");
  keys(config.styling, ["eyebrows", "codeblocks"], "styling");
  assert.equal(config.styling.eyebrows, "breadcrumbs");
  assert.equal(config.styling.codeblocks, "system");
  return pages;
}
export async function checkDocs(
  root = fileURLToPath(new URL("../docs/", import.meta.url)),
) {
  root = await realpath(root);
  async function inside(relative) {
    const candidate = path.resolve(root, relative);
    if (candidate !== root && !candidate.startsWith(root + path.sep))
      fail(relative, "path escapes docs root");
    const actual = await realpath(candidate);
    if (actual !== root && !actual.startsWith(root + path.sep))
      fail(relative, "symlink escapes docs root");
    assert(
      (await stat(actual)).isFile(),
      `${relative}: link target must be a file`,
    );
    return actual;
  }
  async function scan(dir = "") {
    const result = [];
    for (const entry of await readdir(path.join(root, dir), {
      withFileTypes: true,
    })) {
      if (entry.name.startsWith(".")) continue;
      const name = path.posix.join(dir, entry.name);
      if (entry.isDirectory()) result.push(...(await scan(name)));
      else if (name.endsWith(".mdx")) result.push(name);
    }
    return result;
  }
  const config = JSON.parse(await readFile(await inside("docs.json"), "utf8"));
  const pages = checkConfig(config),
    files = await scan(),
    documents = new Map();
  for (const page of pages)
    assert(files.includes(page + ".mdx"), `missing navigation page ${page}`);
  for (const file of files) {
    const source = await readFile(await inside(file), "utf8");
    const processor = createProcessor({ remarkPlugins: [frontmatter, gfm] });
    const tree = processor.parse(source);
    await processor.process({ value: source, path: file }); // Compile only; never evaluate MDX or its imports.
    const doc = {
      anchors: new Set(),
      explicit: new Set(),
      links: [],
      imports: [],
      names: new Set(),
      tree,
    };
    const headings = new Map();
    const definitions = new Map();
    walk(tree, (node) => {
      if (node.type === "definition")
        definitions.set(node.identifier, node.url);
    });
    const yaml = tree.children.find((n) => n.type === "yaml");
    if (!file.startsWith("snippets/")) {
      assert(yaml, `${file}: missing frontmatter`);
      const parsed = parseDocument(yaml.value, { uniqueKeys: true });
      assert.equal(parsed.errors.length, 0, `${file}: invalid YAML`);
      const data = parsed.toJS({ maxAliasCount: 20 });
      keys(
        data,
        ["title", "description", "icon", "mode"],
        file + " frontmatter",
      );
      string(data.title, file + " title");
      string(data.description, file + " description");
      if (data.icon !== undefined) string(data.icon, file + " icon");
      if (data.mode !== undefined)
        assert.equal(data.mode, "frame", file + " mode");
    }
    walk(tree, (node) => {
      if (
        ["mdxFlowExpression", "mdxTextExpression"].includes(node.type) &&
        node.data.estree.body.length
      )
        fail(
          file,
          "dynamic MDX expressions are outside the supported source subset",
        );
      if (node.type === "mdxjsEsm") {
        for (const statement of node.data.estree.body) {
          if (
            statement.type !== "ImportDeclaration" ||
            typeof statement.source.value !== "string"
          )
            fail(file, "only static local MDX imports are supported");
          const source = statement.source.value;
          if (
            !source.endsWith(".mdx") ||
            (!source.startsWith("/") && !source.startsWith("."))
          )
            fail(file, "import must be a local .mdx snippet");
          doc.imports.push(source);
          for (const binding of statement.specifiers) {
            if (binding.type !== "ImportDefaultSpecifier")
              fail(file, "only default snippet imports supported");
            doc.names.add(binding.local.name);
          }
        }
      }
      if (node.type === "heading") {
        const base = headingId(text(node));
        let count = (headings.get(base) ?? 0) + 1;
        headings.set(base, count);
        let id = count === 1 ? base : `${base}-${count}`;
        while (doc.anchors.has(id)) {
          assert(
            !doc.explicit.has(id),
            `${file}: generated/explicit anchor collision`,
          );
          id = `${base}-${++count}`;
        }
        doc.anchors.add(id);
      }
      if (node.type === "link" || node.type === "image")
        doc.links.push(node.url);
      if (node.type === "linkReference" || node.type === "imageReference") {
        assert(
          definitions.has(node.identifier),
          `${file}: unresolved reference`,
        );
        doc.links.push(definitions.get(node.identifier));
      }
      if (
        node.type === "mdxJsxFlowElement" ||
        node.type === "mdxJsxTextElement"
      ) {
        for (const attr of node.attributes) {
          if (attr.type !== "mdxJsxAttribute")
            fail(
              file,
              "spread attributes are outside the supported documentation subset",
            );
          if (attr.value?.type === "mdxJsxAttributeValueExpression") {
            const body = attr.value.data.estree.body;
            if (
              body.length !== 1 ||
              body[0].type !== "ExpressionStatement" ||
              body[0].expression.type !== "Literal"
            )
              fail(file, "dynamic JSX attributes are not supported");
          }
          if (["href", "src", "id"].includes(attr.name)) {
            if (typeof attr.value !== "string")
              fail(file, `dynamic ${attr.name} is not supported`);
            if (attr.name === "id") {
              if (
                /^[A-Z]/.test(node.name) &&
                !["Accordion", "Tab"].includes(node.name)
              )
                fail(
                  file,
                  `id on ${node.name} is not supported as a rendered anchor`,
                );
              assert(!doc.anchors.has(attr.value), `${file}: duplicate id`);
              doc.anchors.add(attr.value);
              doc.explicit.add(attr.value);
            } else doc.links.push(attr.value);
          }
        }
      }
    });
    walk(tree, (node) => {
      if (
        ["mdxJsxFlowElement", "mdxJsxTextElement"].includes(node.type) &&
        node.name?.includes(".")
      )
        fail(file, "namespaced JSX components are not supported");
      if (
        ["mdxJsxFlowElement", "mdxJsxTextElement"].includes(node.type) &&
        node.name &&
        /^[A-Z]/.test(node.name)
      )
        assert(
          components.has(node.name) || doc.names.has(node.name),
          `${file}: unknown component ${node.name}`,
        );
    });
    doc.snippetSafe = true;
    walk(tree, (node) => {
      if (
        ![
          "root",
          "code",
          "paragraph",
          "text",
          "emphasis",
          "strong",
          "inlineCode",
          "break",
          "thematicBreak",
          "mdxFlowExpression",
          "mdxTextExpression",
        ].includes(node.type)
      )
        doc.snippetSafe = false;
    });
    documents.set(file, doc);
  }
  async function link(from, url, isImport = false) {
    if (!url || /^(?:https?:|mailto:|tel:)/i.test(url)) return;
    if (url.startsWith("//") || /^[a-z][a-z\d+.-]*:/i.test(url))
      fail(from, `unsupported URL ${url}`);
    // Delimiters are syntax only before decoding. Encoded # belongs to the path.
    const hash = url.indexOf("#");
    const beforeHash = hash < 0 ? url : url.slice(0, hash);
    let pathname, anchor;
    try {
      pathname = decodeURIComponent(beforeHash.split("?")[0]);
      anchor = hash < 0 ? "" : decodeURIComponent(url.slice(hash + 1));
    } catch {
      fail(from, `invalid URL encoding ${url}`);
    }
    let target = pathname
      ? path.posix.normalize(
          pathname.startsWith("/")
            ? pathname.slice(1)
            : path.posix.join(path.posix.dirname(from), pathname),
        )
      : from;
    if (!path.posix.extname(target)) target += ".mdx";
    await inside(target);
    if (isImport) {
      assert(documents.has(target), `${from}: missing MDX import ${url}`);
      return target;
    }
    assert(
      !target.startsWith("snippets/"),
      `${from}: snippet is not a standalone page target`,
    );
    if (anchor)
      assert(
        documents.get(target)?.anchors.has(anchor),
        `${from}: missing anchor ${url}`,
      );
  }
  for (const [file, doc] of documents) {
    doc.targets = [];
    for (const imp of doc.imports) {
      const target = await link(file, imp, true);
      assert(
        target.startsWith("snippets/") && documents.get(target).snippetSafe,
        `${file}: imported snippet must contain only code, plain formatted text and comments; headings, links, JSX and nested imports require explicit rendering support`,
      );
      doc.targets.push(target);
    }
    for (const url of doc.links) await link(file, url);
  }
  const active = new Set(),
    done = new Set();
  function visit(file) {
    if (active.has(file)) fail(file, "cyclic snippet import");
    if (done.has(file)) return;
    active.add(file);
    for (const target of documents.get(file).targets) visit(target);
    active.delete(file);
    done.add(file);
  }
  for (const file of documents.keys()) visit(file);
  for (const url of [
    config.favicon,
    config.logo.light,
    config.logo.dark,
    config.logo.href,
    ...config.navbar.links.map((x) => x.href),
    config.navbar.primary.href,
    config.footer.socials.github,
  ])
    await link("introduction.mdx", url);
  return { pages: pages.length, documents: documents.size };
}
if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const result = await checkDocs();
  console.log(
    `Documentation source checks passed: ${result.pages} navigation pages, ${result.documents} MDX files. Hosted Mintlify rendering is a separate gate.`,
  );
}
