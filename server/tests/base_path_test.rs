#[cfg(test)]
mod base_path_tests {
    use reqwest::Client;
    use std::process::{Command, Stdio};

    #[actix_web::test]
    async fn test_base_path() {
        let base_path = "/test/path";
        let token = "*:test.test";

        // Run the app as a separate process
        let mut app_process = Command::new("./../target/debug/unleash-edge")
            .arg("--base-path")
            .arg(base_path)
            .arg("offline")
            .arg("-t")
            .arg(token)
            .arg("--bootstrap-file")
            .arg("../examples/features.json")
            .stdout(Stdio::null()) // Suppress stdout
            .stderr(Stdio::null()) // Suppress stderr
            .spawn()
            .expect("Failed to start the app");

        // Wait for the app to start up
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Send a request to the app
        let client = Client::new();
        let base_url = "http://localhost:3063";
        let endpoint = "/internal-backstage/health";
        let url = format!("{}{}{}", base_url, base_path, endpoint);
        let resp = client
            .get(&url)
            .send()
            .await
            .expect("Failed to send request");

        // Assert that the response status is 200 OK
        assert!(resp.status().is_success());

        // Test a different endpoint
        let url = format!("{}{}{}", base_url, base_path, "/api/client/features");
        let resp = client
            .get(url)
            .header("Authorization", token)
            .send()
            .await
            .expect("Failed to send request");

        // Assert that the response status is 200 OK
        assert!(resp.status().is_success());

        // Terminate the app process
        app_process.kill().expect("Failed to kill the app process");
        app_process
            .wait()
            .expect("Failed to wait for the app process");
    }
}
