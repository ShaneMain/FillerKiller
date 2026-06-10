import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// In dev, proxy /api + /health to the backend so the SPA can call the API
// same-origin (no CORS, cookies work). VITE_PROXY_TARGET points at the backend;
// default is the local Caddy stack (https with a self-signed cert -> secure:false).
// For a bare `cargo run` API, set VITE_PROXY_TARGET=http://localhost:8080.
const target = process.env.VITE_PROXY_TARGET ?? 'https://localhost'

// Only skip TLS verification for a local target (the Caddy stack's self-signed
// cert). If VITE_PROXY_TARGET points at a real remote host, keep verification on
// so dev traffic (incl. the session cookie) can't be silently MITM'd.
const isLocal = /^https?:\/\/(localhost|127\.0\.0\.1)(:|\/|$)/.test(target)

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      '/api': { target, changeOrigin: true, secure: !isLocal },
      '/health': { target, changeOrigin: true, secure: !isLocal },
    },
  },
})
