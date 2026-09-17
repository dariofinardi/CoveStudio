// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
/// <reference types="vitest/config" />

import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import path from 'node:path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: '127.0.0.1',
    // Allow ?raw imports from the repo root so LicenseSection can pull
    // in LICENSE (AGPL-3.0 text). Production builds inline the file at
    // build time, but dev-server file access is sandboxed by default.
    fs: { allow: ['..'] },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  resolve: {
    alias: {
      $lib: path.resolve(import.meta.dirname, 'src/lib'),
    },
    // Component tests mount Svelte 5 components, so under Vitest resolve
    // the client (browser) build of svelte rather than its server entry.
    conditions: process.env.VITEST ? ['browser'] : undefined,
  },
  build: {
    target: ['es2022', 'chrome120', 'safari17'],
    sourcemap: true,
    outDir: 'dist',
    emptyOutDir: true,
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.{test,spec}.ts'],
  },
})
