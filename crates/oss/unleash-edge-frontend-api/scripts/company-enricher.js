module.exports = async function enrichContext(context) {
  const properties = context.properties || {};

  return Object.assign({}, context, {
    properties: Object.assign({}, properties, {
      companyId: "bricks",
    }),
  });
};
