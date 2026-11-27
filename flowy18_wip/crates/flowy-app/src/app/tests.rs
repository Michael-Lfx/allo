use super::*;
use gpui::{Focusable, TestAppContext, VisualTestContext, accesskit};
// gpui-pre never builds the accessibility tree in tests (no platform
// adapter activates it), so `Window::a11y_tree()` has no replacement of
// that shape. `gpui-kit`'s test support reads the same native role/label
// properties, but only for elements that registered themselves with
// `.test_support()` in production code. Until the row/button builders do
// that, these label queries return nothing and the assertions below
// report the missing label.
use gpui_kit::test::TestQueryExt;
use std::sync::atomic::{AtomicU64, Ordering};

fn test_root() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let path = std::env::temp_dir().join(format!(
            "flowy-app-notice-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            }
            Err(error) => {
                panic!("could not create test directory: {error}")
            }
        }
    }
}

#[test]
fn credits_balance_maps_to_a_ready_total_without_ui_side_effects() {
    use flowy_studio::CreditsViewState;

    assert_eq!(
        credits_state_from_balance(179_620),
        CreditsViewState::Ready {
            total: 179_620,
            free: None,
            daily: None,
        }
    );
    assert_eq!(
        credits_state_from_balance(-5),
        CreditsViewState::Ready {
            total: 0,
            free: None,
            daily: None,
        }
    );
    assert_eq!(
        credits_state_from_balance(i64::MAX),
        CreditsViewState::Ready {
            total: u64::try_from(i64::MAX).unwrap(),
            free: None,
            daily: None,
        }
    );
}

#[test]
fn catalog_refresh_gate_is_single_flight_for_idle_sessions() {
    assert!(catalog_refresh_allowed(false, true, false));
    assert!(!catalog_refresh_allowed(true, true, false));
    assert!(!catalog_refresh_allowed(false, false, false));
    assert!(!catalog_refresh_allowed(false, true, true));
}

#[gpui::test]
fn skills_and_personalization_are_direct_navigation_items(
    cx: &mut TestAppContext,
) {
    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, label: &str) {
        let bounds = cx.update(|window, _| {
            window
                .find_all_by_label(label)
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .unwrap_or_else(|| panic!("missing button {label:?}"))
                .bounds()
        });
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        draw(cx);
    }

    let root = test_root();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let mut app =
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx);
        app.screen = Screen::Main;
        app.section = Section::More;
        app.ota.checked = true;
        app
    });
    let (plugins, more, skills, personalization, automation, mine, explore) =
        cx.read_entity(&app, |app, _| {
            (
                app.t("dock.plugins"),
                app.t("dock.more"),
                app.t("more.skills"),
                app.t("more.personal"),
                app.t("dock.automation"),
                app.t("skills.mine"),
                app.t("skills.library_explore"),
            )
        });
    draw(cx);
    cx.update(|window, _| {
        for label in [&plugins, &more, &automation] {
            assert!(
                window.find_all_by_label(label).iter().all(|node| {
                    node.role() != Some(accesskit::Role::Button)
                        || !node.visible()
                }),
                "hidden navigation item {label:?} must not render",
            );
        }
        for label in [&skills, &personalization] {
            assert_eq!(
                window
                    .find_all_by_label(label)
                    .iter()
                    .filter(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .count(),
                1,
                "direct navigation item {label:?} must render once",
            );
        }
    });
    click(cx, &skills);
    cx.read_entity(&app, |app, _| assert_eq!(app.section, Section::Skills));
    click(cx, &mine);
    cx.read_entity(&app, |app, _| {
        assert_eq!(app.section, Section::MySkills);
        assert_eq!(app.active_dialog, state::ActiveDialog::None);
        assert_eq!(app.nav_back.last(), Some(&Section::Skills));
    });
    click(cx, &personalization);
    cx.update_entity(&app, super::FlowyApp::navigate_back);
    cx.read_entity(&app, |app, _| {
        assert_eq!(app.section, Section::MySkills);
    });
    cx.update_entity(&app, super::FlowyApp::navigate_back);
    cx.read_entity(&app, |app, _| assert_eq!(app.section, Section::Skills));
    cx.update_entity(&app, super::FlowyApp::navigate_forward);
    draw(cx);
    cx.read_entity(&app, |app, _| {
        assert_eq!(app.section, Section::MySkills);
    });
    click(cx, &explore);
    cx.read_entity(&app, |app, _| assert_eq!(app.section, Section::Skills));
    click(cx, &personalization);
    cx.read_entity(&app, |app, _| {
        assert_eq!(app.section, Section::More);
        assert_eq!(app.nav_back.last(), Some(&Section::Skills));
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn project_group_header_hover_covers_label_space_and_add_button(
    cx: &mut TestAppContext,
) {
    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }

    fn has_header_fill(
        cx: &mut VisualTestContext,
        sample: gpui::Point<gpui::Pixels>,
        color: gpui::Rgba,
    ) -> bool {
        cx.update(|window, _| {
            let sample = sample.scale(window.scale_factor());
            window.painted_quads().iter().any(|quad| {
                quad.bounds.contains(&sample)
                    && quad.background == gpui::Background::from(color)
            })
        })
    }

    fn new_project_bounds(
        cx: &mut VisualTestContext,
        label: &str,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        cx.update(|window, _| {
            window
                .find_all_by_label(label)
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .map(|node| node.bounds())
        })
    }

    for dark in [false, true] {
        let root = test_root();
        let folder = root.join("workspace");
        std::fs::create_dir_all(&folder).unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            let mut app = FlowyApp::new(
                SessionStore::new(root.clone()),
                dark,
                window,
                cx,
            );
            app.screen = Screen::Main;
            app.ota.checked = true;
            app
        });
        app.update(cx, |app, cx| {
            app.studio.update(cx, |panel, cx| {
                panel.create_project("Client", &folder, cx).unwrap();
            });
            let id = app.studio.read(cx).projects[0].id.clone();
            app.expanded_projects.insert(id);
            cx.notify();
        });
        for _ in 0..4 {
            draw(cx);
        }
        let (add_label, hover) = cx.read_entity(&app, |app, _| {
            (app.t("dock.new_project"), app.theme.dock_selected())
        });
        let header_bounds = cx
            .debug_bounds("project-group-header")
            .expect("project group header bounds");
        let project_bounds =
            cx.debug_bounds("project-row").expect("project row bounds");
        let nav_bounds = cx
            .debug_bounds("dock-nav-item")
            .expect("dock nav item bounds");
        assert_eq!(header_bounds.size, nav_bounds.size);
        assert_eq!(project_bounds.size, nav_bounds.size);
        assert_eq!(
            project_bounds.origin.y
                - (header_bounds.origin.y + header_bounds.size.height),
            px(2.)
        );
        let sample = gpui::point(px(130.), header_bounds.center().y);
        let project_sample = project_bounds.center();
        let outside = gpui::point(px(300.), px(80.));
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        draw(cx);
        assert!(!has_header_fill(cx, sample, hover));
        assert!(
            !has_header_fill(cx, project_sample, hover),
            "an expanded project row must not stay highlighted: dark={dark}"
        );
        assert!(
            cx.debug_bounds("project-actions").is_none(),
            "project actions stay hidden until hover: dark={dark}"
        );
        assert!(
            new_project_bounds(cx, &add_label).is_none(),
            "new project button stays hidden until the heading is hovered: dark={dark}"
        );
        cx.simulate_mouse_move(
            project_sample,
            None,
            gpui::Modifiers::default(),
        );
        draw(cx);
        assert!(
            has_header_fill(cx, project_sample, hover),
            "project row must highlight while hovered: dark={dark}"
        );
        let actions_bounds = cx
            .debug_bounds("project-actions")
            .expect("hovered project actions bounds");
        assert!(
            actions_bounds.size.width > px(0.),
            "project actions appear on hover: dark={dark}, bounds={actions_bounds:?}"
        );
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        draw(cx);
        assert!(!has_header_fill(cx, project_sample, hover));

        cx.simulate_mouse_move(sample, None, gpui::Modifiers::default());
        draw(cx);
        assert!(
            has_header_fill(cx, sample, hover),
            "project heading must highlight while hovered: dark={dark}"
        );
        let add_bounds = new_project_bounds(cx, &add_label)
            .expect("new project button appears when the heading is hovered");
        // The retired fork centered the add button exactly on the header row. Under
        // gpui-pre the row is 34.5px tall while the small icon button stays
        // 28px and sits ~3px below the centerline, so an exact equality no
        // longer holds. Keep the "same row" intent with a tolerance rather
        // than deleting the check.
        assert!(
            (add_bounds.center().y - header_bounds.center().y).abs() < px(4.),
            "the add button must sit on the header row: \
             add={add_bounds:?} header={header_bounds:?}"
        );
        for pointer in
            [gpui::point(px(24.), sample.y), sample, add_bounds.center()]
        {
            cx.simulate_mouse_move(pointer, None, gpui::Modifiers::default());
            draw(cx);
            assert!(
                has_header_fill(cx, sample, hover),
                "project heading must highlight across its entire row: dark={dark}, pointer={pointer:?}"
            );
            assert!(
                new_project_bounds(cx, &add_label).is_some(),
                "new project button stays visible across the heading: dark={dark}, pointer={pointer:?}"
            );
        }
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        draw(cx);
        assert!(!has_header_fill(cx, sample, hover));
        assert!(
            new_project_bounds(cx, &add_label).is_none(),
            "new project button hides when the pointer leaves the heading: dark={dark}"
        );
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}

#[gpui::test]
fn conversation_more_button_appears_only_while_its_row_is_hovered(
    cx: &mut TestAppContext,
) {
    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
        cx.run_until_parked();
    }

    fn button_bounds(
        cx: &mut VisualTestContext,
        label: &str,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        cx.update(|window, _| {
            window
                .find_all_by_label(label)
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .map(|node| node.bounds())
        })
    }

    fn contains_box(
        outer: gpui::Bounds<gpui::Pixels>,
        inner: gpui::Bounds<gpui::Pixels>,
    ) -> bool {
        let pad = px(1.);
        inner.origin.x + pad >= outer.origin.x
            && inner.origin.y + pad >= outer.origin.y
            && inner.origin.x + inner.size.width
                <= outer.origin.x + outer.size.width + pad
            && inner.origin.y + inner.size.height
                <= outer.origin.y + outer.size.height + pad
    }

    let root = test_root();
    let folder = root.join("workspace");
    std::fs::create_dir_all(&folder).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let mut app =
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx);
        app.screen = Screen::Main;
        app.ota.checked = true;
        app
    });
    app.update(cx, |app, cx| {
        app.studio.update(cx, |panel, cx| {
            panel.create_project("Client", &folder, cx).unwrap();
            panel.agent.create_session().unwrap();
            panel.agent.set_workspace_for_new_sessions(None);
            panel.agent.create_session().unwrap();
        });
        let id = app.studio.read(cx).projects[0].id.clone();
        app.expanded_projects.insert(id);
        cx.notify();
    });
    let more = cx.read_entity(&app, |app, _| app.t("library.more"));
    draw(cx);

    let outside = gpui::point(px(300.), px(80.));
    cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("task-actions").is_none(),
        "conversation more button stays hidden until the row is hovered"
    );
    assert!(button_bounds(cx, &more).is_none());

    let project = cx.debug_bounds("project-row").expect("project row");
    cx.simulate_mouse_move(project.center(), None, gpui::Modifiers::default());
    draw(cx);
    let project_actions = cx
        .debug_bounds("project-actions")
        .expect("project more button appears on hover");
    let project_edit = cx
        .debug_bounds("project-edit")
        .expect("project edit button appears on hover");
    assert!(
        contains_box(project, project_actions),
        "project hover fill must contain its more button: row={project:?} actions={project_actions:?}"
    );
    cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
    draw(cx);

    let row = cx.debug_bounds("task-row").expect("conversation row");
    cx.simulate_mouse_move(row.center(), None, gpui::Modifiers::default());
    draw(cx);
    let actions = cx
        .debug_bounds("task-actions")
        .expect("conversation more button appears on hover");
    assert!(
        contains_box(row, actions),
        "conversation hover fill must contain its more button: row={row:?} actions={actions:?}"
    );
    assert!(
        (actions.origin.x - project_edit.origin.x).abs() < px(1.)
            && (actions.size.width - project_edit.size.width).abs() < px(1.),
        "conversation more button must line up with the project edit button: conversation={actions:?} edit={project_edit:?}"
    );
    cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("task-actions").is_none(),
        "conversation more button hides when the pointer leaves the row"
    );

    let project_task = cx
        .debug_bounds("project-task-row")
        .expect("project conversation row");
    cx.simulate_mouse_move(
        project_task.center(),
        None,
        gpui::Modifiers::default(),
    );
    draw(cx);
    let nested_actions = cx
        .debug_bounds("task-actions")
        .expect("project conversation more button appears on hover");
    assert!(
        contains_box(project_task, nested_actions),
        "project conversation hover fill must contain its more button"
    );
    assert!(
        (nested_actions.origin.x - project_edit.origin.x).abs() < px(1.)
            && (nested_actions.size.width - project_edit.size.width).abs()
                < px(1.),
        "nested conversation more button must line up with the project edit button: nested={nested_actions:?} edit={project_edit:?}"
    );
    cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("task-actions").is_none(),
        "project conversation more button hides when the pointer leaves"
    );

    let row = cx.debug_bounds("task-row").expect("conversation row");
    cx.simulate_mouse_move(row.center(), None, gpui::Modifiers::default());
    draw(cx);
    let more_bounds =
        button_bounds(cx, &more).expect("more button is reachable");

    cx.simulate_click(more_bounds.center(), gpui::Modifiers::default());
    draw(cx);
    cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("task-actions").is_some(),
        "more button stays while its menu is open"
    );
    cx.read_entity(&app, |app, _| {
        assert!(app.task_menu.is_some());
    });

    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn project_and_task_sections_collapse_and_expand(cx: &mut TestAppContext) {
    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, label: &str) {
        let bounds = cx.update(|window, _| {
            window
                .find_all_by_label(label)
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .unwrap_or_else(|| panic!("missing button {label:?}"))
                .bounds()
        });
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        draw(cx);
    }

    fn button_count(cx: &mut VisualTestContext, label: &str) -> usize {
        cx.update(|window, _| {
            window
                .find_all_by_label(label)
                .iter()
                .filter(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .count()
        })
    }

    let root = test_root();
    let folder = root.join("workspace");
    std::fs::create_dir_all(&folder).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let mut app =
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx);
        app.screen = Screen::Main;
        app.ota.checked = true;
        app
    });
    app.update(cx, |app, cx| {
        app.studio.update(cx, |panel, cx| {
            panel.create_project("Client", &folder, cx).unwrap();
            panel.agent.set_workspace_for_new_sessions(None);
            panel.agent.create_session().unwrap();
        });
        cx.notify();
    });
    draw(cx);

    let (
        collapse_projects,
        expand_projects,
        collapse_tasks,
        expand_tasks,
        task_label,
    ) = cx.read_entity(&app, |app, _| {
        let projects = app.t("dock.projects");
        let tasks = app.t("dock.tasks");
        (
            app.tf("dock.collapse", &[("name", &projects)]),
            app.tf("dock.expand", &[("name", &projects)]),
            app.tf("dock.collapse", &[("name", &tasks)]),
            app.tf("dock.expand", &[("name", &tasks)]),
            app.t("dock.new_task"),
        )
    });
    assert!(cx.debug_bounds("project-row").is_some());
    let tasks_open = button_count(cx, &task_label);
    assert!(
        tasks_open > 1,
        "a loose task should render under the tasks heading"
    );

    click(cx, &collapse_projects);
    cx.read_entity(&app, |app, _| {
        assert!(!app.projects_open);
        assert!(app.tasks_open);
    });
    assert!(cx.debug_bounds("project-row").is_none());
    assert_eq!(button_count(cx, &task_label), tasks_open);

    click(cx, &expand_projects);
    cx.read_entity(&app, |app, _| assert!(app.projects_open));
    assert!(cx.debug_bounds("project-row").is_some());

    click(cx, &collapse_tasks);
    cx.read_entity(&app, |app, _| {
        assert!(app.projects_open);
        assert!(!app.tasks_open);
    });
    assert!(cx.debug_bounds("project-row").is_some());
    assert!(
        button_count(cx, &task_label) < tasks_open,
        "collapsing tasks hides the loose task row"
    );

    click(cx, &expand_tasks);
    cx.read_entity(&app, |app, _| assert!(app.tasks_open));
    assert_eq!(button_count(cx, &task_label), tasks_open);

    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn project_create_dialog_exposes_fields_and_cancel_keeps_registry_empty(
    cx: &mut TestAppContext,
) {
    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, label: &str) {
        let bounds = cx.update(|window, _| {
            window
                .find_all_by_label(label)
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .unwrap_or_else(|| panic!("missing button {label:?}"))
                .bounds()
        });
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        draw(cx);
    }

    let root = test_root();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let mut app =
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx);
        app.screen = Screen::Main;
        app.ota.checked = true;
        app
    });
    let (new_project, name, choose_folder, create, cancel) =
        cx.read_entity(&app, |app, _| {
            (
                app.t("dock.new_project"),
                app.t("project.name"),
                app.t("project.folder"),
                app.t("project.create"),
                app.t("more.cancel"),
            )
        });
    draw(cx);
    let header = cx
        .debug_bounds("project-group-header")
        .expect("project group header bounds");
    cx.simulate_mouse_move(header.center(), None, gpui::Modifiers::default());
    draw(cx);
    click(cx, &new_project);
    cx.read_entity(&app, |app, _| {
        assert_eq!(app.active_dialog, state::ActiveDialog::ProjectCreate);
    });
    cx.update(|window, _| {
        // The retired fork exposed the text editor as an `accesskit::Role::TextInput`
        // node. The gpui-kit `Input` behind `theme::TextField` registers the
        // field frame with its accessible name but no role, and no
        // `TextInput` role exists anywhere in the tree, so the role half of
        // the original assertion is no longer expressible. Assert the
        // labelled, laid-out field instead.
        assert!(
            window.find_all_by_label(&name).iter().any(|node| {
                node.visible() && node.bounds().size.width > px(0.)
            }),
            "missing project dialog name field"
        );
        for label in [&choose_folder, &create, &cancel] {
            assert!(
                window.find_all_by_label(label).iter().any(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                }),
                "missing project dialog control {label:?}"
            );
        }
    });
    click(cx, &cancel);
    cx.read_entity(&app, |app, cx| {
        assert_eq!(app.active_dialog, state::ActiveDialog::None);
        assert_eq!(app.studio.read(cx).projects, []);
    });
    assert!(!root.join("projects.json").exists());
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn project_row_menu_pins_and_unregisters_without_deleting_folder(
    cx: &mut TestAppContext,
) {
    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, label: &str) {
        let bounds = cx.update(|window, _| {
            window
                .find_all_by_label(label)
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .unwrap_or_else(|| panic!("missing button {label:?}"))
                .bounds()
        });
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        draw(cx);
    }

    let root = test_root();
    let folder = root.join("workspace");
    std::fs::create_dir_all(&folder).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let mut app =
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx);
        app.screen = Screen::Main;
        app.ota.checked = true;
        app
    });
    let project = app.update(cx, |app, cx| {
        app.studio.update(cx, |panel, cx| {
            panel.create_project("Client", &folder, cx).unwrap()
        })
    });
    let _project_session = cx.read_entity(&app, |app, cx| {
        app.studio.read(cx).agent.create_session().unwrap()
    });
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.expanded_projects.insert(project.id.clone());
            cx.notify();
        });
    });
    draw(cx);
    fn hover_row(cx: &mut VisualTestContext) {
        let center = cx.update(|window, _| {
            window
                .find_all_by_label("Client")
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .expect("project name")
                .bounds()
                .center()
        });
        cx.simulate_mouse_move(center, None, gpui::Modifiers::default());
        draw(cx);
    }
    hover_row(cx);

    let (options, pin, expand, new_task) = cx.read_entity(&app, |app, _| {
        (
            app.t("project.menu"),
            app.t("project.pin"),
            app.tf("project.collapse", &[("name", "Client")]),
            app.t("project.new_task"),
        )
    });
    for label in [&options, &expand, &new_task] {
        cx.update(|window, _| {
            assert!(
                window.find_all_by_label(label).iter().any(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                }),
                "missing project row control {label:?}"
            );
        });
    }
    let nested_task = cx.read_entity(&app, |app, _| app.t("dock.new_task"));
    cx.update(|window, _| {
        assert!(
            window.find_all_by_label(&nested_task).iter().any(|node| {
                node.role() == Some(accesskit::Role::Button) && node.visible()
            }),
            "the project's session should render inside its expanded group"
        );
    });

    click(cx, &options);
    cx.update(|window, _| {
        assert!(window.find_all_by_label(&pin).iter().any(|node| {
            node.role() == Some(accesskit::Role::Button) && node.visible()
        }));
    });
    click(cx, &pin);
    cx.read_entity(&app, |app, cx| {
        assert!(app.studio.read(cx).projects[0].pinned);
    });

    hover_row(cx);
    click(cx, &options);
    let delete = cx.read_entity(&app, |app, _| app.t("project.delete"));
    click(cx, &delete);
    cx.read_entity(&app, |app, _| {
        assert_eq!(app.active_dialog, state::ActiveDialog::ProjectDelete);
    });
    let cancel = cx.read_entity(&app, |app, _| app.t("more.cancel"));
    click(cx, &cancel);
    hover_row(cx);
    click(cx, &options);
    click(cx, &delete);
    click(cx, &delete);
    cx.read_entity(&app, |app, cx| {
        assert_eq!(app.studio.read(cx).projects, []);
    });
    assert!(folder.is_dir(), "project removal must preserve local files");
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn project_new_task_click_adds_a_blank_task(cx: &mut TestAppContext) {
    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
        cx.run_until_parked();
    }

    let root = test_root();
    let folder = root.join("workspace");
    std::fs::create_dir_all(&folder).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let mut app =
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx);
        app.screen = Screen::Main;
        app.ota.checked = true;
        app
    });
    let project_id = app.update(cx, |app, cx| {
        app.studio.update(cx, |panel, cx| {
            panel
                .create_project("Client", &folder, cx)
                .expect("project")
                .id
        })
    });
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.expanded_projects.insert(project_id.clone());
            cx.notify();
        });
    });
    draw(cx);

    let name = cx.update(|window, _| {
        window
            .find_all_by_label("Client")
            .into_iter()
            .find(|node| {
                node.role() == Some(accesskit::Role::Button) && node.visible()
            })
            .expect("project name")
            .bounds()
            .center()
    });
    let modifiers = gpui::Modifiers::default();
    cx.simulate_mouse_move(name, None, modifiers);
    draw(cx);

    let label = cx.read_entity(&app, |app, _| app.t("project.new_task"));
    let button = cx.update(|window, _| {
        window
            .find_all_by_label(&label)
            .into_iter()
            .filter(|node| {
                node.role() == Some(accesskit::Role::Button) && node.visible()
            })
            .map(|node| node.bounds())
            .min_by(|left, right| {
                left.size
                    .width
                    .partial_cmp(&right.size.width)
                    .expect("button width")
            })
            .expect("project new task button")
            .center()
    });
    cx.simulate_mouse_move(button, None, modifiers);
    draw(cx);
    cx.simulate_mouse_down(button, gpui::MouseButton::Left, modifiers);
    cx.simulate_mouse_up(button, gpui::MouseButton::Left, modifiers);
    draw(cx);

    cx.read_entity(&app, |app, cx| {
        let panel = app.studio.read(cx);
        let sessions = panel.agent.list_sessions();
        assert_eq!(
            sessions.len(),
            0,
            "clicking project + must defer Router creation until first send (0 empty sessions)"
        );
        assert!(panel.agent.active_session_id().is_none());
        assert_eq!(
            panel.active_project_id.as_deref(),
            Some(project_id.as_str())
        );
        assert_eq!(
            panel
                .agent
                .active_workspace()
                .map(|ws| ws.project_id),
            Some(project_id.clone())
        );
        assert_eq!(panel.surface, StudioSurface::Home);
        assert_eq!(app.section, Section::Studio);
        assert!(app.expanded_projects.contains(&project_id));
    });
    // Clicking project `+` a second time still creates zero empty sessions.
    cx.simulate_mouse_down(button, gpui::MouseButton::Left, modifiers);
    cx.simulate_mouse_up(button, gpui::MouseButton::Left, modifiers);
    draw(cx);
    cx.read_entity(&app, |app, cx| {
        assert_eq!(
            app.studio.read(cx).agent.list_sessions().len(),
            0,
            "repeated project + clicks must never leave empty sessions"
        );
    });
    let focused = cx.update(|window, cx| {
        let handle = app
            .read(cx)
            .studio
            .read(cx)
            .prompt
            .read(cx)
            .focus_handle(cx);
        handle.is_focused(window)
    });
    assert!(focused, "the composer should take focus");
    // Once a task actually starts in the project, its sidebar row aligns
    // cleanly with the dock navigation row's left edge.
    app.update(cx, |app, cx| {
        app.studio.update(cx, |panel, cx| {
            let project = panel
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .cloned()
                .expect("created project exists");
            let sid = panel
                .agent
                .create_router_project_session(flowy_agent::AgentWorkspace {
                    project_id: project.id,
                    root: project.root,
                })
                .expect("create project router task");
            panel.open_session(&sid, cx);
        });
    });
    draw(cx);
    let task_label = cx.read_entity(&app, |app, _| app.t("dock.new_task"));
    let (nav_left, task_left) = cx.update(|window, _| {
        let mut rows: Vec<_> = window
            .find_all_by_label(&task_label)
            .into_iter()
            .filter(|node| {
                node.role() == Some(accesskit::Role::Button) && node.visible()
            })
            .map(|node| node.bounds())
            .collect();
        rows.sort_by(|left, right| {
            left.origin.y.partial_cmp(&right.origin.y).expect("row y")
        });
        assert!(
            rows.len() >= 2,
            "the dock and the project should each show the new task"
        );
        (rows[0].left(), rows[rows.len() - 1].left())
    });
    assert!(
        (nav_left - task_left).abs() < gpui::px(1.),
        "project task highlight should share the dock row's left edge"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn project_name_and_icon_toggle_expansion(cx: &mut TestAppContext) {
    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, label: &str) {
        let bounds = cx.update(|window, _| {
            window
                .find_all_by_label(label)
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .unwrap_or_else(|| panic!("missing button {label:?}"))
                .bounds()
        });
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        draw(cx);
    }

    let root = test_root();
    let folder = root.join("workspace");
    std::fs::create_dir_all(&folder).unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let mut app =
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx);
        app.screen = Screen::Main;
        app.ota.checked = true;
        app
    });
    let project_id = app.update(cx, |app, cx| {
        app.studio.update(cx, |panel, cx| {
            panel
                .create_project("Client", &folder, cx)
                .expect("project")
                .id
        })
    });
    draw(cx);

    let name = "Client";
    let expanded = |cx: &mut VisualTestContext| {
        cx.read_entity(&app, |app, _| {
            app.expanded_projects.contains(&project_id)
        })
    };
    assert!(!expanded(cx), "a new project starts collapsed");

    click(cx, name);
    assert!(expanded(cx), "the project name expands its tasks");
    click(cx, name);
    assert!(!expanded(cx), "the project name collapses its tasks");

    let expand = cx.read_entity(&app, |app, _| {
        app.tf("project.expand", &[("name", name)])
    });
    click(cx, &expand);
    assert!(expanded(cx), "the folder icon expands its tasks");
    let collapse = cx.read_entity(&app, |app, _| {
        app.tf("project.collapse", &[("name", name)])
    });
    click(cx, &collapse);
    assert!(!expanded(cx), "the folder icon collapses its tasks");

    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn unavailable_or_locked_tiers_never_change_the_selected_route(
    cx: &mut TestAppContext,
) {
    let root = test_root();
    let app = cx
        .add_window_view(|window, cx| {
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
        })
        .0;
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            // Inject the session after construction so the startup refresh
            // never opens a network request in tests.
            app.auth.session = Some(flowy_auth::Session {
                access_token: "test-token".into(),
                email: "test@example.com".into(),
                display_name: "test".into(),
                user_id: Some(1),
            });
            app.prefs.model = flowy_agent::Model::Lite;
            app.effective_tier = 0;

            // The mapped id is missing: unavailable must not move the
            // selection or pretend the route was applied.
            app.select_tier(flowy_agent::Model::Standard, cx);
            assert_eq!(app.prefs.model, flowy_agent::Model::Lite);
            assert_eq!(app.notice, Some("notice.model_unavailable"));
            assert_eq!(app.applied_tier, None);

            app.studio.update(cx, |panel, _| {
                panel.catalog.replace(vec![flowy_agent::CatalogModel {
                    id: "AIPC-deepseek-v4.1-flash".into(),
                    name: "deepseek".into(),
                    endpoint: "https://example.com/claw/v1".into(),
                    ..flowy_agent::CatalogModel::default()
                }]);
            });
            // Present but gated: a Free account must stay on its route.
            app.select_tier(flowy_agent::Model::Max, cx);
            assert_eq!(app.prefs.model, flowy_agent::Model::Lite);
            assert_eq!(app.notice, Some("notice.upgrade_later"));
            assert_eq!(app.applied_tier, None);
        });
    });
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn new_task_reconciles_deferred_studio_notice(cx: &mut TestAppContext) {
    let root = test_root();
    let app = cx
        .add_window_view(|window, cx| {
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
        })
        .0;
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.studio.update(cx, |panel, cx| {
                panel.note("notice.attach_later", cx);
            });
        });
    });
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            assert_eq!(app.notice, Some("notice.attach_later"));
            app.select_navigation("new-task", Section::Studio, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let app = app.read(cx);
        assert_eq!(app.notice, None);
        assert_eq!(app.studio.read(cx).notice, None);
    });
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn storage_error_survives_login_settings_and_navigation_until_dismissed(
    cx: &mut TestAppContext,
) {
    let root = test_root();
    let app = cx
        .add_window_view(|window, cx| {
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
        })
        .0;
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            assert_eq!(app.screen, Screen::Login);
            app.studio.update(cx, |panel, cx| {
                panel.note("notice.session_storage_failed", cx);
            });
        });
    });
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            assert!(app.storage_error);
            app.screen = Screen::Main;
            app.activate_dialog(state::ActiveDialog::Settings, cx);
            app.select_navigation("automation", Section::Automation, cx);
            app.select_navigation("new-task", Section::Studio, cx);
        });
    });
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            assert!(app.storage_error);
            app.dismiss_storage_error(cx);
            assert!(!app.storage_error);
        });
    });
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn failed_new_task_is_not_hidden_by_navigation(cx: &mut TestAppContext) {
    let root = test_root();
    let sessions = root.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    // A broken index must be quarantined and rebuilt from structured history.
    std::fs::write(sessions.join("index.json"), b"{broken").unwrap();
    std::fs::write(
        sessions.join("history.json"),
        br#"{
            "active": "hist-1",
            "sessions": [
                {
                    "id": "hist-1",
                    "title": "Recovered",
                    "titled": true,
                    "updated_at": 1,
                    "status": { "kind": "idle" },
                    "messages": [],
                    "todos": []
                }
            ]
        }"#,
    )
    .unwrap();
    let app = cx
        .add_window_view(|window, cx| {
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
        })
        .0;
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.select_navigation("new-task", Section::Studio, cx);
        });
    });
    // The storage error is delivered by the 33ms Studio pump; advance the
    // test executor until it lands instead of assuming one fixed tick.
    let mut delivered = false;
    for _ in 0..8 {
        cx.run_until_parked();
        cx.background_executor
            .advance_clock(Duration::from_millis(33));
        cx.run_until_parked();
        delivered = cx.update(|cx| app.read(cx).storage_error);
        if delivered {
            break;
        }
    }
    assert!(delivered, "storage error should reach the app via the pump");
    // Navigation after the notice arrived must not clear it.
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.select_navigation("automation", Section::Automation, cx);
        });
    });
    assert!(cx.update(|cx| app.read(cx).storage_error));
    // The damaged original is kept under a corrupt-* name.
    let quarantined: Vec<_> = std::fs::read_dir(&sessions)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("index.corrupt-")
        })
        .collect();
    assert_eq!(quarantined.len(), 1, "broken index should be quarantined");
    assert_eq!(std::fs::read(quarantined[0].path()).unwrap(), b"{broken");
    // A fresh index is rebuilt and keeps the session recovered from history.
    let rebuilt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(sessions.join("index.json")).unwrap(),
    )
    .expect("rebuilt index should be valid JSON");
    let ids: Vec<&str> = rebuilt["sessions"]
        .as_array()
        .expect("rebuilt index has sessions")
        .iter()
        .filter_map(|session| session["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"hist-1"),
        "history session should survive the rebuild: {ids:?}"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn quick_search_opens_modal_without_polluting_nav_history_and_matches_sessions(
    cx: &mut TestAppContext,
) {
    let root = test_root();
    let sessions = root.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join("history.json"),
        br#"{
            "active": "s-1",
            "sessions": [
                {
                    "id": "s-1",
                    "title": "Q3 Revenue Deck",
                    "titled": true,
                    "updated_at": 1700000001000,
                    "status": { "kind": "idle" },
                    "messages": [
                        {
                            "id": "m-1",
                            "role": "user",
                            "content": [
                                { "type": "text", "text": "Prepare **margin** waterfall for Board" }
                            ],
                            "source": { "kind": "user" }
                        }
                    ],
                    "todos": []
                },
                {
                    "id": "s-2",
                    "title": "Weekend Trip",
                    "titled": true,
                    "updated_at": 1700000002000,
                    "status": { "kind": "idle" },
                    "messages": [],
                    "todos": []
                }
            ]
        }"#,
    )
    .unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
    });
    cx.run_until_parked();

    // Opening Quick Search must NOT push a placeholder section onto nav_back.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.screen = Screen::Main;
            let prev_section = app.section;
            app.open_search(window, cx);
            assert_eq!(app.active_dialog, state::ActiveDialog::QuickSearch);
            assert_eq!(app.section, prev_section);
            assert_eq!(app.nav_back, Vec::new());
            let query = app.quick_search.query.clone();
            app.set_input_text(&query, "waterfall", cx);
        });
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            let rows = app.quick_search_results(cx);
            assert!(
                rows.iter().any(|row| row.title == "Q3 Revenue Deck"),
                "message body search should match Q3 Revenue Deck: {rows:?}"
            );
            assert!(
                rows.iter().all(|row| row.title != "Weekend Trip"),
                "unrelated session should be filtered out"
            );
            app.confirm_quick_search(cx);
            assert_eq!(app.active_dialog, state::ActiveDialog::None);
        });
    });
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn session_context_menu_supports_point_anchor_pin_rename_and_archive(
    cx: &mut TestAppContext,
) {
    let root = test_root();
    let sessions = root.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join("history.json"),
        br#"{
            "active": "s-ctx",
            "sessions": [
                {
                    "id": "s-ctx",
                    "title": "Draft Plan",
                    "titled": true,
                    "updated_at": 1700000001000,
                    "status": { "kind": "idle" },
                    "messages": [],
                    "todos": []
                }
            ]
        }"#,
    )
    .unwrap();
    let (app, cx) = cx.add_window_view(|window, cx| {
        FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.screen = Screen::Main;
            // Titlebar dock toggle flips visibility cleanly.
            assert!(app.dock_visible);
            app.toggle_dock(cx);
            assert!(!app.dock_visible);
            app.toggle_dock(cx);
            assert!(app.dock_visible);

            // Point-anchored right-click menu sets sticky anchor + state.
            app.open_task_menu_at("s-ctx", gpui::point(px(180.), px(240.)), cx);
            let menu = app.task_menu.as_ref().expect("task menu open");
            assert_eq!(menu.session_id, "s-ctx");
            assert!(
                app.ui
                    .overlays
                    .read(cx)
                    .anchor_bounds(&menu.anchor_id)
                    .is_some()
            );

            // Pin, rename, and archive persist through AgentHandle.
            app.toggle_session_pinned("s-ctx", true, cx);
            let list = app.studio.read(cx).agent.list_sessions();
            assert!(list.iter().find(|s| s.id == "s-ctx").unwrap().pinned);

            app.open_conversation_rename("s-ctx", cx);
            assert_eq!(
                app.active_dialog,
                state::ActiveDialog::ConversationRename
            );
            let input = app.conversation_rename.input.clone();
            app.set_input_text(&input, "Final Launch Plan", cx);
        });
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.confirm_conversation_rename(cx);
            assert_eq!(app.active_dialog, state::ActiveDialog::None);
            let list = app.studio.read(cx).agent.list_sessions();
            assert_eq!(
                list.iter().find(|s| s.id == "s-ctx").unwrap().title,
                "Final Launch Plan"
            );

            app.toggle_session_archived("s-ctx", true, cx);
            let list = app.studio.read(cx).agent.list_sessions();
            assert!(list.iter().find(|s| s.id == "s-ctx").unwrap().archived);
        });
    });
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn receive_invite_code_on_main_preserves_deep_link_status(
    cx: &mut TestAppContext,
) {
    let root = test_root();
    let (app, cx) = cx.add_window_view(|window, cx| {
        FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
    });
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.screen = Screen::Main;
            app.receive_invite_code("INV123".into(), cx);
            assert_eq!(app.active_dialog, state::ActiveDialog::Invite);
            assert_eq!(app.invite.inbound_code.as_deref(), Some("INV123"));
            assert_eq!(app.invite.status, Some("invite.deep_link_received"));
            assert!(!app.invite.pending_open);
        });
    });
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
fn receive_invite_code_on_login_defers_dialog_until_main(
    cx: &mut TestAppContext,
) {
    let root = test_root();
    let (app, cx) = cx.add_window_view(|window, cx| {
        FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
    });
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            assert_eq!(app.screen, Screen::Login);
            app.receive_invite_code("LOGIN789".into(), cx);
            assert_eq!(app.active_dialog, state::ActiveDialog::None);
            assert_eq!(app.invite.inbound_code.as_deref(), Some("LOGIN789"));
            assert_eq!(app.invite.status, Some("invite.deep_link_received"));
            assert!(app.invite.pending_open);

            app.screen = Screen::Main;
            app.flush_pending_invite(cx);
            assert_eq!(app.active_dialog, state::ActiveDialog::Invite);
            assert_eq!(app.invite.status, Some("invite.deep_link_received"));
            assert!(!app.invite.pending_open);
        });
    });
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
