import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// The window of the app keeps files from an earlier dev server, and asks with `If-None-Match` if they changed. Then
// Vite answers "304, not changed", and this server does not compile the component. The CSS of that component is then
// missing ("failed to load virtual css module"). This plugin removes the question, so the server always compiles.
//
// WebKit also keeps a list of the files of a page. On the next start, it can ask for the CSS of a component before it
// asks for the component. Vite then sends the full source file as the CSS, and keeps that wrong answer. Thus, this
// plugin compiles the component first when a request for its CSS arrives.
const alwaysCompile: Plugin = {
  name: "panim-always-compile",
  configureServer(server) {
    server.middlewares.use((req, _res, next) => {
      delete (req as { headers: Record<string, unknown> }).headers["if-none-match"];
      const [path, query] = (req.url ?? "").split("?");
      if (!path.endsWith(".svelte") || !query?.includes("type=style")) return next();
      server
        .transformRequest(path)
        .catch(() => undefined)
        .then(() => next());
    });
  },
};

// Tauri loads the dev server on a fixed port, and the build output from dist/.
export default defineConfig({
  plugins: [alwaysCompile, svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true, headers: { "Cache-Control": "no-store" } },
  build: { target: "safari16", outDir: "dist", emptyOutDir: true },
});
