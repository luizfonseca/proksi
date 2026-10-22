import { defineConfig } from 'vite';
import preact from '@preact/preset-vite';
import tailwindcss from '@tailwindcss/vite';

// `npm run dev` serves the UI on :5173 and sends the API to a Proksi running locally
const admin = process.env.PROKSI_ADMIN ?? 'http://127.0.0.1:7555';

export default defineConfig({
  plugins: [preact(), tailwindcss()],
  build: { outDir: 'dist', assetsDir: 'assets', modulePreload: { polyfill: false } },
  // Host is kept: changes are refused unless their Origin names it (ADR 0008)
  server: {
    proxy: {
      '/api': { target: admin, changeOrigin: false },
      '/metrics': { target: admin, changeOrigin: false },
    },
  },
});
