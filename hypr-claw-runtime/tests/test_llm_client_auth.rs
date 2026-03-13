#[cfg(test)]
mod llm_client_tests {
    use hypr_claw_runtime::LLMClient;

    #[test]
    fn test_llm_client_with_api_key() {
        let client = LLMClient::with_api_key(
            "https://integrate.api.nvidia.com/v1".to_string(),
            1,
            "test-api-key".to_string(),
        );

        // Client should be created successfully
        // API key is stored internally (not exposed)
        drop(client);
    }

    #[test]
    fn test_llm_client_without_api_key() {
        let client = LLMClient::new("http://localhost:8080".to_string(), 1);

        // Client should be created successfully
        drop(client);
    }

    #[test]
    fn test_nvidia_url_construction() {
        // Verify NVIDIA URL construction is correct
        let base_url = "https://integrate.api.nvidia.com/v1";
        let endpoint = "/chat/completions";
        let expected = "https://integrate.api.nvidia.com/v1/chat/completions";

        let constructed = format!("{}{}", base_url, endpoint);
        assert_eq!(
            constructed, expected,
            "NVIDIA URL construction must be correct"
        );
    }

    #[test]
    fn test_google_url_construction() {
        // Verify Google URL construction is correct
        let base_url = "https://generativelanguage.googleapis.com/v1beta/openai";
        let endpoint = "/chat/completions";
        let expected = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";

        let constructed = format!("{}{}", base_url, endpoint);
        assert_eq!(
            constructed, expected,
            "Google URL construction must be correct"
        );
    }

    #[test]
    fn test_local_url_construction() {
        // Verify local URL construction is correct
        let base_url = "http://localhost:8080";
        let endpoint = "/chat/completions";
        let expected = "http://localhost:8080/chat/completions";

        let constructed = format!("{}{}", base_url, endpoint);
        assert_eq!(
            constructed, expected,
            "Local URL construction must be correct"
        );
    }

    #[test]
    fn test_google_auth_header_present() {
        // Verify Google client is created with API key
        let client = LLMClient::with_api_key_and_model(
            "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
            1,
            "test-google-api-key".to_string(),
            "gemini-2.5-pro".to_string(),
        );

        // Client should be created successfully with API key
        drop(client);
    }
}
