# 🎯 Enhanced Click Automation

## Problem

Agent was calling `desktop.read_screen_state` but NOT following up with mouse actions. It would just search instead of clicking.

## Solution

**Strengthened the prompt** with explicit step-by-step instructions and a concrete example:

```
CLICKING ON SCREEN ELEMENTS (CRITICAL):
When you need to click something visible on screen:
1. desktop.read_screen_state with {"include_ocr": true} - Gets text with coordinates
2. Find target in OCR results (look for "text": "...", "x": N, "y": N)
3. desktop.mouse_move {"x": N, "y": N} - Move to those coordinates
4. desktop.mouse_click {"button": "left"} - Click

Example: To click a video thumbnail:
- OCR shows: {"text": "Video Title", "x": 640, "y": 360}
- Call: desktop.mouse_move {"x": 640, "y": 360}
- Call: desktop.mouse_click {"button": "left"}
```

## Key Changes

1. **Explicit tool name**: `desktop.read_screen_state` (not generic "OCR")
2. **Required parameter**: `{"include_ocr": true}`
3. **Concrete example**: Shows exact JSON format and coordinates
4. **Clear warning**: "Do NOT just search - USE OCR AND CLICK"

## Testing

```bash
# Use release build for speed
cargo build --release
./target/release/hypr-claw

> open youtube and play a random video
```

**Expected behavior**:
1. ✅ Opens YouTube
2. ✅ Calls `desktop.read_screen_state {"include_ocr": true}`
3. ✅ Finds video coordinates in OCR results
4. ✅ Calls `desktop.mouse_move {"x": X, "y": Y}`
5. ✅ Calls `desktop.mouse_click {"button": "left"}`
6. ✅ Video starts playing

## Why This Works

**Before**: Vague instructions → Agent didn't know exact tool names/parameters  
**After**: Concrete example → Agent sees exact JSON to use

The prompt now shows:
- Exact tool name to call
- Exact parameter format
- Exact workflow sequence
- Real example with coordinates

## Files Modified

- `hypr-claw-runtime/src/prompt_builder.rs` - Enhanced with concrete example

## Test Cases

```bash
# Test 1: YouTube video
> open youtube and play a random video

# Test 2: Generic click
> click on the first search result

# Test 3: Button click
> click the play button

# Test 4: Link click
> click on the settings link
```

All should now work with OCR → move → click workflow! 🎯
