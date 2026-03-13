use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMetadata {
    #[serde(rename = "ideType")]
    pub ide_type: String,
    pub platform: String,
    #[serde(rename = "pluginType")]
    pub plugin_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fingerprint {
    #[serde(rename = "deviceId")]
    pub device_id: String,
    #[serde(rename = "sessionToken")]
    pub session_token: String,
    #[serde(rename = "userAgent")]
    pub user_agent: String,
    #[serde(rename = "apiClient")]
    pub api_client: String,
    #[serde(rename = "clientMetadata")]
    pub client_metadata: ClientMetadata,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
}

const ANTIGRAVITY_VERSION: &str = "1.18.3";

const SDK_CLIENTS: &[&str] = &[
    "google-cloud-sdk vscode_cloudshelleditor/0.1",
    "google-cloud-sdk vscode/1.86.0",
    "google-cloud-sdk vscode/1.87.0",
    "google-cloud-sdk vscode/1.96.0",
];

const PLATFORMS: &[&str] = &["WINDOWS", "MACOS"];

fn random_from<T: Copy>(items: &[T]) -> T {
    let mut rng = rand::thread_rng();
    items[rng.gen_range(0..items.len())]
}

fn generate_session_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
    hex::encode(bytes)
}

pub fn generate_fingerprint() -> Fingerprint {
    let platform = random_from(PLATFORMS);
    let api_client = random_from(SDK_CLIENTS);

    let user_agent = format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Antigravity/{} Chrome/138.0.7204.235 Electron/37.3.1 Safari/537.36",
        ANTIGRAVITY_VERSION
    );

    Fingerprint {
        device_id: Uuid::new_v4().to_string(),
        session_token: generate_session_token(),
        user_agent,
        api_client: api_client.to_string(),
        client_metadata: ClientMetadata {
            ide_type: "ANTIGRAVITY".to_string(),
            platform: platform.to_string(),
            plugin_type: "GEMINI".to_string(),
        },
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    }
}

pub fn build_fingerprint_headers(fingerprint: &Fingerprint) -> Vec<(String, String)> {
    vec![
        ("User-Agent".to_string(), fingerprint.user_agent.clone()),
        (
            "X-Goog-Api-Client".to_string(),
            fingerprint.api_client.clone(),
        ),
        (
            "Client-Metadata".to_string(),
            serde_json::to_string(&fingerprint.client_metadata).unwrap_or_default(),
        ),
    ]
}
