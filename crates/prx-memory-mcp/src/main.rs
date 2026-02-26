use prx_memory_mcp::McpServer;

fn main() -> std::io::Result<()> {
    McpServer::new().serve_stdio()
}
