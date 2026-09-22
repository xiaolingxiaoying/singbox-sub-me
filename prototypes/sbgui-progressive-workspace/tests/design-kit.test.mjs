import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const root = path.resolve(import.meta.dirname, "..");
const kit = path.join(root, "design-kit");

test("design kit contains the documented handoff resources", async () => {
  const required = ["README.md", "colors.css", "IconSet.jsx", "icon-manifest.md", "layout.md", "acceptance.md", "assets/serein.ico"];
  await Promise.all(required.map(async file => assert.ok((await stat(path.join(kit, file))).isFile(), `${file} is missing`)));
});

test("design kit documents language and acceptance behavior", async () => {
  const [readme, acceptance] = await Promise.all([readFile(path.join(kit, "README.md"), "utf8"), readFile(path.join(kit, "acceptance.md"), "utf8")]);
  assert.match(readme, /Chinese and English/i);
  assert.match(acceptance, /language button/i);
  assert.match(acceptance, /npm run build/);
});
