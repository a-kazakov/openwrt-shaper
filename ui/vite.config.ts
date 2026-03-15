import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  build: {
    outDir: "../web",
    emptyOutDir: true,
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ["react", "react-dom", "antd", "@ant-design/icons"],
        },
      },
    },
  },
  server: {
    proxy: {
      "/api": "http://localhost:8275",
      "/ws": {
        target: "ws://localhost:8275",
        ws: true,
      },
    },
  },
});
