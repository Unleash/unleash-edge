const semver = require("semver");

const versionHeader = (process.env.VERSION_HEADER || "x-application-version").toLowerCase();

module.exports = async function enrich(context, headers) {
  const version = semver.parse(headers[versionHeader]);
  if (!version) {
    return context;
  }

  return {
    ...context,
    properties: {
      ...(context.properties || {}),
      releaseChannel: version.prerelease.length > 0 ? "experimental" : "stable",
    },
  };
};
