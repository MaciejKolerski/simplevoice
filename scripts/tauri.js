import { fork } from 'child_process';
import os from 'os';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const env = { ...process.env };

if (process.platform === 'win32') {
  const shortTargetDir = 'C:\\t\\sv';

  // Ensure the directory exists
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
