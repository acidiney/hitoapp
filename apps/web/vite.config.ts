import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { assetFileNames } from "./vite.assets";
export default defineConfig({
  plugins: [react()],
  build: {
    rollupOptions: {
      input: "src/app/main.tsx",
      output: {
        entryFileNames: "assets/app.js",
        assetFileNames,
        chunkFileNames: "assets/[name]-[hash].js",
      },
    },
  },
});
