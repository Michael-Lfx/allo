// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, Node,
    NodeId, Point, Rect, Role, ScrollUnit, Tree, TreeId, TreeUpdate,
};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::E_INVALIDARG;

use super::*;

const WINDOW_TITLE: &str = "Scroll pattern test";

const WINDOW_ID: NodeId = NodeId(0);
const SCROLLABLE_ID: NodeId = NodeId(1);
const NON_SCROLLABLE_ID: NodeId = NodeId(2);

fn build_tree() -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_children(vec![SCROLLABLE_ID, NON_SCROLLABLE_ID]);

    let mut scrollable = Node::new(Role::ScrollView);
    scrollable.set_label("Scrollable view");
    scrollable.set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
    scrollable.set_clips_children();
    scrollable.set_scroll_y_min(0.0);
    scrollable.set_scroll_y_max(300.0);
    scrollable.set_scroll_y(75.0);
    scrollable.add_action(Action::ScrollUp);
    scrollable.add_action(Action::ScrollDown);
    scrollable.add_action(Action::SetScrollOffset);

    let mut non_scrollable = Node::new(Role::ScrollView);
    non_scrollable.set_label("Static view");
    non_scrollable.set_bounds(Rect::new(0.0, 100.0, 200.0, 200.0));
    non_scrollable.set_clips_children();
    non_scrollable.set_scroll_y_min(0.0);
    non_scrollable.set_scroll_y_max(0.0);
    non_scrollable.set_scroll_y(0.0);

    TreeUpdate {
        nodes: vec![
            (WINDOW_ID, root),
            (SCROLLABLE_ID, scrollable),
            (NON_SCROLLABLE_ID, non_scrollable),
        ],
        tree: Some(Tree::new(WINDOW_ID)),
        tree_id: TreeId::ROOT,
        focus: WINDOW_ID,
    }
}

struct ScrollActivationHandler;

impl ActivationHandler for ScrollActivationHandler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(build_tree())
    }
}

struct RecordingActionHandler {
    requests: Arc<Mutex<Vec<ActionRequest>>>,
}

impl ActionHandler for RecordingActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        self.requests.lock().unwrap().push(request);
    }
}

fn find_child_by_name(
    uia: &IUIAutomation,
    root: &IUIAutomationElement,
    expected: &str,
) -> Result<IUIAutomationElement> {
    let walker = unsafe { uia.ControlViewWalker() }?;
    let mut current = unsafe { walker.GetFirstChildElement(root) };
    while let Ok(element) = current {
        if let Ok(name) = unsafe { element.CurrentName() } {
            let name: String = name.try_into().unwrap_or_default();
            if name == expected {
                return Ok(element);
            }
        }
        current = unsafe { walker.GetNextSiblingElement(&element) };
    }
    Err(Error::empty())
}

#[test]
fn scroll_pattern_properties_and_actions() -> Result<()> {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let handler = RecordingActionHandler {
        requests: Arc::clone(&recorded),
    };

    scope(WINDOW_TITLE, ScrollActivationHandler {}, handler, |s| {
        let root = unsafe { s.uia.ElementFromHandle(s.window.0) }?;
        let scrollable = find_child_by_name(&s.uia, &root, "Scrollable view")?;
        let pattern: IUIAutomationScrollPattern =
            unsafe { scrollable.GetCurrentPatternAs(UIA_ScrollPatternId) }?;

        let v_scrollable: bool =
            unsafe { pattern.CurrentVerticallyScrollable() }?.into();
        let h_scrollable: bool =
            unsafe { pattern.CurrentHorizontallyScrollable() }?.into();
        assert!(v_scrollable);
        assert!(!h_scrollable);

        let v_pct = unsafe { pattern.CurrentVerticalScrollPercent() }?;
        let h_pct = unsafe { pattern.CurrentHorizontalScrollPercent() }?;
        assert!((v_pct - 25.0).abs() < 1e-6);
        assert_eq!(h_pct, UIA_ScrollPatternNoScroll);

        let v_view = unsafe { pattern.CurrentVerticalViewSize() }?;
        let h_view = unsafe { pattern.CurrentHorizontalViewSize() }?;
        assert!((v_view - 25.0).abs() < 1e-6);
        assert!((h_view - 100.0).abs() < 1e-6);

        unsafe {
            pattern.Scroll(ScrollAmount_NoAmount, ScrollAmount_SmallIncrement)
        }?;
        unsafe {
            pattern.Scroll(ScrollAmount_NoAmount, ScrollAmount_LargeDecrement)
        }?;
        unsafe { pattern.SetScrollPercent(UIA_ScrollPatternNoScroll, 50.0) }?;

        let invalid_pct = unsafe {
            pattern.SetScrollPercent(UIA_ScrollPatternNoScroll, 120.0)
        };
        assert_eq!(invalid_pct.unwrap_err().code(), E_INVALIDARG);

        let invalid_axis = unsafe {
            pattern.Scroll(ScrollAmount_SmallIncrement, ScrollAmount_NoAmount)
        };
        assert_eq!(
            invalid_axis.unwrap_err().code(),
            HRESULT(UIA_E_INVALIDOPERATION as _)
        );

        let static_view = find_child_by_name(&s.uia, &root, "Static view")?;
        let missing: Result<IUIAutomationScrollPattern> =
            unsafe { static_view.GetCurrentPatternAs(UIA_ScrollPatternId) };
        assert!(missing.is_err());

        let scroll_actions: Vec<_> = recorded
            .lock()
            .unwrap()
            .iter()
            .filter(|req| req.action != Action::Focus)
            .cloned()
            .collect();
        assert_eq!(
            scroll_actions.len(),
            3,
            "unexpected scroll actions: {scroll_actions:?}"
        );
        assert_eq!(scroll_actions[0].action, Action::ScrollDown);
        assert_eq!(
            scroll_actions[0].data,
            Some(ActionData::ScrollUnit(ScrollUnit::Item))
        );
        assert_eq!(scroll_actions[1].action, Action::ScrollUp);
        assert_eq!(
            scroll_actions[1].data,
            Some(ActionData::ScrollUnit(ScrollUnit::Page))
        );
        assert_eq!(scroll_actions[2].action, Action::SetScrollOffset);
        assert_eq!(
            scroll_actions[2].data,
            Some(ActionData::SetScrollOffset(Point::new(0.0, 150.0)))
        );

        Ok(())
    })
}
