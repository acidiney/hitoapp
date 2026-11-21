import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { assetFileNames, stylesheetGuard } from "./vite.assets";
export default defineConfig({
  plugins: [react(), stylesheetGuard],
  build: {
    rollupOptions: {
      input: { app: "src/app/main.tsx", "first-open": "first-open.html" },
      output: {
        entryFileNames: (chunk) =>
          chunk.name === "app" ? "assets/app.js" : "assets/[name]-[hash].js",
        assetFileNames,
        chunkFileNames: "assets/[name]-[hash].js",
      },
    },
  },
});
