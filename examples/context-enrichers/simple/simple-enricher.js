module.exports = async function enrich(context, headers) {
  return {
    ...context,
    userId: headers["x-user-id"] ?? context.userId,
    properties: {
      ...(context.properties || {})
    },
  };
};
