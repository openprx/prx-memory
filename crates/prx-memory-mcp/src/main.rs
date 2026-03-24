use prx_memory_mcp::McpServer;

fn main() -> std::io::Result<()> {
    let server = McpServer::new().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    server.serve_stdio()
}
