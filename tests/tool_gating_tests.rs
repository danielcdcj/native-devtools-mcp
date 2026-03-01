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
        let tools = MacOSDevToolsServer::get_tools(false, false);
        let tool_names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

        // Base tools should be present
        assert!(tool_names.contains(&"take_screenshot".to_string()));
        assert!(tool_names.contains(&"list_windows".to_string()));
        assert!(tool_names.contains(&"click".to_string()));
        assert!(tool_names.contains(&"type_text".to_string()));
        assert!(tool_names.contains(&"element_at_point".to_string()));

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

        let connected_tools = MacOSDevToolsServer::get_tools(true, false);
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
        let disconnected_tools = MacOSDevToolsServer::get_tools(false, false);
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
        let tools = MacOSDevToolsServer::get_tools(false, false);
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

        assert!(names.contains(&"android_list_devices".to_string()));
        assert!(names.contains(&"android_connect".to_string()));
    }

    #[test]
    fn test_android_tools_hidden_when_disconnected() {
        let tools = MacOSDevToolsServer::get_tools(false, false);
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
        let tools = MacOSDevToolsServer::get_tools(false, true);
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
        let disconnected = MacOSDevToolsServer::get_tools(false, false);
        let connected = MacOSDevToolsServer::get_tools(false, true);

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
