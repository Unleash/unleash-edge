module.exports = async function enrichContext(context) {
  const properties = context.properties || {};

  if (properties.block === true) {
    while (true) {
      // Deliberately blocks the Node event loop for manual experimentation.
    }
  }

  if (properties.hang === true) {
    await new Promise(() => {});
  }

  return Object.assign({}, context, {
    properties: Object.assign({}, properties, {
      enriched: true,
    }),
  });
};
