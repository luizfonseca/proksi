import { defineConfig } from 'vite';
import preact from '@preact/preset-vite';

// `npm run dev` serves the UI on :5173 and sends the API to a Proksi running locally
const admin = process.env.PROKSI_ADMIN ?? 'http://127.0.0.1:7555';

export default defineConfig({
  plugins: [preact()],
  build: { outDir: 'dist', assetsDir: 'assets', modulePreload: { polyfill: false } },
  server: { proxy: { '/api': admin, '/metrics': admin } },
});
