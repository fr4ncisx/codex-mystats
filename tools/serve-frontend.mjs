import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../frontend");
const rootPrefix = root.endsWith(path.sep) ? root : root + path.sep;
const contentTypes = new Map([
  [".css", "text/css; charset=utf-8"],
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".svg", "image/svg+xml"],
]);

const server = createServer(async (request, response) => {
  if (request.method !== "GET" && request.method !== "HEAD") {
    response.writeHead(405, { Allow: "GET, HEAD" }).end();
    return;
  }

  let pathname;
  try {
    pathname = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
  } catch {
    response.writeHead(400).end();
    return;
  }
  if (pathname === "/") pathname = "/index.html";
  if (pathname === "/__test_shutdown") {
    response.once("finish", shutdownServer);
    response.writeHead(204).end();
    return;
  }
  if (pathname === "/favicon.ico") {
    response.writeHead(204).end();
    return;
  }

  const filePath = path.resolve(root, "." + pathname);
  if (!filePath.startsWith(rootPrefix)) {
    response.writeHead(404).end();
    return;
  }

  try {
    const contents = await readFile(filePath);
    response.writeHead(200, {
      "Cache-Control": "no-store",
      "Content-Length": contents.length,
      "Content-Type": contentTypes.get(path.extname(filePath)) ?? "application/octet-stream",
      "X-Content-Type-Options": "nosniff",
    });
    response.end(request.method === "HEAD" ? undefined : contents);
  } catch {
    response.writeHead(404).end();
  }
});

server.listen(4173, "127.0.0.1");
server.keepAliveTimeout = 500;

function shutdownServer() {
  server.close(() => process.exit(0));
  server.closeAllConnections();
  const forcedExit = setTimeout(() => process.exit(0), 1_000);
  forcedExit.unref();
}

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, shutdownServer);
}
