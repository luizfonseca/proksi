//! The ports proksi listens on, served by the same router

/// Which listener a request came through
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listener {
    /// The HTTPS port: hosts with TLS
    Https,
    /// The HTTP port: hosts without TLS, ACME challenges, and redirects to HTTPS for the rest
    Http,
}
