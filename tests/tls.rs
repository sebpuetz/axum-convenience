#![cfg(feature = "tls")]
use axum_convenience::RustlsConfig;
use reqwest::Certificate;

mod helpers;

const ROOTCA: &'static [u8] = include_bytes!("../self-signed-certs/rootCA.pem");
const KEY: &'static [u8] = include_bytes!("../self-signed-certs/localhost-key.pem");
const CERT: &'static [u8] = include_bytes!("../self-signed-certs/localhost.pem");

#[tokio::test]
async fn test_tls() {
    let tls_config = RustlsConfig::from_pem(CERT.to_vec(), KEY.to_vec())
        .await
        .expect("Failed to build tls config");
    let tls_app = helpers::hello_world_app_builder()
        .with_tls(tls_config)
        .spawn()
        .await
        .expect("failed to build app");
    let addr = tls_app.local_addr();
    let cert = Certificate::from_pem(ROOTCA).expect("Failed to create cert");
    let client = reqwest::ClientBuilder::new()
        .add_root_certificate(cert)
        .build()
        .expect("Failed to build client");

    let resp = client
        .get(format!("https://localhost:{}/hello-world", addr.port()))
        .send()
        .await;

    let text = resp
        .expect("should be able to send request")
        .text()
        .await
        .expect("should be able to decode hello world response");
    assert_eq!(text, helpers::HELLO_WORLD);
}
