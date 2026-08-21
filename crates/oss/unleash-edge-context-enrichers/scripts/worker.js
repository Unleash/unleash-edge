const path = require("path");
const readline = require("readline");

const originalConsole = {
  log: console.log.bind(console),
  info: console.info.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  debug: console.debug.bind(console),
};

function formatConsolePart(part) {
  if (typeof part === "string") {
    return part;
  }

  if (part instanceof Error) {
    return part.stack || part.message;
  }

  try {
    return JSON.stringify(part);
  } catch (_error) {
    return String(part);
  }
}

function redirectConsole(method, parts) {
  const message = parts.map(formatConsolePart).join(" ");
  process.stderr.write(`[customer console.${method}] ${message}\n`);
}

console.log = (...parts) => redirectConsole("log", parts);
console.info = (...parts) => redirectConsole("info", parts);
console.warn = (...parts) => redirectConsole("warn", parts);
console.error = (...parts) => redirectConsole("error", parts);
console.debug = (...parts) => redirectConsole("debug", parts);

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function failStartup(message) {
  originalConsole.error(message);
  process.exit(1);
}

const scriptPath = process.argv[2];

if (!scriptPath) {
  failStartup("missing customer script path");
}

let enrichContext;

try {
  const resolvedScriptPath = path.resolve(scriptPath);
  enrichContext = require(resolvedScriptPath);
} catch (error) {
  failStartup(
    `failed to load customer script: ${
      error instanceof Error ? error.message : String(error)
    }`,
  );
}

if (typeof enrichContext !== "function") {
  failStartup("customer script must export a function");
}

send({ type: "ready" });

async function handle(message) {
  try {
    const result = await enrichContext(message.context);
    send({ id: message.id, result });
  } catch (error) {
    send({
      id: message.id,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

const input = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

input.on("line", (line) => {
  if (line.trim() === "") {
    return;
  }

  let message;
  try {
    message = JSON.parse(line);
  } catch (error) {
    console.error(
      `invalid JSON request: ${
        error instanceof Error ? error.message : String(error)
      }`,
    );
    return;
  }

  void handle(message);
});
