// Writes src/generated/third-party-notices.txt: the license notices of every
// third-party package built into the Windows app. Rust crates come from the
// resolved dependency graph for the Windows target (normal dependencies only;
// build and dev tools are not shipped); npm packages from the production
// entries of package-lock.json. Packages with identical license text share
// one copy of it. Run after changing dependencies: npm run notices
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
// About loads this file on demand as its own bundled chunk.
const output = join(root, "src", "generated", "third-party-notices.txt");
const LICENSE_FILE = /^(licen[cs]e|copying|notice)/i;
// Some published packages omit the license files their repository carries.
// These are reviewed upstream copies, each kept under scripts/notice-supplements
// at its https://raw.githubusercontent.com/ path, pinned to the commit that exact
// version was published from (its .cargo_vcs_info.json). A new version needs a
// new review.
const supplements = join(root, "scripts", "notice-supplements");
const SUPPLEMENTS = new Map(
  Object.entries({
    "alloc-stdlib 0.2.4": [
      "dropbox/rust-alloc-no-stdlib/ae42d22078b98549e987d2f03d12df7b984fde47/LICENSE",
    ],
    "alloc-stdlib 0.3.0": [
      "dropbox/rust-alloc-no-stdlib/0a81fd6928ea3b33c8cd484aa4575d50ffb98012/LICENSE",
    ],
    "defmt-parser 1.0.0": [
      "knurling-rs/defmt/4a8cdb44891ed57b8ff5a023b6bec7137c48708f/LICENSE-APACHE",
      "knurling-rs/defmt/4a8cdb44891ed57b8ff5a023b6bec7137c48708f/LICENSE-MIT",
    ],
    "webview2-com 0.39.1": [
      "wravery/webview2-rs/edc2caf886175ccaebe86078c9cfe1ae2a187328/LICENSE",
    ],
    "webview2-com-sys 0.39.1": [
      "wravery/webview2-rs/edc2caf886175ccaebe86078c9cfe1ae2a187328/LICENSE",
    ],
    "webview2-com-macros 0.8.1": [
      "wravery/webview2-rs/dffa41a8a46d3f5565eefbff2de57d38d399f158/LICENSE",
    ],
  }).map(([key, paths]) => [key, paths.map((path) => join(supplements, path))]),
);

const readText = (path) =>
  readFileSync(path, "utf8").replace(/\r\n/g, "\n").trim();

// A matching directory is a REUSE-style LICENSES folder of license files.
function licenseTexts(directory) {
  return readdirSync(directory)
    .filter((name) => LICENSE_FILE.test(name))
    .map((name) => join(directory, name))
    .flatMap((path) =>
      statSync(path).isDirectory()
        ? readdirSync(path)
            .map((name) => join(path, name))
            .filter((file) => statSync(file).isFile())
        : statSync(path).isFile()
          ? [path]
          : [],
    )
    .sort()
    .map(readText);
}

function packageTexts(name, version, directory) {
  const shipped = licenseTexts(directory);
  if (shipped.length) return shipped;
  return (SUPPLEMENTS.get(`${name} ${version}`) ?? []).map(readText);
}

function rustPackages() {
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      [
        "metadata",
        "--manifest-path",
        join(root, "src-tauri", "Cargo.toml"),
        "--format-version",
        "1",
        "--filter-platform",
        "x86_64-pc-windows-msvc",
        "--locked",
      ],
      { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
    ),
  );
  const packages = new Map(metadata.packages.map((p) => [p.id, p]));
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const shipped = new Set();
  const pending = [metadata.resolve.root];
  while (pending.length) {
    const id = pending.pop();
    if (shipped.has(id)) continue;
    shipped.add(id);
    for (const dep of nodes.get(id).deps)
      if (dep.dep_kinds.some((kind) => kind.kind === null))
        pending.push(dep.pkg);
  }
  shipped.delete(metadata.resolve.root);
  return [...shipped].map((id) => {
    const p = packages.get(id);
    return {
      name: p.name,
      version: p.version,
      license: p.license ?? "See package",
      source: p.repository ?? "",
      texts: packageTexts(p.name, p.version, dirname(p.manifest_path)),
    };
  });
}

function npmPackages() {
  const lock = JSON.parse(
    readFileSync(join(root, "package-lock.json"), "utf8"),
  );
  return Object.entries(lock.packages)
    .filter(([path, entry]) => path && !entry.dev)
    .map(([path]) => {
      const directory = join(root, path);
      const manifest = JSON.parse(
        readFileSync(join(directory, "package.json"), "utf8"),
      );
      const repository =
        typeof manifest.repository === "string"
          ? manifest.repository
          : (manifest.repository?.url ?? "");
      return {
        name: manifest.name,
        version: manifest.version,
        license: manifest.license ?? "See package",
        source: repository,
        texts: licenseTexts(directory),
      };
    });
}

const packages = [...npmPackages(), ...rustPackages()].sort(
  (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version),
);
const groups = new Map();
const unlicensed = [];
for (const p of packages) {
  if (!p.texts.length) unlicensed.push(`${p.name} ${p.version}`);
  const text = p.texts.length
    ? p.texts.join("\n\n")
    : `Licensed under ${p.license}. The package ships no separate license file${p.source ? `; see ${p.source}` : ""}.`;
  if (!groups.has(text)) groups.set(text, []);
  groups.get(text).push(p);
}

const rule = "=".repeat(78);
const sections = [...groups].map(
  ([text, members]) =>
    `${rule}\n${members.map((p) => `${p.name} ${p.version} (${p.license})`).join("\n")}\n${rule}\n\n${text}\n`,
);
writeFileSync(
  output,
  `Shift Happens third-party notices\n\nShift Happens includes the following ${packages.length} open-source packages. Each is used under the license reproduced below it.\n\n${sections.join("\n")}`,
);
console.log(
  `Wrote ${output}: ${packages.length} packages, ${groups.size} distinct license texts.`,
);
if (unlicensed.length)
  console.warn(
    `No license file found, so only the declared license is named, for: ${unlicensed.join(", ")}. Review the upstream repository and add a supplement if it has one.`,
  );
