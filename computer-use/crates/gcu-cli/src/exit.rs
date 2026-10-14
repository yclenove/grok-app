//! Single authority for architecture §2 process exit codes.
use gcu_protocol::ErrorCode;
use rmcp::model::CallToolResult;

/// Map every protocol error to its CLI exit code. Pure, nonblocking, no cancellation.
pub const fn error_exit_code(code: ErrorCode) -> u8 {
    match code {
        ErrorCode::AmbiguousApp | ErrorCode::InvalidKey | ErrorCode::OutOfBounds => 2,
        ErrorCode::PermissionMissing => 3,
        ErrorCode::Internal => 10,
        _ => 4,
    }
}
pub(crate) fn result_exit_code(result: &CallToolResult) -> u8 {
    if result.is_error != Some(true) {
        return 0;
    }
    result
        .structured_content
        .as_ref()
        .and_then(|v| v.pointer("/error/code"))
        .and_then(|v| serde_json::from_value::<ErrorCode>(v.clone()).ok())
        .map(error_exit_code)
        .unwrap_or(10)
}
pub(crate) struct Failure {
    pub code: u8,
    pub message: String,
}
impl Failure {
    pub(crate) fn arguments(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            code: 10,
            message: message.into(),
        }
    }
}
impl From<anyhow::Error> for Failure {
    fn from(_: anyhow::Error) -> Self {
        Self::internal("Internal IO/runtime failure; no operation is replayed")
    }
}
impl From<gcu_host_protocol::Error> for Failure {
    fn from(_: gcu_host_protocol::Error) -> Self {
        Self::internal("Host authentication or connection failed")
    }
}
impl From<gcu_mcp::ServeError> for Failure {
    fn from(_: gcu_mcp::ServeError) -> Self {
        Self::internal("MCP session/transport failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn error_all_twenty_six_codes_match_specification_table() {
        let expected = [
            4, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2, 4, 4, 2, 4, 4, 4, 4, 4, 4, 3, 4, 4, 4, 10,
        ];
        for (code, expected) in ErrorCode::ALL.into_iter().zip(expected) {
            assert_eq!(error_exit_code(code), expected, "{code}");
            let mut result = CallToolResult::error(vec![]);
            result.structured_content = Some(serde_json::json!({"error":{"code":code}}));
            assert_eq!(result_exit_code(&result), expected);
        }
    }
    #[test]
    fn result_unknown_or_missing_error_is_internal() {
        assert_eq!(result_exit_code(&CallToolResult::error(vec![])), 10);
        assert_eq!(result_exit_code(&CallToolResult::success(vec![])), 0);
    }
}
