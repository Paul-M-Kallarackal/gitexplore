import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

const apiTarget = process.env.GITEXPLORE_DEV_API_BASE_URL ?? 'http://127.0.0.1:4000';

export default defineConfig({
  plugins: [react()],
  server: {
    host: '0.0.0.0',
    port: 3000,
    proxy: Object.fromEntries(
      ['/auth', '/graphql', '/health', '/sync', '/bookmarks', '/categories', '/explore'].map(
        (path) => [path, { target: apiTarget, changeOrigin: false }],
      ),
    ),
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    exclude: ['e2e/**', '**/node_modules/**'],
  },
});
