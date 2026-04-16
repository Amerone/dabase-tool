import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          const moduleId = id.replace(/\\/g, '/')

          if (
            moduleId.includes('node_modules/react') ||
            moduleId.includes('node_modules/react-dom') ||
            moduleId.includes('node_modules/react-router-dom')
          ) {
            return 'vendor-react'
          }
          if (
            moduleId.includes('node_modules/@tanstack/react-query') ||
            moduleId.includes('node_modules/axios')
          ) {
            return 'vendor-query'
          }
          if (moduleId.includes('node_modules/animejs')) {
            return 'vendor-motion'
          }
          return undefined
        },
      },
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:3000',
        changeOrigin: true,
      },
    },
  },
})
