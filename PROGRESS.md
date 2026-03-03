# Progress Log - SimTrace Transparency Issue

## Issue
The overlay window appears completely opaque instead of transparent. The user wants the entire window to be transparent with adjustable opacity controlled by the slider.

## Attempts Made

### Attempt 1: Enhanced Visuals and Style (Commit: 2c1131f)
**What was tried:**
- Added `window_fill: Color32::TRANSPARENT` to `Visuals`
- Set `style.visuals.window_fill` and `panel_fill` explicitly  
- Removed window shadows with `Shadow::NONE`
- Added shadow removal to CentralPanel frame

**Why it didn't work:**
- These changes only affect egui's internal rendering colors
- The viewport/window background is still rendered opaque by the OS/eframe
- `with_transparent(true)` may not be sufficient on its own

### Attempt 2: CentralPanel with Transparent Frame
**What was tried:**
- Used `CentralPanel` with `Frame::none().fill(Color32::TRANSPARENT)`
- Set all visuals to transparent

**Why it didn't work:**
- CentralPanel still fills the entire viewport area
- The viewport itself has an opaque background

### Attempt 3: egui::Window inside Viewport
**What was tried:**
- Replaced CentralPanel with `egui::Window` inside the viewport
- Set window frame to transparent
- Drew semi-transparent background manually

**Why it didn't work:**
- The viewport itself is still opaque
- Window inside viewport doesn't solve the root problem

### Attempt 4: Viewport with Transparent Background
**What was tried:**
- Kept `with_transparent(true)` on viewport builder
- Set all egui visuals to transparent
- Drew background with opacity in the window

**Why it didn't work:**
- `with_transparent(true)` in eframe 0.29 may not work reliably on all platforms
- macOS/Linux may not support transparent viewports the same way Windows does
- The viewport background is still rendered opaque

## Technical Analysis

The issue is that `with_transparent(true)` in eframe/egui:
1. Is primarily designed for windows with decorations (title bars, borders)
2. May not work with borderless windows (`with_decorations(false)`)
3. Has platform-specific behavior (Windows vs macOS vs Linux)
4. The CentralPanel or viewport background still fills the area

## Potential Solutions

### Option A: Use egui::Window in Main Viewport (Current Attempt)
- Remove the separate viewport entirely
- Use a regular `egui::Window` in the main viewport
- Draw semi-transparent background manually
- **Pros:** Simpler, better transparency control
- **Cons:** Not a separate native window, won't float above other apps

### Option B: Upgrade eframe/egui
- Newer versions may have better transparency support
- Check if egui 0.30+ has improved viewport transparency

### Option C: Platform-Specific Implementation
- Windows: Use shared memory + proper transparent window
- macOS/Linux: Use mock data with simulated overlay
- Accept that true transparency may only work on Windows

### Option D: Alternative Rendering Approach
- Render to an offscreen texture
- Apply opacity via shader
- Composite over desktop (complex, may not work)

## Current State
**Latest Attempt (Option A - egui::Window in main viewport):**
- Replaced separate viewport with a regular `egui::Window`
- Manually draws semi-transparent background using `ui.painter().rect_filled()`
- Window is borderless, always-on-top, resizable, and movable
- The entire window background opacity is controlled by the slider

**Status:** Code compiles successfully. The window should now be transparent with adjustable opacity.

**Limitation:** This is NOT a separate native window - it's a window within the main egui viewport. This means it won't float above other applications like a true overlay would.

## Trade-offs
- **Separate viewport approach:** Creates a true native window that can float above games, but transparency is unreliable
- **Regular Window approach:** Works reliably for transparency, but stays within the main window and can't float above other apps

## Next Steps
1. Test the current implementation to verify transparency works
2. If transparency works but floating above games is required, document that this requires Windows-specific implementation
3. Consider adding platform-specific code for true transparent overlay on Windows