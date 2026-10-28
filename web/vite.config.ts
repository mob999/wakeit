import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwind from '@tailwindcss/vite'
export default defineConfig({
  plugins: [react(), tailwind()],
  server: {
    fs: { allow: ['..'] },
    proxy: Object.fromEntries(
      ['/api', '/health'].map((path) => [
        path,
        {
          target: process.env.WAKEIT_DEV_GATEWAY ?? 'http://127.0.0.1:8080',
          changeOrigin: true,
          ws: true,
          configure(proxy: any) {
            proxy.on('proxyReqWs', (request: any) =>
              request.setHeader(
                'Origin',
                process.env.WAKEIT_DEV_PUBLIC_URL ??
                  process.env.WAKEIT_DEV_GATEWAY ??
                  'http://127.0.0.1:8080',
              ),
            )
            proxy.on('proxyReq', (request: any) =>
              request.setHeader(
                'Origin',
                process.env.WAKEIT_DEV_PUBLIC_URL ??
                  process.env.WAKEIT_DEV_GATEWAY ??
                  'http://127.0.0.1:8080',
              ),
            )
          },
        },
      ]),
    ),
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks: {
          yaml: ['yaml'],
          pinyin: ['pinyin-pro'],
          routing: ['@tanstack/react-router', '@tanstack/react-query'],
          markdown: ['react-markdown', 'remark-gfm'],
        },
      },
    },
  },
})
