import { fork } from 'child_process';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const env = { ...process.env };

function prependPath(currentValue, entry) {
  const entries = (currentValue || '').split(path.delimiter).filter(Boolean);
  return entries.includes(entry)
    ? entries.join(path.delimiter)
    : [entry, ...entries].join(path.delimiter);
}

if (process.platform !== 'win32') {
  const cargoHome = env.CARGO_HOME || (env.HOME && path.join(env.HOME, '.cargo'));
  const cargoBin = cargoHome && path.join(cargoHome, 'bin');

  if (cargoBin && fs.existsSync(path.join(cargoBin, 'cargo'))) {
    env.PATH = prependPath(env.PATH, cargoBin);
  }
}

if (process.platform === 'linux' && env.HOME) {
  const buildTools = env.SIMPLEVOICE_BUILD_TOOLS
    || path.join(env.HOME, '.local', 'share', 'simplevoice-build-tools', 'usr');
  const buildToolsBin = path.join(buildTools, 'bin');
  const buildToolsLib = path.join(buildTools, 'lib');

  if (fs.existsSync(path.join(buildToolsLib, 'libclang.so'))) {
    env.PATH = prependPath(env.PATH, buildToolsBin);
    env.LD_LIBRARY_PATH = prependPath(env.LD_LIBRARY_PATH, buildToolsLib);
    env.CMAKE_PREFIX_PATH = prependPath(env.CMAKE_PREFIX_PATH, buildTools);
    env.LIBCLANG_PATH ||= buildToolsLib;
    env.VULKAN_SDK ||= buildTools;
  }
}

if (process.platform === 'win32') {
  const shortTargetDir = 'C:\\t\\sv';

  if (!fs.existsSync(shortTargetDir)) {
    try {
      fs.mkdirSync(shortTargetDir, { recursive: true });
    } catch (e) {
      console.warn(`[Windows Path Length Fix] Could not create directory ${shortTargetDir}:`, e);
    }
  }

  console.log(`[Windows Path Length Fix] Setting CARGO_TARGET_DIR to: ${shortTargetDir}`);
  console.log(`[Windows Path Length Fix] Disabling MSBuild file tracking (TrackFileAccess=false)`);
  
  env.CARGO_TARGET_DIR = shortTargetDir;
  env.TrackFileAccess = 'false';
  
  // Force CMake and CC builds to use static CRT (MT) to match prebuilt sherpa-onnx-sys
  console.log(`[Windows CRT Fix] Forcing static CRT (MT) linking for C/C++ dependencies`);
  env.CMAKE_MSVC_RUNTIME_LIBRARY = 'MultiThreaded';
  env.CFLAGS = '/MT';
  env.CXXFLAGS = '/MT';
  env.CMAKE_C_FLAGS_RELEASE = '/MT /O2 /Ob2 /DNDEBUG';
  env.CMAKE_CXX_FLAGS_RELEASE = '/MT /O2 /Ob2 /DNDEBUG';
  env.CMAKE_C_FLAGS_RELWITHDEBINFO = '/MT /Zi /O2 /Ob2 /DNDEBUG';
  env.CMAKE_CXX_FLAGS_RELWITHDEBINFO = '/MT /Zi /O2 /Ob2 /DNDEBUG';
  env.CMAKE_C_FLAGS_DEBUG = '/MTd /Ob0 /Od /RTC1';
  env.CMAKE_CXX_FLAGS_DEBUG = '/MTd /Ob0 /Od /RTC1';
}

const cliPath = path.resolve(__dirname, '../node_modules/@tauri-apps/cli/tauri.js');
const args = process.argv.slice(2);

const child = fork(cliPath, args, { env });

child.on('close', (code) => {
  process.exit(code ?? 0);
});
