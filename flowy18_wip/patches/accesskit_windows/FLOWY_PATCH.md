# Local Patch: `accesskit_windows 0.34.0`

- **Upstream crate**: `accesskit_windows = "0.34.0"` (`https://github.com/AccessKit/accesskit`)
- **Upstream license**: `MIT OR Apache-2.0` (copyright headers preserved verbatim in all source files)
- **Modified / Added files**:
  - `src/node.rs`: Implement Windows UI Automation `IScrollProvider` (`UIA_ScrollPatternId`) on `PlatformNode` and project `scroll_x/y`, `scroll_x/y_min`, `scroll_x/y_max`, and `bounds` into UIA scroll properties and `Scroll` / `SetScrollPercent` actions.
  - `src/tests/mod.rs` & `src/tests/scroll.rs`: Add real Windows UIA client COM integration tests covering `IUIAutomationScrollPattern`.
- **Exit criterion**: Remove this `[patch.crates-io]` override once Flowy upgrades to an upstream `accesskit_windows` release with native `IScrollProvider` support.
