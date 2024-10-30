use tracing::info;

pub fn _access_log(_attrs: Option<u32>) {
    info!(
        duration = "1ms",
        log = "access.log",
        path = "/",
        host = "example.com",
        headers = "{}",
        method = "GET",
        backend = "host:port",
        status = 200,
        "Access log with attrs"
    );
}
