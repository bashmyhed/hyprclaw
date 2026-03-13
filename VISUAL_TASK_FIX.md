# Visual Task Workflow Enhancement

## Issue

Agent was opening YouTube but couldn't click on videos - it was just searching instead of using OCR to find and click elements.

## Fix

Enhanced the system prompt to explicitly guide the agent through visual tasks:

```
VISUAL TASKS (clicking on screen elements):
1. Use desktop.ocr_screen to see what's on screen
2. Find target element coordinates from OCR results
3. Use desktop.mouse_move to move to coordinates
4. Use desktop.mouse_click to click
```

## Testing

```bash
cargo build --release
cargo run

> open youtube and play a random video
```

The agent should now:
1. Open YouTube (✓ already working)
2. Use `desktop.ocr_screen` to see the page
3. Find video thumbnail coordinates
4. Move mouse to coordinates
5. Click on the video

## Available Visual Tools

- `desktop.ocr_screen` - Read all text on screen with coordinates
- `desktop.mouse_move` - Move cursor to x,y position
- `desktop.mouse_click` - Click at current position
- `desktop.screenshot` - Take screenshot (if available)

## Workflow Example

```
User: "click on the first video"

Agent:
1. desktop.ocr_screen → Gets all text with coordinates
2. Finds "video title" at (x: 300, y: 400)
3. desktop.mouse_move {"x": 300, "y": 400}
4. desktop.mouse_click {"button": "left"}
5. Confirms: "Clicked on the video"
```

## Files Modified

- `hypr-claw-runtime/src/prompt_builder.rs` - Added visual task workflow

## Next Steps

Test with various visual tasks:
- "click on the search button"
- "click on the first result"
- "click on the play button"

The agent now has explicit instructions for visual interaction! 🎯
