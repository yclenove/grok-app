//! Session MCP stdio → Bearer loopback `/cu/tool` → Broker → packaged worker.
//! Probe-only. Not a substitute for a live Grok model (E4).

pub fn run_product_managed_route_gate() -> Result<(), String> {
    super::mcp_scripted_agent_run::run_mcp_scripted_agent_gate_inner(true)
}

pub fn run_mcp_scripted_agent_gate() -> Result<(), String> {
    super::mcp_scripted_agent_run::run_mcp_scripted_agent_gate_inner(false)
}
