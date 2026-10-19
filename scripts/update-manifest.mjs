// Writes the latest.json the updater reads (see src-tauri/src/updates.rs):
//   node scripts/update-manifest.mjs <version> <installer url> <signature file> <notes file>
import { readFileSync, writeFileSync } from "node:fs";

const [version, url, sigFile, notesFile] = process.argv.slice(2);
if (!notesFile) {
  console.error("Usage: node scripts/update-manifest.mjs <version> <url> <sig file> <notes file>");
  process.exit(1);
}
const release = { signature: readFileSync(sigFile, "utf8").trim(), url };
const manifest = {
  version,
  notes: readFileSync(notesFile, "utf8").trim(),
  pub_date: new Date().toISOString(),
  platforms: { "windows-x86_64": release, "windows-x86_64-nsis": release },
};
writeFileSync("latest.json", JSON.stringify(manifest, null, 2) + "\n");
console.log(`latest.json for ${version}`);
