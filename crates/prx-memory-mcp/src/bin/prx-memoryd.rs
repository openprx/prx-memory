use std::io;

use prx_memory_mcp::McpServer;

fn main() -> io::Result<()> {
    let mode = std::env::var("PRX_MEMORYD_TRANSPORT").unwrap_or_else(|_| "stdio".to_string());
    let server = McpServer::new();
    match mode.as_str() {
        "stdio" => server.serve_stdio(),
        "http" => {
            let addr = std::env::var("PRX_MEMORY_HTTP_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:8787".to_string());
            server.serve_http(&addr)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PRX_MEMORYD_TRANSPORT must be stdio or http",
        )),
    }
}
