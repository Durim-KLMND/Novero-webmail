// Sets the app version everywhere it is written down, for a release:
//   node scripts/set-version.mjs 0.2.0-beta.1
// Then commit, and tag the commit `v<version>` (see docs/releasing.md).
import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-beta\.\d+)?$/.test(version ?? "")) {
  console.error("Usage: node scripts/set-version.mjs <x.y.z or x.y.z-beta.n>");
  process.exit(1);
}

function edit(path, change) {
  const before = readFileSync(path, "utf8");
  const after = change(before);
  if (after === before) throw new Error(`no version found in ${path}`);
  writeFileSync(path, after);
}

edit("Cargo.toml", (s) => s.replace(/(\[workspace\.package\]\nversion = )"[^"]*"/, `$1"${version}"`));
edit("apps/desktop/src-tauri/tauri.conf.json", (s) => s.replace(/("version": )"[^"]*"/, `$1"${version}"`));
edit("apps/desktop/package.json", (s) => s.replace(/("version": )"[^"]*"/, `$1"${version}"`));
// The lock file names the package's version twice: at the top and under "".
edit("apps/desktop/package-lock.json", (s) => {
  let n = 0;
  return s.replace(/("version": )"[^"]*"/g, (all, key) => (n++ < 2 ? `${key}"${version}"` : all));
});
execFileSync("cargo", ["update", "--workspace", "--offline"], { stdio: "inherit" });
console.log(`Version is now ${version}.`);
