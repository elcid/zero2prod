use crate::domain::SubscriberEmail;
use anyhow::Context;
use reqwest::Client;
use resend_rs::types::CreateEmailBaseOptions;
use resend_rs::{Config, Resend};
use secrecy::{ExposeSecret, SecretString};

pub struct EmailClient {
    http_client: Client,
    base_url: String,
    sender: SubscriberEmail,
    authorization_token: SecretString,
}

impl EmailClient {
    pub fn new(
        base_url: String,
        sender: SubscriberEmail,
        authorization_token: SecretString,
        timeout: std::time::Duration,
    ) -> Self {
        let http_client = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap();
        Self {
            http_client,
            base_url,
            sender,
            authorization_token,
        }
    }

    pub async fn send_email(
        &self,
        recipient: SubscriberEmail,
        subject: &str,
        html_content: &str,
        text_content: &str,
    ) -> anyhow::Result<()> {
        let resend = Resend::with_config(
            Config::builder(self.authorization_token.expose_secret())
                .base_url(
                    // this is Resend's default base url, but you can provide
                    // your override here, which is especially helpful when running
                    // many parallel tests and intercepting email requests
                    // in each of them
                    self.base_url.parse().context("failed to parse URL")?,
                )
                .client(self.http_client.clone()
                )
                .build(),
        );

        let from = self.sender.as_ref();
        let to = [recipient.as_ref()];

        let email = CreateEmailBaseOptions::new(from, to, subject)
            .with_text(text_content)
            .with_html(html_content);

        let _email = resend.emails.send(email).await?;
        println!("{:?}", _email);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use claims::{assert_err, assert_ok};
    use crate::domain::SubscriberEmail;
    use crate::email_client::EmailClient;
    use fake::faker::internet::en::SafeEmail;
    use fake::faker::lorem::en::{Paragraph, Sentence};
    use fake::Fake;
    use secrecy::SecretString;
    use wiremock::matchers::{any, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn send_email_fires_a_request_to_base_url() {
        // Arrange
        let mock_server = MockServer::start().await;
        let email_client = email_client(mock_server.uri());

        Mock::given(header_exists("Authorization"))
            .and(header("Content-Type", "application/json"))
            .and(path("/emails"))
            .and(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "id": "b1946ac9-46c4-4c8e-8b8a-8e1e8c8d8f8e",
                        "from": "test@example.com",
                        "to": ["recipient@example.com"],
                        "created_at": "2023-01-01T00:00:00.000Z"
                    }))
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        // Act
        let result = email_client.send_email(email(), &subject(), &content(), &content()).await;
        
        // Assert
        assert!(result.is_ok(), "Email sending failed: {:?}", result);
    }

    #[tokio::test]
    async fn send_email_succeeds_if_the_server_returns_200() {
        // Arrange
        let mock_server = MockServer::start().await;
        let email_client = email_client(mock_server.uri());

        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "id": "b1946ac9-46c4-4c8e-8b8a-8e1e8c8d8f8e",
                        "from": "test@example.com",
                        "to": ["recipient@example.com"],
                        "created_at": "2023-01-01T00:00:00.000Z"
                    }))
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        // Act
        let outcome = email_client
            .send_email(email(), &subject(), &content(), &content())
            .await;

        // Assert
        assert_ok!(outcome);
    }

    #[tokio::test]
    async fn send_email_fails_if_the_server_returns_500() {
        // Arrange
        let mock_server = MockServer::start().await;
        let email_client = email_client(mock_server.uri());

        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(serde_json::json!({
                        "id": "b1946ac9-46c4-4c8e-8b8a-8e1e8c8d8f8e",
                        "from": "test@example.com",
                        "to": ["recipient@example.com"],
                        "created_at": "2023-01-01T00:00:00.000Z"
                    }))
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        // Act
        let outcome = email_client
            .send_email(email(), &subject(), &content(), &content())
            .await;

        // Assert
        assert_err!(outcome);
    }

    #[tokio::test]
    async fn send_email_times_out_if_the_server_takes_too_long() {
        // Arrange
        let mock_server = MockServer::start().await;
        let email_client = email_client(mock_server.uri());

        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(200)
                    // 3 minutes!
                    .set_delay(std::time::Duration::from_secs(180))
                    .set_body_json(serde_json::json!({
                        "id": "b1946ac9-46c4-4c8e-8b8a-8e1e8c8d8f8e",
                        "from": "test@example.com",
                        "to": ["recipient@example.com"],
                        "created_at": "2023-01-01T00:00:00.000Z"
                    }))
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        // Act
        let outcome = email_client
            .send_email(email(), &subject(), &content(), &content())
            .await;

        // Assert
        assert_err!(outcome);
    }

    /// Generate a random email subject
    fn subject() -> String {
        Sentence(1..2).fake()
    }

    /// Generate a random email content
    fn content() -> String {
        Paragraph(1..10).fake()
    }

    /// Generate a random subscriber email
    fn email() -> SubscriberEmail {
        SubscriberEmail::parse(SafeEmail().fake()).unwrap()
    }

    /// Get a test instance of `EmailClient`.
    fn email_client(base_url: String) -> EmailClient {
        let sender = SubscriberEmail::parse(SafeEmail().fake()).unwrap();
        let auth_token = format!("re_{}", uuid::Uuid::new_v4().simple());
        EmailClient::new(
            base_url,
            sender,
            SecretString::new(auth_token.into_boxed_str()),
            std::time::Duration::from_millis(200),
        )
    }
}
