import { readFile, writeFile } from "node:fs/promises";

// package.json owns the version; npm's version lifecycle updates Cargo with it.
const manifest = JSON.parse(
  await readFile(new URL("../package.json", import.meta.url), "utf8"),
);
if (
  typeof manifest.version !== "string" ||
  !/^\d+\.\d+\.\d+$/.test(manifest.version)
) {
  throw new Error("The application version must use major.minor.patch format.");
}
const cargoPath = new URL("../src-tauri/Cargo.toml", import.meta.url);
const cargo = await readFile(cargoPath, "utf8");
if (!/^version = "\d+\.\d+\.\d+"$/m.test(cargo)) {
  throw new Error(
    "The Cargo application version field is missing or malformed.",
  );
}
const updated = cargo.replace(
  /^version = "\d+\.\d+\.\d+"$/m,
  `version = "${manifest.version}"`,
);
const cargoLockPath = new URL("../src-tauri/Cargo.lock", import.meta.url);
const cargoLock = await readFile(cargoLockPath, "utf8");
const applicationEntry =
  /(\[\[package\]\]\nname = "shift-happens"\nversion = ")[^"]+("\n)/;
if (!applicationEntry.test(cargoLock)) {
  throw new Error("The application entry is missing from Cargo.lock.");
}
const updatedLock = cargoLock.replace(
  applicationEntry,
  (_match, prefix, suffix) => `${prefix}${manifest.version}${suffix}`,
);
if (updated !== cargo) await writeFile(cargoPath, updated, "utf8");
if (updatedLock !== cargoLock)
  await writeFile(cargoLockPath, updatedLock, "utf8");
