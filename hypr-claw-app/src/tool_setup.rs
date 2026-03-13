use std::sync::Arc;

pub fn build_tool_registry() -> anyhow::Result<hypr_claw_tools::ToolRegistryImpl> {
    let mut registry = hypr_claw_tools::ToolRegistryImpl::new();
    registry.register(Arc::new(hypr_claw_tools::tools::EchoTool));
    let imported_registry = registry.clone();
    registry.register_core(Arc::new(
        hypr_claw_tools::imported::HiddenToolSearchBm25Tool::new(imported_registry.clone()),
    ));
    registry.register_core(Arc::new(
        hypr_claw_tools::imported::HiddenToolSearchRegexTool::new(imported_registry),
    ));

    registry.register(Arc::new(hypr_claw_tools::os_tools::FsCreateDirTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsDeleteTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsMoveTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsCopyTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsReadTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsWriteTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::FsListTool));

    registry.register_hidden(Arc::new(hypr_claw_tools::imported::Fs2ReadTool::new(
        "./sandbox",
    )?));
    registry.register_hidden(Arc::new(hypr_claw_tools::imported::Fs2WriteTool::new(
        "./sandbox",
    )?));
    registry.register_hidden(Arc::new(hypr_claw_tools::imported::Fs2ListTool::new(
        "./sandbox",
    )?));
    registry.register_hidden(Arc::new(hypr_claw_tools::imported::Fs2EditTool::new(
        "./sandbox",
    )?));
    registry.register_hidden(Arc::new(hypr_claw_tools::imported::Fs2AppendTool::new(
        "./sandbox",
    )?));

    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprWorkspaceSwitchTool));
    hypr_claw_tools::register_workspace_tools(&mut registry);
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::HyprWorkspaceMoveWindowTool,
    ));
    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprWindowFocusTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprWindowCloseTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprWindowMoveTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::HyprExecTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::ProcSpawnTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::ProcKillTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::ProcListTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopOpenUrlTool));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopOpenWorkspaceAppTool,
    ));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopLaunchAppTool));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopLaunchAppAndWaitTextTool,
    ));
    registry.register(Arc::new(hypr_claw_tools::os_tools::BrowserHealthTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::BrowserNavigateTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::BrowserSnapshotTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::BrowserActionTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::BrowserEvaluateTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::BrowserScreenshotTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopSearchWebTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopOpenGmailTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopTypeTextTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopKeyPressTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopKeyComboTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopMouseClickTool));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopCaptureScreenTool,
    ));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopHealthStatusTool));
    registry.register(Arc::new(hypr_claw_tools::DesktopFastWindowStateTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopActiveWindowTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopListWindowsTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopMouseMoveTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopClickTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopClickAtTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopOcrScreenTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopFindTextTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopClickTextTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::DesktopWaitForTextTool));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopCursorPositionTool,
    ));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopMouseMoveAndVerifyTool,
    ));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopClickAtAndVerifyTool,
    ));
    registry.register(Arc::new(
        hypr_claw_tools::os_tools::DesktopReadScreenStateTool,
    ));
    registry.register(Arc::new(hypr_claw_tools::os_tools::WallpaperSetTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::SystemShutdownTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::SystemRebootTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::SystemBatteryTool));
    registry.register(Arc::new(hypr_claw_tools::os_tools::SystemMemoryTool));

    Ok(registry)
}
