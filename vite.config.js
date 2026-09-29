import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

// 开发时前端跑在 5173，接口代理到本机转换服务（8765）。
// 打包后由 Rust 服务直接托管 dist/，不需要 Node。
export default defineConfig({
  plugins: [vue()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8765',
        changeOrigin: false,
        ws: false
      }
    }
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'chrome110',
    assetsInlineLimit: 0
  }
});
