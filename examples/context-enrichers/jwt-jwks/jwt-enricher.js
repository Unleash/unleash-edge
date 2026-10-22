const crypto = require("node:crypto");

const jwksUrl = requiredEnv("JWKS_URL");
const expectedIssuer = process.env.JWT_ISSUER;
const expectedAudience = process.env.JWT_AUDIENCE;
const userIdClaim = process.env.JWT_USER_ID_CLAIM || "sub";
const jwtHeader = (process.env.JWT_HEADER || "x-context-jwt").toLowerCase();
const jwksCacheTtlMs = Number(process.env.JWKS_CACHE_TTL_MS || 60000);

let cachedJwks;
let cachedJwksUntil = 0;

module.exports = async function enrich(context, headers) {
  const token = bearerToken(headers[jwtHeader]);
  if (!token) {
    return context;
  }

  const payload = await verifyJwt(token);
  const userId = payload[userIdClaim];

  if (typeof userId !== "string" || userId.length === 0) {
    throw new Error(`JWT claim '${userIdClaim}' is missing or not a string`);
  }

  return {
    ...context,
    userId,
  };
};

function bearerToken(authorization) {
  if (!authorization) {
    return undefined;
  }

  const match = authorization.match(/^Bearer\s+(.+)$/i);
  return match ? match[1] : undefined;
}

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} must be set`);
  }
  return value;
}

async function verifyJwt(token) {
  const parts = token.split(".");
  if (parts.length !== 3) {
    throw new Error("JWT must have three parts");
  }

  const [encodedHeader, encodedPayload, encodedSignature] = parts;
  const header = parseBase64UrlJson(encodedHeader);
  const payload = parseBase64UrlJson(encodedPayload);

  if (header.alg !== "RS256") {
    throw new Error(`Unsupported JWT alg '${header.alg}'`);
  }

  const jwk = await findSigningKey(header.kid);
  const publicKey = crypto.createPublicKey({ key: jwk, format: "jwk" });
  const verifier = crypto.createVerify("RSA-SHA256");
  verifier.update(`${encodedHeader}.${encodedPayload}`);
  verifier.end();

  const signature = Buffer.from(encodedSignature, "base64url");
  if (!verifier.verify(publicKey, signature)) {
    throw new Error("JWT signature verification failed");
  }

  validateClaims(payload);
  return payload;
}

async function findSigningKey(kid) {
  const jwks = await getJwks();
  const jwk = jwks.keys.find((key) => key.kid === kid && key.kty === "RSA");

  if (!jwk) {
    throw new Error(`No RSA signing key found for kid '${kid}'`);
  }

  return jwk;
}

async function getJwks() {
  const now = Date.now();
  if (cachedJwks && cachedJwksUntil > now) {
    return cachedJwks;
  }

  const response = await fetch(jwksUrl);
  if (!response.ok) {
    throw new Error(`JWKS request failed with status ${response.status}`);
  }

  cachedJwks = await response.json();
  cachedJwksUntil = now + jwksCacheTtlMs;
  return cachedJwks;
}

function validateClaims(payload) {
  const nowSeconds = Math.floor(Date.now() / 1000);

  if (payload.exp !== undefined && payload.exp <= nowSeconds) {
    throw new Error("JWT is expired");
  }

  if (payload.nbf !== undefined && payload.nbf > nowSeconds) {
    throw new Error("JWT is not valid yet");
  }

  if (expectedIssuer && payload.iss !== expectedIssuer) {
    throw new Error("JWT issuer does not match");
  }

  if (expectedAudience && !audienceMatches(payload.aud, expectedAudience)) {
    throw new Error("JWT audience does not match");
  }
}

function audienceMatches(actual, expected) {
  return actual === expected || (Array.isArray(actual) && actual.includes(expected));
}

function parseBase64UrlJson(value) {
  return JSON.parse(Buffer.from(value, "base64url").toString("utf8"));
}
