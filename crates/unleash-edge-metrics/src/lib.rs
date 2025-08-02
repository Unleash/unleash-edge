use lazy_static::lazy_static;
use semver::{Version, VersionReq};
use tracing::trace;

pub mod client_impact_metrics;
pub mod client_metrics;
pub(crate) mod metric_batching;
pub mod metrics_pusher;

const EDGE_REQUIREMENT: &str = ">=17.0.0";
const UNLEASH_REQUIREMENT: &str = ">=5.9.0";

lazy_static! {
    pub static ref EDGE_VERSION_REQ: VersionReq = VersionReq::parse(EDGE_REQUIREMENT).unwrap();
    pub static ref UNLEASH_VERSION_REQ: VersionReq =
        VersionReq::parse(UNLEASH_REQUIREMENT).unwrap();
}

pub fn version_is_new_enough_for_client_bulk(upstream: &str, version: &str) -> bool {
    match upstream {
        "edge" => {
            let edge_version = Version::parse(version).unwrap();
            trace!("Comparing version {version} against Edge requirement of {EDGE_REQUIREMENT}");
            EDGE_VERSION_REQ.matches(&edge_version)
        }
        "unleash" => {
            trace!(
                "Comparing version {version} against Unleash requirement of {UNLEASH_REQUIREMENT}"
            );
            let unleash_version = Version::parse(version).unwrap();
            UNLEASH_VERSION_REQ.matches(&unleash_version)
                || (unleash_version.major == 5
                    && unleash_version.minor == 8
                    && unleash_version.patch == 0
                    && unleash_version.build.contains("main"))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;
    use tracing_test::traced_test;

    #[test_case("unleash", "5.8.0+main", true)]
    #[test_case("unleash", "5.9.0", true)]
    #[test_case("unleash", "5.9.1", true)]
    #[test_case("unleash", "5.10.0", true)]
    #[test_case("unleash", "6.0.0", true)]
    #[test_case("unleash", "5.9.0+main", true)]
    #[test_case("unleash", "5.8.0", false)]
    #[test_case("unleash", "5.8.1", false)]
    #[traced_test]
    pub fn unleash_version_is_new_enough_for_client_bulk(
        upstream: &str,
        version: &str,
        expected: bool,
    ) {
        let result = super::version_is_new_enough_for_client_bulk(upstream, version);
        assert_eq!(result, expected);
    }

    #[test_case("edge", "17.0.0", true)]
    #[test_case("edge", "17.1.0", true)]
    #[test_case("edge", "16.1.0", false)]
    #[test_case("edge", "16.0.0", false)]
    pub fn edge_version_is_new_enough_for_client_bulk(
        upstream: &str,
        version: &str,
        expected: bool,
    ) {
        let result = super::version_is_new_enough_for_client_bulk(upstream, version);
        assert_eq!(result, expected);
    }
}
