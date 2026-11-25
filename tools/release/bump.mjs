#!/usr/bin/env node
// Sets the app's version everywhere it lives (claude/RELEASING.md):
//   package.json, package-lock.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock, src-tauri/tauri.conf.json
//
//   node tools/release/bump.mjs patch     0.15.0 → 0.15.1   (fixes only)
//   node tools/release/bump.mjs minor     0.15.1 → 0.16.0   (new features, nothing breaks)
//   node tools/release/bump.mjs major     0.16.0 → 1.0.0    (big changes, or something people must relearn)
//   node tools/release/bump.mjs 0.15.0    an exact version
//
// Prints the new version. Changes only the version text, keeping each file's line endings.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const arg = process.argv[2];
const SEMVER = /^(\d+)\.(\d+)\.(\d+)$/;

const read = (p) => readFileSync(join(root, p), "utf8");
const write = (p, s) => writeFileSync(join(root, p), s);
const current = JSON.parse(read("package.json")).version;
const m = SEMVER.exec(current);
if (!m) throw new Error(`package.json has version ${current}, not X.Y.Z`);
const [maj, min, pat] = m.slice(1).map(Number);

let next;
if (arg === "major") next = `${maj + 1}.0.0`;
else if (arg === "minor") next = `${maj}.${min + 1}.0`;
else if (arg === "patch") next = `${maj}.${min}.${pat + 1}`;
else if (arg && SEMVER.test(arg.replace(/^v/, ""))) next = arg.replace(/^v/, "");
else { console.error("Usage: node tools/release/bump.mjs major|minor|patch|X.Y.Z"); process.exit(1); }

const newer = (a, b) => { const x = a.split(".").map(Number), y = b.split(".").map(Number); for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] > y[i]; return false; };
if (!newer(next, current)) { console.error(`${next} is not newer than ${current}: installed copies would never update to it.`); process.exit(1); }

function swap(path, re, what) {
  if (!existsSync(join(root, path))) return;
  const s = read(path);
  if (!re.test(s)) throw new Error(`Couldn't find ${what} in ${path}`);
  write(path, s.replace(re, (_, a, _v, b) => `${a}${next}${b}`));
}
// The first "version" of package.json / tauri.conf.json is the app's own (it comes before any dependency).
swap("package.json", /("version"\s*:\s*")([^"]+)(")/, "version");
swap("src-tauri/tauri.conf.json", /("version"\s*:\s*")([^"]+)(")/, "version");
// package-lock.json: the root's version and packages[""].version (the first two).
if (existsSync(join(root, "package-lock.json"))) {
  let n = 0;
  write("package-lock.json", read("package-lock.json").replace(/("version"\s*:\s*")([^"]+)(")/g, (all, a, _v, b) => (n++ < 2 ? `${a}${next}${b}` : all)));
}
// Cargo.toml: the version line of [package] (the first one in the file).
swap("src-tauri/Cargo.toml", /^(version\s*=\s*")([^"]+)(")/m, "[package] version");
// Cargo.lock: the physiopulse package's entry.
swap("src-tauri/Cargo.lock", /(name = "physiopulse"\r?\nversion = ")([^"]+)(")/, "physiopulse");

console.log(next);
