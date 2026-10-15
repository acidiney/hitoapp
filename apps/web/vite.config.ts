import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({
  plugins: [react()],
  build: {
    rollupOptions: {
      input: "src/main.tsx",
      output: {
        entryFileNames: "assets/app.js",
        assetFileNames: "assets/app.[ext]",
        chunkFileNames: "assets/[name]-[hash].js",
      },
    },
  },
});
