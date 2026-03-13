use async_trait::async_trait;

#[async_trait]
pub trait Interface: Send + Sync {
    async fn receive_input(&self) -> Option<String>;
    async fn send_output(&self, message: &str);
    async fn request_approval(&self, action: &str) -> bool;
    async fn show_status(&self, status: &str);
}
