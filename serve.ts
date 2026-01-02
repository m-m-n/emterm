// Development server for eMterm frontend

const server = Bun.serve({
  port: 5173,
  async fetch(req) {
    const url = new URL(req.url);
    let path = url.pathname;

    if (path === '/') {
      path = '/index.html';
    }

    const filePath = `./src${path}`;
    const file = Bun.file(filePath);

    if (await file.exists()) {
      const contentType = getContentType(path);

      // For TypeScript files, transpile on the fly
      if (path.endsWith('.ts')) {
        const transpiler = new Bun.Transpiler({
          loader: 'ts',
        });
        const code = await file.text();
        const result = transpiler.transformSync(code);
        return new Response(result, {
          headers: { 'Content-Type': 'application/javascript' },
        });
      }

      return new Response(file, {
        headers: { 'Content-Type': contentType },
      });
    }

    return new Response('Not Found', { status: 404 });
  },
});

function getContentType(path: string): string {
  if (path.endsWith('.html')) return 'text/html';
  if (path.endsWith('.css')) return 'text/css';
  if (path.endsWith('.js')) return 'application/javascript';
  if (path.endsWith('.ts')) return 'application/javascript';
  if (path.endsWith('.json')) return 'application/json';
  return 'application/octet-stream';
}

console.log(`Development server running at http://localhost:${server.port}`);
