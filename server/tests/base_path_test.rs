#[cfg(test)]
mod base_path_tests {
    use reqwest::Client;
    use std::process::{Command, Stdio};
    use unleash_edge::types::BuildInfo;

    #[actix_web::test]
    async fn test_base_path() {
        let base_path = "/nuno/test";

        // Run the app as a separate process
        let mut app_process = Command::new("./../target/debug/unleash-edge")
            .arg("-b")
            .arg(&base_path)
            .arg("edge")
            .arg("-u")
            .arg("http://localhost:4242")
            .stdout(Stdio::null()) // Suppress stdout
            .stderr(Stdio::null()) // Suppress stderr
            .spawn()
            .expect("Failed to start the app");

        // Wait for the app to start up
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Send a request to the app
        let client = Client::new();
        let base_url = "http://localhost:3063";
        let endpoint = "/internal-backstage/info";
        let url = format!("{}{}{}", base_url, base_path, endpoint);

        let resp = client
            .get(&url)
            .send()
            .await
            .expect("Failed to send request");

        // Assert that the response status is 200 OK
        assert!(resp.status().is_success());

        let body = resp
            .bytes()
            .await
            .expect("Failed to retrieve response body as bytes");

        // Deserialize the response body into BuildInfo struct
        let info: BuildInfo =
            serde_json::from_slice(&body).expect("Failed to deserialize response body");

        // Assert that the app_name field matches the expected value
        assert_eq!(info.app_name, "unleash-edge");

        // Test a different endpoint
        let url = format!("{}{}{}", base_url, base_path, "/api/client/features");
        let resp = client
            .get(&url)
            .send()
            .await
            .expect("Failed to send request");

        // Assert that the response status is 403 Forbidden
        assert_eq!(resp.status(), 403);

        // Terminate the app process
        app_process.kill().expect("Failed to kill the app process");
        app_process
            .wait()
            .expect("Failed to wait for the app process");
    }
}
