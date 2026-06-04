import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const dir = dirname(fileURLToPath(import.meta.url));
const load = (f) =>
  JSON.parse(readFileSync(join(dir, "../src/i18n/locales/", f), "utf8"));

const locales = { en: load("en.json"), pl: load("pl.json"), de: load("de.json") };

function flatten(obj, prefix = "") {
  return Object.entries(obj).flatMap(([k, v]) =>
    v && typeof v === "object" && !Array.isArray(v)
      ? flatten(v, `${prefix}${k}.`)
      : [`${prefix}${k}`],
  );
}

const sets = Object.fromEntries(
  Object.entries(locales).map(([lng, obj]) => [lng, new Set(flatten(obj))]),
);

let ok = true;
for (const other of ["pl", "de"]) {
  const missing = [...sets.en].filter((k) => !sets[other].has(k));
  const extra = [...sets[other]].filter((k) => !sets.en.has(k));
  if (missing.length) {
    ok = false;
    console.error(`Missing in ${other} (${missing.length}):`, missing);
  }
  if (extra.length) {
    ok = false;
    console.error(`Extra in ${other} not in en (${extra.length}):`, extra);
  }
}

if (!ok) {
  console.error("i18n key parity FAILED");
  process.exit(1);
}
console.log(`i18n key parity OK (${sets.en.size} keys per locale)`);
