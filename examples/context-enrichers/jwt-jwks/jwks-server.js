const http = require("node:http");
const crypto = require("node:crypto");

const issuer = process.env.JWT_ISSUER || "edge-context-enricher-example";
const audience = process.env.JWT_AUDIENCE || "unleash-edge";
const kid = "example-key-1";
const port = Number(process.env.PORT || 8080);

const { publicKey, privateKey } = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
});

const publicJwk = publicKey.export({ format: "jwk" });
const jwks = {
  keys: [
    {
      ...publicJwk,
      kid,
      alg: "RS256",
      use: "sig",
    },
  ],
};

const server = http.createServer((request, response) => {
  const url = new URL(request.url, `http://${request.headers.host}`);

  if (request.method === "GET" && url.pathname === "/.well-known/jwks.json") {
    return json(response, 200, jwks);
  }

  if (request.method === "GET" && url.pathname === "/token") {
    const userId = url.searchParams.get("userId") || "jwks-user";
    return json(response, 200, {
      token: signJwt({
        iss: issuer,
        aud: audience,
        sub: userId,
        iat: nowSeconds(),
        exp: nowSeconds() + 3600,
      }),
    });
  }

  json(response, 404, { error: "not found" });
});

server.listen(port, "0.0.0.0", () => {
  console.log(`JWKS mock listening on ${port}`);
});

function signJwt(payload) {
  const encodedHeader = base64urlJson({ alg: "RS256", typ: "JWT", kid });
  const encodedPayload = base64urlJson(payload);
  const signer = crypto.createSign("RSA-SHA256");
  signer.update(`${encodedHeader}.${encodedPayload}`);
  signer.end();
  const signature = signer.sign(privateKey).toString("base64url");
  return `${encodedHeader}.${encodedPayload}.${signature}`;
}

function base64urlJson(value) {
  return Buffer.from(JSON.stringify(value)).toString("base64url");
}

function nowSeconds() {
  return Math.floor(Date.now() / 1000);
}

function json(response, status, body) {
  response.writeHead(status, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
}
