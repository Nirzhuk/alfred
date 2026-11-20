// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == "mcp") {
        // Claude Code and other agents launch this as a stdio MCP server.
        match args.next() {
            Some(endpoint) => return alfred_lib::mcp_bridge(endpoint.into()),
            None => {
                eprintln!("usage: alfred mcp <path to mcp-endpoint.json>");
                std::process::exit(2);
            }
        }
    }
    alfred_lib::run()
}
