// Compiles the Icon Composer `.icon` file into a macOS asset catalog (Assets.car)
// so the app shows a fully native Liquid Glass icon on macOS 26+ (system tinting,
// light/dark/clear/tinted appearances). Run automatically as Tauri's macOS
// `beforeBundleCommand`. A fallback `.icns` is produced for older macOS.
//
// Output lands in src-tauri/icons/macos/ and is wired into the bundle via
// `bundle.resources` + `CFBundleIconName` (see tauri.macos.conf.json / Info.plist).

import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(__dirname, '..');

// Name must match `CFBundleIconName` in Info.plist and the bundle product name.
const ICON_NAME = 'Simplevoice';
const MIN_MACOS = '26.0';

const iconSource = path.join(projectRoot, 'Simplevoice.icon');
const outDir = path.join(projectRoot, 'src-tauri', 'icons', 'macos');

if (process.platform !== 'darwin') {
  console.log('[macos-icon] Not macOS — skipping Liquid Glass icon build.');
  process.exit(0);
}

if (!fs.existsSync(iconSource)) {
  console.error(`[macos-icon] Missing icon source: ${iconSource}`);
  process.exit(1);
}

// Resolve actool from the active Xcode toolchain.
let actool = 'actool';
try {
  const dev = execFileSync('xcode-select', ['-p'], { encoding: 'utf8' }).trim();
  const candidate = path.join(dev, 'usr', 'bin', 'actool');
  if (fs.existsSync(candidate)) actool = candidate;
} catch {
  // fall back to PATH lookup
}

fs.mkdirSync(outDir, { recursive: true });

const partialPlist = path.join(outDir, 'partial-icon-info.plist');

console.log(`[macos-icon] Compiling ${path.basename(iconSource)} -> Assets.car`);
execFileSync(
  actool,
  [
    iconSource,
    '--compile', outDir,
    '--app-icon', ICON_NAME,
    '--output-partial-info-plist', partialPlist,
    '--platform', 'macosx',
    '--minimum-deployment-target', MIN_MACOS,
    '--target-device', 'mac',
    '--errors', '--warnings',
  ],
  { stdio: ['ignore', 'inherit', 'inherit'] },
);

const car = path.join(outDir, 'Assets.car');
if (!fs.existsSync(car)) {
  console.error('[macos-icon] actool did not produce Assets.car');
  process.exit(1);
}
console.log(`[macos-icon] Done: ${path.relative(projectRoot, car)}`);
