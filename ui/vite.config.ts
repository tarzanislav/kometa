import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Výsledok ide priamo do balíka kometa/webui – Python ho otvorí v okne aplikácie.
export default defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    outDir: '../kometa/webui',
    emptyOutDir: true,
    target: 'es2022',
    assetsInlineLimit: 0,
    reportCompressedSize: false,
  },
})
