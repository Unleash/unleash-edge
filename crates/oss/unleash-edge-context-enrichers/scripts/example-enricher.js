module.exports = async function enrichContext(context) {
  const properties = context.properties || {};

  if (properties.exit === true) {
    process.exit(42);
  }

  if (properties.log === true) {
    console.log("enriching", context.userId, { delayMs: properties.delayMs });
  }

  const delayMs = Number(properties.delayMs || 0);

  await new Promise((resolve) => {
    setTimeout(resolve, delayMs);
  });

  return Object.assign({}, context, {
    properties: Object.assign({}, properties, {
      enriched: true,
      completedAfterMs: delayMs,
    }),
  });
};
