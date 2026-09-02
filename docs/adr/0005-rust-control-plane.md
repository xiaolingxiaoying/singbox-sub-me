# Use Rust for the sbctl control plane

sbctl is implemented in stable Rust rather than C++. Its small long-lived HTTPS service and CLI use Tokio with a single-thread runtime, Rustls for TLS, and Serde for configuration artifacts; Certbot remains a non-resident system package for ACME issuance and renewal. Rust retains the low-memory, native-binary profile required by the VPS target while making the public HTTP, credential, concurrent state, and filesystem boundaries memory-safe by default.
