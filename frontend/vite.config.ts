// vite.config.ts
import { sentryVitePlugin } from "@sentry/vite-plugin";
import { defineConfig, Plugin } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";
import fs from "fs";
import os from "os";

function executorSchemasPlugin(): Plugin {
  const VIRTUAL_ID = "virtual:executor-schemas";
  const RESOLVED_VIRTUAL_ID = "\0" + VIRTUAL_ID;

  return {
    name: "executor-schemas-plugin",
    resolveId(id) {
      if (id === VIRTUAL_ID) return RESOLVED_VIRTUAL_ID; // keep it virtual
      return null;
    },
    load(id) {
      if (id !== RESOLVED_VIRTUAL_ID) return null;

      const schemasDir = path.resolve(__dirname, "../shared/schemas");
      const files = fs.existsSync(schemasDir)
        ? fs.readdirSync(schemasDir).filter((f) => f.endsWith(".json"))
        : [];

      const imports: string[] = [];
      const entries: string[] = [];

      files.forEach((file, i) => {
        const varName = `__schema_${i}`;
        const importPath = `shared/schemas/${file}`; // uses your alias
        const key = file.replace(/\.json$/, "").toUpperCase(); // claude_code -> CLAUDE_CODE
        imports.push(`import ${varName} from "${importPath}";`);
        entries.push(`  "${key}": ${varName}`);
      });

      // IMPORTANT: pure JS (no TS types), and quote keys.
      const code = `
${imports.join("\n")}

export const schemas = {
${entries.join(",\n")}
};

export default schemas;
`;
      return code;
    },
  };
}

// Get all possible hostnames for this machine
function getAllowedHosts() {
  const hosts = new Set([
    'localhost',
    '127.0.0.1',
    '::1',
    '.local',
    '.tailscale.net',
    '.ts.net',
  ]);

  // Add current hostname
  try {
    const hostname = os.hostname();
    if (hostname) {
      hosts.add(hostname);
      hosts.add(hostname.toLowerCase());
      // Also add without domain suffix if present
      const shortName = hostname.split('.')[0];
      if (shortName) {
        hosts.add(shortName);
        hosts.add(shortName.toLowerCase());
      }
    }
  } catch (e) {
    // Ignore errors getting hostname
  }

  // Add any explicitly configured hosts
  if (process.env.VITE_ALLOWED_HOSTS) {
    process.env.VITE_ALLOWED_HOSTS.split(',').forEach(h => hosts.add(h.trim()));
  }

  // Add the HMR host if configured
  if (process.env.VITE_HMR_HOST) {
    hosts.add(process.env.VITE_HMR_HOST);
  }

  return Array.from(hosts);
}

const useStrictHostCheck = process.env.VITE_STRICT_ALLOWED_HOSTS === "true";

export default defineConfig({
  plugins: [
    react(),
    sentryVitePlugin({ org: "bloop-ai", project: "vibe-kanban" }),
    executorSchemasPlugin(),
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      shared: path.resolve(__dirname, "../shared"),
    },
  },
  server: {
    port: parseInt(process.env.FRONTEND_PORT || "3000"),
    // When VITE_HOST is 0.0.0.0, use true to allow all hosts
    host: process.env.VITE_HOST === "0.0.0.0" ? true : (process.env.VITE_HOST || "localhost"),
    // Allow all hosts by default; opt into strict checking via VITE_STRICT_ALLOWED_HOSTS
    allowedHosts: useStrictHostCheck ? getAllowedHosts() : true,
    // Allow all hosts when binding to 0.0.0.0 for network access
    hmr: {
      host: process.env.VITE_HMR_HOST || undefined,
    },
    proxy: {
      "/api": {
        target: `http://${process.env.BACKEND_HOST || "localhost"}:${process.env.BACKEND_PORT || "3001"}`,
        changeOrigin: true,
        ws: true,
      },
    },
    fs: {
      allow: [path.resolve(__dirname, "."), path.resolve(__dirname, "..")],
    },
    open: process.env.VITE_OPEN === "true",
  },
  build: { sourcemap: true },
});
