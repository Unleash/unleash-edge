const ENRICHMENT_URL = "http://127.0.0.1:3210/context";

module.exports = async function enrichContext(context) {
  const response = await fetch(ENRICHMENT_URL, {
    method: "POST",
    headers: {
      "content-type": "application/json",
    },
    body: JSON.stringify(context),
  });

  if (!response.ok) {
    throw new Error(`local enrichment service returned ${response.status}`);
  }

  const enrichment = await response.json();

  return Object.assign({}, context, enrichment, {
    properties: Object.assign(
      {},
      context.properties || {},
      enrichment.properties || {},
    ),
  });
};
