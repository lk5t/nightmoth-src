import { defineConfig } from "vite";
import { resolve } from "path";

export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2021",
    outDir: "dist",
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        toast: resolve(__dirname, "toast.html"),
        widgets: resolve(__dirname, "widgets.html"),
      },
    },
  },
});
