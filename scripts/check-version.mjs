// Fails unless every place that names the app version agrees, and with the
// release tag when one is given: node scripts/check-version.mjs v0.2.0
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const found = {
  "Cargo.toml": read("Cargo.toml").match(/\[workspace\.package\]\nversion = "([^"]*)"/)?.[1],
  "tauri.conf.json": JSON.parse(read("apps/desktop/src-tauri/tauri.conf.json")).version,
  "package.json": JSON.parse(read("apps/desktop/package.json")).version,
  "package-lock.json": JSON.parse(read("apps/desktop/package-lock.json")).version,
};
const tag = process.argv[2];
if (tag) found[`tag ${tag}`] = tag.replace(/^v/, "");

const versions = new Set(Object.values(found));
if (versions.size !== 1) {
  console.error("The version differs between files:", found);
  console.error("Run node scripts/set-version.mjs <version> and commit.");
  process.exit(1);
}
console.log(`Version ${[...versions][0]}`);
