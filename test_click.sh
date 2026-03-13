#!/bin/bash
# Test script for click reliability fixes

set -e

echo "=================================="
echo "Click Reliability Test Script"
echo "=================================="
echo ""

# Check backend availability
echo "1. Checking backend availability..."
if command -v ydotool &> /dev/null; then
    echo "   ✅ ydotool found: $(which ydotool)"
elif command -v wlrctl &> /dev/null; then
    echo "   ✅ wlrctl found: $(which wlrctl)"
else
    echo "   ❌ No backend found!"
    echo "   Install one of:"
    echo "     - ydotool: sudo pacman -S ydotool (Arch)"
    echo "     - ydotool: sudo apt install ydotool (Ubuntu)"
    echo "     - wlrctl: sudo pacman -S wlrctl (Arch)"
    exit 1
fi

# Check if ydotoold service is running
if command -v ydotool &> /dev/null; then
    echo ""
    echo "2. Checking ydotoold service..."
    if systemctl is-active --quiet ydotoold 2>/dev/null; then
        echo "   ✅ ydotoold service is running"
    else
        echo "   ⚠️  ydotoold service not running"
        echo "   Start with: sudo systemctl start ydotoold"
        echo "   Enable on boot: sudo systemctl enable ydotoold"
    fi
fi

# Test manual click
echo ""
echo "3. Testing manual click..."
if command -v ydotool &> /dev/null; then
    echo "   Running: ydotool click 1"
    if ydotool click 1 2>/dev/null; then
        echo "   ✅ Manual click works"
    else
        echo "   ❌ Manual click failed"
        echo "   Check permissions: sudo usermod -aG input $USER"
        echo "   Then log out and back in"
    fi
fi

# Build the project
echo ""
echo "4. Building project..."
if cargo check --workspace --quiet 2>&1 | grep -q "error"; then
    echo "   ❌ Build failed"
    cargo check --workspace
    exit 1
else
    echo "   ✅ Build successful"
fi

# Instructions for manual testing
echo ""
echo "=================================="
echo "Manual Testing Instructions"
echo "=================================="
echo ""
echo "Run the agent with debug logging:"
echo "  cd /home/bigfoot/hyprclaw"
echo "  RUST_LOG=debug cargo run"
echo ""
echo "Test commands to try:"
echo "  > click left mouse button"
echo "  > perform a left click"
echo "  > click the mouse"
echo "  > left click"
echo ""
echo "Look for in logs:"
echo "  🔧 TOOL CALL:"
echo "    Tool: 'desktop.mouse_click'"
echo "    Input: {"
echo "      \"button\": \"left\""
echo "    }"
echo "  ✅ TOOL SUCCESS"
echo ""
echo "Test error handling:"
echo "  > click"
echo ""
echo "Should show clear error about missing button parameter."
echo ""
echo "=================================="
echo "All checks complete!"
echo "=================================="
