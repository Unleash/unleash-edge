module.exports = async function enrichContext(context) {
  await new Promise((resolve) => {
    setTimeout(resolve, 150);
  });

  return Object.assign({}, context, {
    properties: Object.assign({}, context.properties || {}, {
      delayed: "true",
    }),
  });
};
