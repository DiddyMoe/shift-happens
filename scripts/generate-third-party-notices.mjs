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

function licenseTexts(directory) {
  return readdirSync(directory)
    .filter((name) => LICENSE_FILE.test(name))
    .map((name) => join(directory, name))
    .filter((path) => statSync(path).isFile())
    .sort()
    .map((path) => readFileSync(path, "utf8").replace(/\r\n/g, "\n").trim());
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
      texts: licenseTexts(dirname(p.manifest_path)),
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
for (const p of packages) {
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
