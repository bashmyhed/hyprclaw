#[cfg(test)]
mod integration_workflow_tests {
    use hypr_claw_app::scan::*;

    #[tokio::test]
    async fn test_integrated_scan_basic() {
        // Test basic (non-deep) scan
        let result = run_integrated_scan("test_user", false).await;
        assert!(result.is_ok(), "Basic scan should succeed");

        let profile = result.unwrap();

        // Verify basic fields exist
        assert!(profile.get("scanned_at").is_some());
        assert!(profile.get("platform").is_some());
        assert!(profile.get("user").is_some());
        assert!(profile.get("desktop").is_some());

        // Should not have deep_scan data
        assert!(profile.get("deep_scan").is_none());

        println!("\n✅ Basic scan profile:");
        println!("{}", serde_json::to_string_pretty(&profile).unwrap());
    }

    #[test]
    fn test_scan_module_exports() {
        // Verify all public APIs are accessible
        let _dirs = UserDirectories::discover();
        let _policy = ScanPolicy::default();
        let _monitor = ResourceMonitor::auto_calibrate();

        // Verify types are exported
        let _level: SensitivityLevel = SensitivityLevel::Public;
        let _category: DirectoryCategory = DirectoryCategory::Projects;
    }
}
