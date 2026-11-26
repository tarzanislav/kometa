import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Výsledok ide do jadro/webui – jadro ho vloží do programu (aplikácia aj `kometa okno`).
export default defineConfig({
  plugins: [svelte()],
  base: './',
  // vývoj: `npm run dev` posiela volania jadru spustenému príkazom `kometa okno --port 8765 --bez-okna`
  server: {
    proxy: { '/api': { target: 'http://127.0.0.1:8765', changeOrigin: true } },
  },
  build: {
    outDir: '../jadro/webui',
    emptyOutDir: true,
    target: 'es2022',
    assetsInlineLimit: 0,
    reportCompressedSize: false,
  },
})
