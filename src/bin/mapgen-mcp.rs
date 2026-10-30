//! MCP server for Claude: configure the generator (gates, mobs, any node),
//! generate and export. Speaks JSON-RPC over stdio; see `src/mcp.rs`.

fn main() -> std::io::Result<()> {
    map_template_generator::mcp::run_stdio(map_template_generator::mcp::config_path_from_env())
}
