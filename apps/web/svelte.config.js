import path from 'node:path';
import { fileURLToPath } from 'node:url';
import adapter from '@sveltejs/adapter-vercel';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

const rootDir = fileURLToPath(new URL('.', import.meta.url));
const workspaceDir = path.resolve(rootDir, '../..');

/** @type {import('@sveltejs/kit').Config} */
const config = {
	preprocess: vitePreprocess(),
	kit: {
		adapter: adapter(),
		alias: {
			'$api': path.resolve(workspaceDir, 'packages/api_client/src'),
			'$ui': path.resolve(workspaceDir, 'packages/ui/src'),
			'$app-lib': path.resolve(rootDir, 'src/lib')
		}
	}
};

export default config;
