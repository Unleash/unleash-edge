const http = require("http");

const server = http.createServer((request, response) => {
  if (request.method !== "POST" || request.url !== "/context") {
    response.writeHead(404);
    response.end();
    return;
  }

  let body = "";
  request.setEncoding("utf8");
  request.on("data", (chunk) => {
    body += chunk;
  });
  request.on("end", () => {
    const context = JSON.parse(body || "{}");
    const userId = context.userId || "anonymous";

    response.writeHead(200, {
      "content-type": "application/json",
    });
    response.end(
      JSON.stringify({
        properties: {
          localUserSegment: userId === "7" ? "known" : "unknown",
        },
      }),
    );
  });
});

server.listen(3210, "127.0.0.1", () => {
  console.log("context enrichment service listening on http://127.0.0.1:3210/context");
});
