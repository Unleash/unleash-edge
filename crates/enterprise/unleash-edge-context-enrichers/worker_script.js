const path = require("path");
const readline = require("readline");
const protocolWrite = process.stdout.write.bind(process.stdout);

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

function toStdErr(method, parts) {
    const message = parts.map(formatConsolePart).join(" ");
    process.stderr.write(`[console.${method}] ${message}\n`);
}

function send(message) {
    protocolWrite(`${JSON.stringify(message)}\n`);
}

// We use stdout for communication with the parent process, so anything that touches that will
// break the protocol. Easy enough to just capture console output and redirect it to stderr instead.
// The Rust worker runner expects us to do this and pipes it all out to logs
function captureConsole() {
    console.log = (...parts) => toStdErr("log", parts);
    console.info = (...parts) => toStdErr("info", parts);
    console.warn = (...parts) => toStdErr("warn", parts);
    console.error = (...parts) => toStdErr("error", parts);
    console.debug = (...parts) => toStdErr("debug", parts);
}

function captureProcessStdout() {
    process.stdout.write = (...parts) => {
        const message = parts.map(formatConsolePart).join(" ");
        process.stderr.write(`[process.stdout.write] ${message}\n`);
        return true;
    };
}

async function handle(message, enrich) {
    try {
        const result = await enrich(message.context, message.headers || {});
        send({ id: message.id, context: result });
    } catch (error) {
        send({
            id: message.id,
            error: error instanceof Error ? error.message : String(error),
        });
    }
}

function loadEnricherScript(args) {
    const index = args.indexOf("--enricher-script");
    if (index === -1 || args.length <= index + 1) {
        throw new Error("enricher script path not provided");
    }

    const enricherScriptPath = args[index + 1];
    const resolvedScriptPath = path.resolve(enricherScriptPath);
    return require(resolvedScriptPath);
}

function setupMessageHandler(enricherScript) {
    const input = readline.createInterface({
        input: process.stdin,
        crlfDelay: Infinity,
    });

    input.on("line", (line) => {
        if (line.trim() === "") {
            return;
        }

        try {
            const message = JSON.parse(line);
            // awaiting here would block our message handler and prevent inputs from the parent process being read.
            // When the task to process the message is complete, we'll write it back over stdout so it can be fire and forget
            void handle(message, enricherScript);
        } catch (error) {
            console.error(
                `invalid JSON request: ${error instanceof Error ? error.message : String(error)}`,
            );
        }
    });
}

function main() {
    try {
        captureConsole();
        captureProcessStdout();
        const enricherScript = loadEnricherScript(process.argv);
        if (typeof enricherScript !== "function") {
            throw new Error("enricher script must export a function");
        }
        setupMessageHandler(enricherScript);
        send({ messageType: "ready" });

    } catch (error) {
        console.error(
            `failed to setup reader: ${error instanceof Error ? error.message : String(error)
            }`,
        );
        process.exit(1);
    }
}

main();
