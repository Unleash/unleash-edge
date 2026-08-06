module.exports = function enrichContext(context) {
  return Object.assign({}, context, {
    userId: "7",
  });
};
