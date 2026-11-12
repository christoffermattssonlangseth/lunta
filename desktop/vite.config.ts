import { defineConfig } from 'vite';

// Tauri expects a fixed port and doesn't want Vite to clear its output.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  // WebView2 (Windows) and WebKitGTK (Linux) are modern engines, but not always the newest
  build: { target: ['es2021', 'chrome100', 'safari15'], chunkSizeWarningLimit: 2000 },
});
