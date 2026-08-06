const http = require("http");

module.exports = async function enrichContext(context) {
  const enrichment = await postJsonToLocalService(context);

  return Object.assign({}, context, enrichment, {
    properties: Object.assign(
      {},
      context.properties || {},
      enrichment.properties || {},
    ),
  });
};

function postJsonToLocalService(context) {
  const body = JSON.stringify(context);

  return new Promise((resolve, reject) => {
    const request = http.request(
      {
        hostname: "127.0.0.1",
        port: 3210,
        path: "/context",
        method: "POST",
        headers: {
          "content-type": "application/json",
          "content-length": Buffer.byteLength(body),
        },
      },
      (response) => {
        let responseBody = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => {
          responseBody += chunk;
        });
        response.on("end", () => {
          if (response.statusCode < 200 || response.statusCode >= 300) {
            reject(
              new Error(
                `local enrichment service returned ${response.statusCode}`,
              ),
            );
            return;
          }

          try {
            resolve(JSON.parse(responseBody || "{}"));
          } catch (error) {
            reject(error);
          }
        });
      },
    );

    request.on("error", reject);
    request.write(body);
    request.end();
  });
}
