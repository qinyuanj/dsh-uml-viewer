//! Window decoration choice and the undecorated window's own controls: with
//! no window-manager title bar, the drag strip carries minimise and close.

use viewer::app::{DRAG_STRIP, RESIZE_GRIP, WindowMode, mode_for, window_controls, window_mode};

#[test]
fn x11_is_decorated_and_its_absence_falls_back_to_undecorated_wayland() {
    assert_eq!(WindowMode::X11Decorated, mode_for(true, false));
    assert_eq!(WindowMode::WaylandUndecorated, mode_for(false, false));
}

#[test]
fn windows_and_macos_use_the_system_title_bar() {
    // Declared hosts: no X11 probe applies off Linux.
    assert_eq!(
        WindowMode::NativeDecorated,
        window_mode("windows", || false, false)
    );
    assert_eq!(
        WindowMode::NativeDecorated,
        window_mode("macos", || false, false)
    );
    assert_eq!(
        WindowMode::X11Decorated,
        window_mode("linux", || true, false)
    );
    assert_eq!(
        WindowMode::WaylandUndecorated,
        window_mode("linux", || false, false)
    );
}

#[test]
fn each_host_renders_on_its_own_gpu_path() {
    // wgpu prefers Vulkan on Windows when a Vulkan driver is installed;
    // the viewer's Windows backend is D3D12 (ADR 0010).
    assert_eq!(
        &[("WGPU_BACKEND", "dx12")][..],
        viewer::app::render_defaults("windows")
    );
    assert_eq!(
        &[("GALLIUM_DRIVER", "d3d12"), ("WGPU_BACKEND", "gl")][..],
        viewer::app::render_defaults("linux")
    );
    assert!(viewer::app::render_defaults("macos").is_empty(), "Metal");
}

#[test]
fn the_test_seam_forces_the_undecorated_window_on_every_host() {
    for os in ["windows", "macos", "linux"] {
        assert_eq!(
            WindowMode::WaylandUndecorated,
            window_mode(os, || true, true),
            "{os}"
        );
    }
}

#[test]
fn the_test_seam_forces_the_undecorated_window_even_when_x11_works() {
    assert_eq!(WindowMode::WaylandUndecorated, mode_for(true, true));
    assert_eq!(WindowMode::WaylandUndecorated, mode_for(false, true));
}

#[test]
fn the_controls_sit_in_the_drag_strip_at_the_right_clear_of_the_resize_grips() {
    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1500.0, 920.0));
    let [minimise, close] = window_controls(screen);
    assert!(
        close.max.x <= screen.max.x - RESIZE_GRIP,
        "close overlaps the east grip"
    );
    assert!(minimise.max.x <= close.min.x, "minimise sits left of close");
    for rect in [minimise, close] {
        assert!(
            rect.min.y >= screen.min.y + RESIZE_GRIP,
            "a control overlaps the north grip"
        );
        assert!(
            rect.max.y <= screen.min.y + DRAG_STRIP,
            "a control leaves the drag strip"
        );
        assert!(rect.width() >= 20.0, "a control is too narrow to hit");
    }
}

#[test]
fn the_controls_follow_an_offset_content_rect() {
    let screen = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(640.0, 480.0));
    let [_, close] = window_controls(screen);
    assert!(close.max.x <= screen.max.x - RESIZE_GRIP);
    assert!(close.min.y >= screen.min.y + RESIZE_GRIP);
}
