import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  server: {
    // During `npm run dev`, forward API calls to the Rust server.
    proxy: { '/api': 'http://127.0.0.1:9100' },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  test: {
    environment: 'jsdom',
  },
})
