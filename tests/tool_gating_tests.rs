//! Tests for dynamic tool gating and identity validation
//!
//! These tests verify:
//! 1. Tool list changes based on connection state
//! 2. Server capability advertisement for tool list changes
//! 3. Identity validation logic for expected_bundle_id and expected_app_name
//!
//! ## Test Pattern Guidelines
//!
//! The `identity_validation` module demonstrates recommended test patterns:
//!
//! - **Granular test functions**: One behavior per test, named descriptively
//! - **Edge case coverage**: Empty strings, whitespace, case sensitivity, missing fields
//! - **MARK comments**: Group related tests (e.g., `// MARK: - validate_bundle_id tests`)
//! - **Integration tests**: `validate_identity` tests combine lower-level validators
//! - **Behavior documentation**: Test names describe what behavior is being verified
//!
//! When adding new tests, follow the patterns in `identity_validation` for consistency.

use native_devtools_mcp::server::MacOSDevToolsServer;
use rmcp::handler::server::ServerHandler;

#[cfg(test)]
mod tool_gating {
    use super::*;

    #[test]
    fn test_base_tools_always_present_when_disconnected() {
        // When disconnected, should have base tools + app_connect
        // app_* tools (except app_connect) should NOT be present

        // This tests the get_tools(false) case
        let tools = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let tool_names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

        // Base tools should be present
        assert!(tool_names.contains(&"take_screenshot".to_string()));
        assert!(tool_names.contains(&"list_windows".to_string()));
        assert!(tool_names.contains(&"click".to_string()));
        assert!(tool_names.contains(&"type_text".to_string()));
        assert!(tool_names.contains(&"element_at_point".to_string()));
        assert!(tool_names.contains(&"quit_app".to_string()));
        assert!(tool_names.contains(&"probe_app".to_string()));

        // app_connect should always be present
        assert!(tool_names.contains(&"app_connect".to_string()));

        // app_* tools should NOT be present when disconnected
        assert!(!tool_names.contains(&"app_disconnect".to_string()));
        assert!(!tool_names.contains(&"app_get_info".to_string()));
        assert!(!tool_names.contains(&"app_get_tree".to_string()));
        assert!(!tool_names.contains(&"app_click".to_string()));
    }

    #[test]
    fn test_app_tools_present_when_connected() {
        // When connected, should have base tools + app_connect + all app_* tools

        let connected_tools = MacOSDevToolsServer::get_tools(true, false, false, false, false);
        let tool_names: Vec<String> = connected_tools.iter().map(|t| t.name.to_string()).collect();

        // Base tools should still be present
        assert!(tool_names.contains(&"take_screenshot".to_string()));
        assert!(tool_names.contains(&"list_windows".to_string()));

        // app_connect should always be present
        assert!(tool_names.contains(&"app_connect".to_string()));

        // app_* tools should be present when connected
        assert!(tool_names.contains(&"app_disconnect".to_string()));
        assert!(tool_names.contains(&"app_get_info".to_string()));
        assert!(tool_names.contains(&"app_get_tree".to_string()));
        assert!(tool_names.contains(&"app_query".to_string()));
        assert!(tool_names.contains(&"app_get_element".to_string()));
        assert!(tool_names.contains(&"app_click".to_string()));
        assert!(tool_names.contains(&"app_type".to_string()));
        assert!(tool_names.contains(&"app_press_key".to_string()));
        assert!(tool_names.contains(&"app_focus".to_string()));
        assert!(tool_names.contains(&"app_screenshot".to_string()));
        assert!(tool_names.contains(&"app_list_windows".to_string()));
        assert!(tool_names.contains(&"app_focus_window".to_string()));

        // Connected state should have more tools than disconnected
        let disconnected_tools = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        assert!(
            connected_tools.len() > disconnected_tools.len(),
            "Connected state should expose more tools than disconnected state"
        );
    }
}

#[cfg(test)]
mod android_tool_gating {
    use super::*;

    #[test]
    fn test_android_base_tools_always_present() {
        let tools = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

        assert!(names.contains(&"android_list_devices".to_string()));
        assert!(names.contains(&"android_connect".to_string()));
    }

    #[test]
    fn test_android_tools_hidden_when_disconnected() {
        let tools = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

        assert!(!names.contains(&"android_disconnect".to_string()));
        assert!(!names.contains(&"android_screenshot".to_string()));
        assert!(!names.contains(&"android_click".to_string()));
        assert!(!names.contains(&"android_swipe".to_string()));
        assert!(!names.contains(&"android_type_text".to_string()));
        assert!(!names.contains(&"android_press_key".to_string()));
        assert!(!names.contains(&"android_find_text".to_string()));
        assert!(!names.contains(&"android_list_apps".to_string()));
        assert!(!names.contains(&"android_launch_app".to_string()));
        assert!(!names.contains(&"android_get_display_info".to_string()));
        assert!(!names.contains(&"android_get_current_activity".to_string()));
    }

    #[test]
    fn test_android_tools_visible_when_connected() {
        let tools = MacOSDevToolsServer::get_tools(false, true, false, false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

        // Base tools
        assert!(names.contains(&"android_list_devices".to_string()));
        assert!(names.contains(&"android_connect".to_string()));

        // Connected-only tools
        assert!(names.contains(&"android_disconnect".to_string()));
        assert!(names.contains(&"android_screenshot".to_string()));
        assert!(names.contains(&"android_click".to_string()));
        assert!(names.contains(&"android_swipe".to_string()));
        assert!(names.contains(&"android_type_text".to_string()));
        assert!(names.contains(&"android_press_key".to_string()));
        assert!(names.contains(&"android_find_text".to_string()));
        assert!(names.contains(&"android_list_apps".to_string()));
        assert!(names.contains(&"android_launch_app".to_string()));
        assert!(names.contains(&"android_get_display_info".to_string()));
        assert!(names.contains(&"android_get_current_activity".to_string()));
    }

    #[test]
    fn test_android_connection_adds_tools() {
        let disconnected = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let connected = MacOSDevToolsServer::get_tools(false, true, false, false, false);

        assert!(
            connected.len() > disconnected.len(),
            "Android connected state should expose more tools: {} vs {}",
            connected.len(),
            disconnected.len()
        );

        // Should add exactly 11 tools (disconnect + 10 functional tools)
        assert_eq!(connected.len() - disconnected.len(), 11);
    }
}

#[cfg(test)]
#[cfg(feature = "cdp")]
mod cdp_tool_gating {
    use super::*;

    /// The full set of CDP tools — used by visibility checks below.
    /// Kept in sync with `get_cdp_connect_tool` + `get_cdp_tools` in
    /// `src/server.rs`.
    const CDP_TOOL_NAMES: &[&str] = &[
        "cdp_connect",
        "cdp_disconnect",
        "cdp_take_ax_snapshot",
        "cdp_take_dom_snapshot",
        "cdp_find_elements",
        "cdp_evaluate_script",
        "cdp_click",
        "cdp_list_pages",
        "cdp_select_page",
        "cdp_hover",
        "cdp_fill",
        "cdp_press_key",
        "cdp_handle_dialog",
        "cdp_navigate",
        "cdp_new_page",
        "cdp_close_page",
        "cdp_wait_for",
        "cdp_type_text",
        "cdp_element_at_point",
    ];

    #[test]
    fn test_cdp_tools_always_visible_when_disconnected() {
        // CDP tools are now listed unconditionally so the tool surface does
        // not mutate mid-session. Handlers return a clean "No CDP connection"
        // error when called without an active connection.
        let tools = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        for tool in CDP_TOOL_NAMES {
            assert!(
                names.contains(&tool.to_string()),
                "{} should be listed even without a CDP connection",
                tool
            );
        }
    }

    #[test]
    fn test_cdp_tools_always_visible_when_connected() {
        let tools = MacOSDevToolsServer::get_tools(false, false, true, false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        for tool in CDP_TOOL_NAMES {
            assert!(
                names.contains(&tool.to_string()),
                "{} should be listed when connected",
                tool
            );
        }
    }

    #[test]
    fn test_cdp_tool_list_is_stable_across_connection_state() {
        // The visible tool list must be identical regardless of
        // `cdp_connected` — mid-session tool-list changes are the whole
        // reason these tools are now unconditional.
        let disconnected = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let connected = MacOSDevToolsServer::get_tools(false, false, true, false, false);
        assert_eq!(
            disconnected.len(),
            connected.len(),
            "CDP connection state must not change the number of visible tools"
        );
        let dn: Vec<_> = disconnected.iter().map(|t| t.name.to_string()).collect();
        let cn: Vec<_> = connected.iter().map(|t| t.name.to_string()).collect();
        assert_eq!(dn, cn);
    }
}

#[cfg(test)]
#[cfg(feature = "cdp")]
mod cdp_not_connected_errors {
    //! Calling CDP tool handlers without an active connection must return a
    //! structured error, not panic or silently no-op. This guards the
    //! "tools always listed" guarantee.

    use native_devtools_mcp::cdp::tools;
    use native_devtools_mcp::cdp::CdpClient;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn empty_client() -> Arc<RwLock<Option<CdpClient>>> {
        Arc::new(RwLock::new(None))
    }

    fn assert_not_connected(result: &rmcp::model::CallToolResult) {
        assert_eq!(
            result.is_error,
            Some(true),
            "expected error result when CDP is not connected, got: {:?}",
            result
        );
        let text = result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("");
        assert!(
            text.to_lowercase().contains("no cdp connection"),
            "error should mention 'No CDP connection', got: {}",
            text
        );
    }

    #[tokio::test]
    async fn cdp_click_without_connection_returns_clean_error() {
        let result = tools::cdp_click("a1".to_string(), false, false, empty_client()).await;
        assert_not_connected(&result);
    }

    #[tokio::test]
    async fn cdp_take_ax_snapshot_without_connection_returns_clean_error() {
        let result = tools::cdp_take_ax_snapshot(empty_client()).await;
        assert_not_connected(&result);
    }

    #[tokio::test]
    async fn cdp_list_pages_without_connection_returns_clean_error() {
        let result = tools::cdp_list_pages(empty_client()).await;
        assert_not_connected(&result);
    }

    #[tokio::test]
    async fn cdp_element_at_point_without_connection_returns_clean_error() {
        let result = tools::cdp_element_at_point(0.0, 0.0, empty_client()).await;
        assert_not_connected(&result);
    }
}

#[cfg(test)]
mod server_capabilities {
    use super::*;

    #[test]
    fn test_server_advertises_tool_list_changed() {
        let server = MacOSDevToolsServer::new();
        let info = server.get_info();

        // Check that tools capability exists
        let tools_cap = info.capabilities.tools.as_ref();
        assert!(
            tools_cap.is_some(),
            "Server should advertise tools capability"
        );

        // Check that list_changed is advertised
        let tools = tools_cap.unwrap();
        assert_eq!(
            tools.list_changed,
            Some(true),
            "Server should advertise tools.listChanged capability"
        );
    }
}

#[cfg(test)]
mod identity_validation {
    //! Tests for expected_bundle_id and expected_app_name validation logic
    //!
    //! These tests exercise the extracted validation helpers that are used
    //! by app_connect to validate app identity.

    use native_devtools_mcp::tools::app_protocol::{
        validate_app_name, validate_bundle_id, validate_identity, IdentityValidationResult,
    };
    use serde_json::json;

    // MARK: - validate_bundle_id tests

    #[test]
    fn test_bundle_id_exact_match() {
        assert!(validate_bundle_id("com.example.MyApp", "com.example.MyApp"));
    }

    #[test]
    fn test_bundle_id_case_sensitive() {
        // Bundle ID matching should be case-sensitive
        assert!(!validate_bundle_id(
            "com.example.MyApp",
            "com.example.myapp"
        ));
        assert!(!validate_bundle_id(
            "com.example.MyApp",
            "COM.EXAMPLE.MYAPP"
        ));
    }

    #[test]
    fn test_bundle_id_different_ids() {
        assert!(!validate_bundle_id("com.example.MyApp", "com.other.MyApp"));
        assert!(!validate_bundle_id("com.example.MyApp", ""));
    }

    // MARK: - validate_app_name tests

    #[test]
    fn test_app_name_exact_match() {
        assert!(validate_app_name("MyApp", "MyApp"));
    }

    #[test]
    fn test_app_name_case_insensitive() {
        assert!(validate_app_name("MyApp", "myapp"));
        assert!(validate_app_name("MyApp", "MYAPP"));
        assert!(validate_app_name("myapp", "MyApp"));
    }

    #[test]
    fn test_app_name_whitespace_trimmed() {
        assert!(validate_app_name("MyApp", "  MyApp  "));
        assert!(validate_app_name("  MyApp  ", "MyApp"));
        assert!(validate_app_name("  MyApp  ", "  myapp  "));
    }

    #[test]
    fn test_app_name_different_names() {
        assert!(!validate_app_name("MyApp", "OtherApp"));
        assert!(!validate_app_name("MyApp", ""));
    }

    // MARK: - validate_identity tests (integration of both validations)

    #[test]
    fn test_validate_identity_no_expectations() {
        let info = json!({
            "appName": "TestApp",
            "bundleId": "com.test.TestApp"
        });

        assert_eq!(
            validate_identity(None, None, &info),
            IdentityValidationResult::Ok
        );
    }

    #[test]
    fn test_validate_identity_bundle_id_match() {
        let info = json!({
            "appName": "TestApp",
            "bundleId": "com.test.TestApp"
        });

        assert_eq!(
            validate_identity(Some("com.test.TestApp"), None, &info),
            IdentityValidationResult::Ok
        );
    }

    #[test]
    fn test_validate_identity_bundle_id_mismatch() {
        let info = json!({
            "appName": "TestApp",
            "bundleId": "com.test.TestApp"
        });

        assert_eq!(
            validate_identity(Some("com.other.App"), None, &info),
            IdentityValidationResult::BundleIdMismatch {
                expected: "com.other.App".to_string(),
                actual: "com.test.TestApp".to_string(),
                actual_app_name: "TestApp".to_string(),
            }
        );
    }

    #[test]
    fn test_validate_identity_app_name_match() {
        let info = json!({
            "appName": "TestApp",
            "bundleId": "com.test.TestApp"
        });

        assert_eq!(
            validate_identity(None, Some("TestApp"), &info),
            IdentityValidationResult::Ok
        );
        // Case-insensitive
        assert_eq!(
            validate_identity(None, Some("testapp"), &info),
            IdentityValidationResult::Ok
        );
    }

    #[test]
    fn test_validate_identity_app_name_mismatch() {
        let info = json!({
            "appName": "TestApp",
            "bundleId": "com.test.TestApp"
        });

        assert_eq!(
            validate_identity(None, Some("OtherApp"), &info),
            IdentityValidationResult::AppNameMismatch {
                expected: "OtherApp".to_string(),
                actual: "TestApp".to_string(),
                actual_bundle_id: "com.test.TestApp".to_string(),
            }
        );
    }

    #[test]
    fn test_validate_identity_both_expectations_pass() {
        let info = json!({
            "appName": "TestApp",
            "bundleId": "com.test.TestApp"
        });

        assert_eq!(
            validate_identity(Some("com.test.TestApp"), Some("testapp"), &info),
            IdentityValidationResult::Ok
        );
    }

    #[test]
    fn test_validate_identity_bundle_id_checked_first() {
        // When both are provided, bundle ID is checked first
        let info = json!({
            "appName": "TestApp",
            "bundleId": "com.test.TestApp"
        });

        // Both mismatch - should return BundleIdMismatch since it's checked first
        let result = validate_identity(Some("com.wrong.Id"), Some("WrongName"), &info);
        assert!(matches!(
            result,
            IdentityValidationResult::BundleIdMismatch { .. }
        ));
    }

    #[test]
    fn test_validate_identity_missing_fields() {
        // Empty info object
        let info = json!({});

        // Missing bundleId should fail if expected
        assert_eq!(
            validate_identity(Some("com.test.App"), None, &info),
            IdentityValidationResult::BundleIdMismatch {
                expected: "com.test.App".to_string(),
                actual: "".to_string(),
                actual_app_name: "".to_string(),
            }
        );

        // Missing appName should fail if expected
        assert_eq!(
            validate_identity(None, Some("TestApp"), &info),
            IdentityValidationResult::AppNameMismatch {
                expected: "TestApp".to_string(),
                actual: "".to_string(),
                actual_bundle_id: "".to_string(),
            }
        );
    }

    #[test]
    fn test_validate_identity_whitespace_in_app_name() {
        let info = json!({
            "appName": "  TestApp  ",
            "bundleId": "com.test.TestApp"
        });

        // Should still match with trimming
        assert_eq!(
            validate_identity(None, Some("TestApp"), &info),
            IdentityValidationResult::Ok
        );
        assert_eq!(
            validate_identity(None, Some("  testapp  "), &info),
            IdentityValidationResult::Ok
        );
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod hover_tracking_tool_gating {
    use super::*;

    #[test]
    fn test_start_hover_tracking_always_visible() {
        let tools = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&"start_hover_tracking".to_string()));
    }

    #[test]
    fn test_hover_tools_hidden_when_not_tracking() {
        let tools = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        assert!(!names.contains(&"get_hover_events".to_string()));
        assert!(!names.contains(&"stop_hover_tracking".to_string()));
    }

    #[test]
    fn test_hover_tools_visible_when_tracking() {
        let tools = MacOSDevToolsServer::get_tools(false, false, false, true, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&"start_hover_tracking".to_string()));
        assert!(names.contains(&"get_hover_events".to_string()));
        assert!(names.contains(&"stop_hover_tracking".to_string()));
    }

    #[test]
    fn test_hover_tracking_adds_two_tools() {
        let not_tracking = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let tracking = MacOSDevToolsServer::get_tools(false, false, false, true, false);
        assert_eq!(tracking.len() - not_tracking.len(), 2);
    }

    #[test]
    fn test_all_states_independent() {
        // Hover tracking state should be independent of app/android connection
        let base = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let hover_only = MacOSDevToolsServer::get_tools(false, false, false, true, false);
        let app_and_hover = MacOSDevToolsServer::get_tools(true, false, false, true, false);
        // Hover adds exactly 2 tools regardless of other state
        assert_eq!(hover_only.len() - base.len(), 2);

        // App connection adds the same number of tools regardless of hover state
        let app_only = MacOSDevToolsServer::get_tools(true, false, false, false, false);
        assert_eq!(
            app_and_hover.len() - hover_only.len(),
            app_only.len() - base.len()
        );

        // All five states are additive
        let android_only = MacOSDevToolsServer::get_tools(false, true, false, false, false);
        let cdp_only = MacOSDevToolsServer::get_tools(false, false, true, false, false);
        let recording_only = MacOSDevToolsServer::get_tools(false, false, false, false, true);
        let all = MacOSDevToolsServer::get_tools(true, true, true, true, true);
        let expected_total = base.len()
            + (app_only.len() - base.len())
            + (android_only.len() - base.len())
            + (cdp_only.len() - base.len())
            + (hover_only.len() - base.len())
            + (recording_only.len() - base.len());
        assert_eq!(all.len(), expected_total);
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod recording_tool_gating {
    use super::*;

    #[test]
    fn test_start_recording_always_visible() {
        let tools = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&"start_recording".to_string()));
    }

    #[test]
    fn test_stop_recording_hidden_when_not_recording() {
        let tools = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        assert!(!names.contains(&"stop_recording".to_string()));
    }

    #[test]
    fn test_stop_recording_visible_when_recording() {
        let tools = MacOSDevToolsServer::get_tools(false, false, false, false, true);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&"start_recording".to_string()));
        assert!(names.contains(&"stop_recording".to_string()));
    }

    #[test]
    fn test_recording_adds_one_tool() {
        let not_recording = MacOSDevToolsServer::get_tools(false, false, false, false, false);
        let recording = MacOSDevToolsServer::get_tools(false, false, false, false, true);
        assert_eq!(recording.len() - not_recording.len(), 1);
    }
}
